//! 仕掛台帳（LOTNOW.sqlite3）。旧版 master.py と同じ答えを、止まらず・速く出す。仕様は program/docs/CM3_API_spec.md の 6 章。
//!
//! 1. 共有の元ファイルを手元へ写す（共有の物を SQLite で開かない: 更新する側を錠で止めない）。大きさ＋更新時刻が変わったときだけ
//! 2. 手元の写しを読み取り専用で開き、表をすべてメモリへ読む（1 行 = 1 つの文字列と区切りの位置。Python の辞書の 1/10 ほどの大きさ）
//! 3. ロット番号の列を、列名と実データから決める（列ごとに糸を分けて並列に調べる）
//! 4. 突合せキー（スケジュールの項目 = 台帳の列。複数の組はすべて一致）で引ける索引を作る
//!
//! 旧版との違い（答えは同じ）: 写しと読み込みは**裏の糸**（`spawn`）。問い合わせは読み終えた控え（Snapshot）を使うので、
//! 共有から写している間・読み込んでいる間も待たない。キーを変えたときはメモリの行から索引だけを作り直す（SQLite を読み直さない）。

use crate::pyfmt::sqlite_str;
use crate::text::{display_value, label, norm_key, norm_name};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime};

/// 「ロット番号」として扱う列名（そろえてから比べる）
const KEY_NAME_CANDIDATES: [&str; 12] = [
    "ロット番号",
    "ﾛｯﾄ番号",
    "ロットNo",
    "ﾛｯﾄNo",
    "ロットNO",
    "ロットＮｏ",
    "ロットNO.",
    "ロット",
    "LotNo",
    "LOT_NO",
    "LOTNO",
    "Lot Number",
];
/// 列の判定で見る行の数（頭から）
const SAMPLE: usize = 3000;

fn key_names() -> &'static HashSet<String> {
    static K: std::sync::OnceLock<HashSet<String>> = std::sync::OnceLock::new();
    K.get_or_init(|| KEY_NAME_CANDIDATES.iter().map(|s| norm_name(s)).collect())
}

/// `^[A-Z][0-9A-Z]{6}$`（ロット番号の形）
fn lot_pattern(v: &str) -> bool {
    let b = v.as_bytes();
    b.len() == 7 && b[0].is_ascii_uppercase() && b[1..].iter().all(|c| c.is_ascii_digit() || c.is_ascii_uppercase())
}

/// 1 行（列の値を 1 つの文字列にまとめ、終わりの位置を持つ）。値は Python の str()（NULL は空）。
#[derive(Debug, Clone)]
pub struct Row {
    text: Box<str>,
    ends: Box<[u32]>,
}

impl Row {
    #[cfg(test)]
    fn new(cells: &[String]) -> Row {
        let mut b = RowBuilder::default();
        for c in cells {
            b.text.push_str(c);
            b.ends.push(b.text.len() as u32);
        }
        b.finish()
    }
    pub fn get(&self, i: usize) -> &str {
        let end = *self.ends.get(i).unwrap_or(&0) as usize;
        let start = if i == 0 { 0 } else { *self.ends.get(i - 1).unwrap_or(&0) as usize };
        self.text.get(start..end).unwrap_or("")
    }
}

/// 行を作る（セルの値を 1 つの文字列へ直に書き足す。セルごとに文字列を作らない）。
#[derive(Default)]
struct RowBuilder {
    text: String,
    ends: Vec<u32>,
}

impl RowBuilder {
    fn push(&mut self, v: rusqlite::types::ValueRef<'_>) {
        use rusqlite::types::ValueRef;
        match v {
            ValueRef::Null => {}
            ValueRef::Text(t) => match std::str::from_utf8(t) {
                Ok(s) => self.text.push_str(s),
                Err(_) => self.text.push_str(&String::from_utf8_lossy(t)),
            },
            ValueRef::Integer(i) => {
                use std::fmt::Write;
                let _ = write!(self.text, "{i}");
            }
            other => self.text.push_str(&sqlite_str(other)),
        }
        self.ends.push(self.text.len() as u32);
    }
    fn finish(&mut self) -> Row {
        let row = Row { text: self.text.as_str().into(), ends: self.ends.as_slice().into() };
        self.text.clear();
        self.ends.clear();
        row
    }
}

/// 1 つの表（列名と行）。
#[derive(Debug, Default)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
}

impl Table {
    /// 列の位置（同じ名前が 2 つあれば前の方: Python の list.index）
    fn col(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == name)
    }
}

/// 突合せの索引（キーの値の組 → 行の位置）
#[derive(Debug, Default)]
struct Index {
    rows: HashMap<String, u32>,
    dup: usize,
}

/// キーの値の組を 1 つの文字にする（区切りの 0x1F は norm_key が除く空白なので、値に現れない）
fn join_key<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    let mut s = String::new();
    for (i, p) in parts.enumerate() {
        if i > 0 {
            s.push('\u{1f}');
        }
        s.push_str(p);
    }
    s
}

fn build_index(t: &Table, pairs: &[(String, String)]) -> Index {
    let mut ix = Index::default();
    if pairs.is_empty() {
        return ix;
    }
    let Some(idx): Option<Vec<usize>> = pairs.iter().map(|(_, m)| t.col(m)).collect() else { return ix };
    ix.rows.reserve(t.rows.len());
    for (n, r) in t.rows.iter().enumerate() {
        let parts: Vec<String> = idx.iter().map(|&i| norm_key(r.get(i))).collect();
        if parts.iter().any(String::is_empty) {
            continue;
        }
        let k = join_key(parts.iter().map(String::as_str));
        if ix.rows.contains_key(&k) {
            ix.dup += 1;
            continue;
        }
        ix.rows.insert(k, n as u32);
    }
    ix
}

/// 読み終えた台帳の控え（問い合わせはこれを使う。差し替えは丸ごと）。
#[derive(Debug, Default)]
pub struct Snapshot {
    /// 表と列（名前の順: sqlite_master を ORDER BY name）。キーを変えた控えと行を分け合う
    pub tables: Arc<Vec<(String, Table)>>,
    pub meta: Vec<(String, String)>,
    pub auto_table: String,
    pub auto_key: String,
    pub table: String,
    pub keys: Vec<(String, String)>,
    pub key_warning: String,
    index: Index,
    loaded: bool,
}

impl Snapshot {
    pub fn table(&self, name: &str) -> Option<&Table> {
        self.tables.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }
    fn current(&self) -> Option<&Table> {
        self.table(&self.table)
    }
    pub fn columns(&self) -> Vec<String> {
        self.current().map(|t| t.columns.clone()).unwrap_or_default()
    }
    pub fn row_count(&self) -> usize {
        self.index.rows.len()
    }
    /// いまの表の列で name に当たる物（そのまま → そろえて）
    pub fn find_column(&self, name: &str) -> String {
        let cols = self.columns();
        if cols.iter().any(|c| c == name) {
            return name.to_string();
        }
        let n = norm_name(name);
        cols.into_iter().find(|c| norm_name(c) == n).unwrap_or_default()
    }
    pub fn key_columns(&self) -> Vec<String> {
        self.keys.iter().map(|(_, m)| m.clone()).collect()
    }
    pub fn key_label(&self) -> String {
        self.keys.iter().map(|(_, m)| label(m)).collect::<Vec<_>>().join(" + ")
    }

    /// スケジュールの行に当たる台帳の行（キーの値が 1 つでも空なら None）
    pub fn row_for(&self, item: &Map<String, Value>) -> Option<&Row> {
        if self.keys.is_empty() {
            return None;
        }
        let parts: Vec<String> = self.keys.iter().map(|(s, _)| norm_key(&item_str(item.get(s)))).collect();
        if parts.iter().any(String::is_empty) {
            return None;
        }
        let n = *self.index.rows.get(&join_key(parts.iter().map(String::as_str)))?;
        self.current()?.rows.get(n as usize)
    }

    /// 台帳の行の値（列名 → 表示の値）。cols が None なら全部の列
    pub fn values(&self, r: &Row, cols: Option<&[String]>) -> Map<String, Value> {
        let mut out = Map::new();
        let Some(t) = self.current() else { return out };
        match cols {
            None => {
                // dict(zip(cols, r)): 同じ名前の列は後ろの値
                for (i, c) in t.columns.iter().enumerate() {
                    out.insert(c.clone(), json!(display_value(r.get(i))));
                }
            }
            Some(cols) => {
                for c in cols {
                    let i = t.columns.iter().rposition(|x| x == c);
                    out.insert(c.clone(), json!(i.map(|i| display_value(r.get(i))).unwrap_or_default()));
                }
            }
        }
        out
    }

    /// 製造行に台帳の列を付ける: m = {列: 値}（cols の列だけ）・mHit = true/false。製造でない行は null。→ 一致した数
    pub fn join(&self, items: &mut [Map<String, Value>], cols: &[String]) -> usize {
        let mut hit = 0;
        for it in items.iter_mut() {
            if it.get("rowKind") != Some(&json!("lot")) || self.keys.is_empty() {
                it.insert("m".into(), Value::Null);
                it.insert("mHit".into(), Value::Null);
                continue;
            }
            match self.row_for(it).map(|r| self.values(r, Some(cols))) {
                None => {
                    it.insert("m".into(), json!({}));
                    it.insert("mHit".into(), json!(false));
                }
                Some(v) => {
                    it.insert("m".into(), Value::Object(v));
                    it.insert("mHit".into(), json!(true));
                    hit += 1;
                }
            }
        }
        hit
    }

    /// 列の判定（列名・ロット番号の形の割合・重複の無さ・スケジュールのロットとの一致）
    fn column_stats(&self, table: &str, col: usize, lots: Option<&HashSet<String>>) -> Value {
        let t = self.table(table).expect("表");
        let name = &t.columns[col];
        let vals: Vec<String> = t.rows.iter().take(SAMPLE).map(|r| norm_key(r.get(col))).filter(|v| !v.is_empty()).collect();
        let n = vals.len();
        let pattern = vals.iter().filter(|v| lot_pattern(v)).count();
        let uniq = vals.iter().collect::<HashSet<_>>().len();
        let name_match = key_names().contains(&norm_name(name));
        let is_cand = name_match || (n > 0 && pattern as f64 / n as f64 >= 0.5);
        let hit = match lots {
            Some(lots) if is_cand && !lots.is_empty() => {
                let full: HashSet<String> = t.rows.iter().map(|r| norm_key(r.get(col))).collect();
                json!(lots.iter().filter(|l| full.contains(*l)).count())
            }
            _ => Value::Null,
        };
        let rate = |x: usize| if n > 0 { crate::pyfmt::py_round(x as f64 / n as f64 * 100.0, 1).unwrap_or(0.0) } else { 0.0 };
        json!({"table": table, "column": name, "label": label(name), "nameMatch": name_match, "isCandidate": is_cand,
               "patternRate": rate(pattern), "uniqueRate": rate(uniq), "scheduleHit": hit, "sample": vals.iter().take(3).collect::<Vec<_>>()})
    }

    /// すべての表・列を調べる（列ごとに糸を分ける）。
    fn all_stats(&self, lots: Option<&HashSet<String>>) -> Vec<Value> {
        let jobs: Vec<(&str, usize)> = self.tables.iter().flat_map(|(t, tb)| (0..tb.columns.len()).map(move |c| (t.as_str(), c))).collect();
        let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 8);
        let chunk = jobs.len().div_ceil(workers).max(1);
        std::thread::scope(|s| {
            let handles: Vec<_> = jobs
                .chunks(chunk)
                .map(|part| s.spawn(move || part.iter().map(|&(t, c)| self.column_stats(t, c, lots)).collect::<Vec<_>>()))
                .collect();
            handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
        })
    }

    fn auto_detect(&self) -> (String, String) {
        let mut best: Option<((bool, f64, f64), String, String)> = None;
        for st in self.all_stats(None) {
            if st["isCandidate"] != true {
                continue;
            }
            let score = (st["nameMatch"] == true, st["patternRate"].as_f64().unwrap_or(0.0), st["uniqueRate"].as_f64().unwrap_or(0.0));
            let better = match &best {
                None => true,
                Some((b, _, _)) => score.partial_cmp(b) == Some(std::cmp::Ordering::Greater),
            };
            if better {
                best = Some((score, st["table"].as_str().unwrap_or("").into(), st["column"].as_str().unwrap_or("").into()));
            }
        }
        best.map(|(_, t, c)| (t, c)).unwrap_or_default()
    }

    /// 設定 → (表, キーの組, 警告)。台帳の列が空なら「自動」（判定したロット番号の列）。
    fn resolve(&self, settings: &Value) -> (String, Vec<(String, String)>, String) {
        let mut table = settings["table"].as_str().unwrap_or("").to_string();
        if self.table(&table).is_none() {
            table = self.auto_table.clone();
        }
        let cols = self.table(&table).map(|t| t.columns.clone()).unwrap_or_default();
        let (mut pairs, mut bad) = (vec![], vec![]);
        for k in settings["keys"].as_array().into_iter().flatten() {
            let s = k["schedule"].as_str().unwrap_or("").to_string();
            let mut m = k["master"].as_str().unwrap_or("").to_string();
            if m.is_empty() {
                m = if table == self.auto_table { self.auto_key.clone() } else { String::new() };
            }
            if !s.is_empty() && cols.contains(&m) {
                pairs.push((s, m));
            } else {
                let given = label(k["master"].as_str().unwrap_or(""));
                bad.push(if given.is_empty() { "(自動)".to_string() } else { given });
            }
        }
        let warn =
            if bad.is_empty() { String::new() } else { format!("台帳に無いキー列を除外しました: {}", bad.join(", ")) };
        if pairs.is_empty() && table == self.auto_table && !self.auto_key.is_empty() {
            pairs = vec![("lotNo".into(), self.auto_key.clone())];
        }
        (table, pairs, warn)
    }

    /// 設定を当てる（表・キー・索引）。
    fn apply(&mut self, settings: &Value) {
        let (table, keys, warn) = self.resolve(settings);
        self.index = self.table(&table).map(|t| build_index(t, &keys)).unwrap_or_default();
        (self.table, self.keys, self.key_warning) = (table, keys, warn);
    }
}

/// スケジュールの値の str()（None は空。norm_key にかける前）
fn item_str(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(v) => crate::pyfmt::json_py_str(v),
    }
}

/// SQLite の名前を引用する。
fn q(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn open_ro(path: &Path) -> rusqlite::Result<rusqlite::Connection> {
    rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX)
}

/// 1 本の糸で読む行の数の目安（これより大きな表は rowid の範囲で分けて、糸ごとに別の接続で読む）
const SPLIT_ROWS: i64 = 20_000;

/// 行を読む（sql の答えを全部）。
fn read_rows(con: &rusqlite::Connection, sql: &str, params: &[i64]) -> rusqlite::Result<Vec<Row>> {
    let mut st = con.prepare(sql)?;
    let n = st.column_count();
    let mut it = st.query(rusqlite::params_from_iter(params.iter()))?;
    let (mut rows, mut b) = (Vec::new(), RowBuilder::default());
    while let Some(r) = it.next()? {
        for i in 0..n {
            b.push(r.get_ref(i)?);
        }
        rows.push(b.finish());
    }
    Ok(rows)
}

/// 表を全部読む。大きな表（rowid のある表）は範囲で分けて並列に読み、rowid の順につなぐ（SELECT * と同じ順）。
fn read_table(con: &rusqlite::Connection, path: &Path, name: &str, columns: Vec<String>) -> rusqlite::Result<Table> {
    let all = format!("SELECT * FROM {}", q(name));
    let n = con.prepare(&all)?.column_count();
    // SELECT * の列（PRAGMA table_info と同じ数のはず。違えば SELECT * の方に合わせる）
    let columns = if columns.len() == n { columns } else { con.prepare(&all)?.column_names().iter().map(|s| s.to_string()).collect() };
    let span: Option<(i64, i64, i64)> = con
        .query_row(&format!("SELECT min(rowid), max(rowid), count(*) FROM {}", q(name)), [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .ok()
        .and_then(|(a, b, c): (Option<i64>, Option<i64>, i64)| Some((a?, b?, c)));
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 8) as i64;
    let rows = match span {
        Some((lo, hi, count)) if count > SPLIT_ROWS && workers > 1 => {
            let parts = workers.min(count / SPLIT_ROWS + 1);
            let step = (hi - lo) / parts + 1;
            let ranges: Vec<(i64, i64)> = (0..parts).map(|i| (lo + i * step, (lo + (i + 1) * step - 1).min(hi))).collect();
            let sql = format!("SELECT * FROM {} WHERE rowid BETWEEN ?1 AND ?2 ORDER BY rowid", q(name));
            let got: Vec<rusqlite::Result<Vec<Row>>> = std::thread::scope(|s| {
                let hs: Vec<_> = ranges
                    .iter()
                    .map(|&(a, b)| {
                        let sql = &sql;
                        s.spawn(move || open_ro(path).and_then(|c| read_rows(&c, sql, &[a, b])))
                    })
                    .collect();
                hs.into_iter().map(|h| h.join().unwrap_or_else(|_| Err(rusqlite::Error::InvalidQuery))).collect()
            });
            let mut rows = Vec::with_capacity(count as usize);
            for part in got {
                rows.extend(part?);
            }
            rows
        }
        _ => read_rows(con, &all, &[])?,
    };
    Ok(Table { columns, rows })
}

/// 手元の写しを読み、設定を当てた控えを作る。
pub fn load_snapshot(path: &Path, settings: &Value) -> rusqlite::Result<Snapshot> {
    let con = open_ro(path)?;
    let names: Vec<String> = con
        .prepare("SELECT name FROM sqlite_master WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' ORDER BY name")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let mut tables = Vec::with_capacity(names.len());
    for t in &names {
        let cols: Vec<String> =
            con.prepare(&format!("PRAGMA table_info({})", q(t)))?.query_map([], |r| r.get(1))?.collect::<Result<_, _>>()?;
        tables.push((t.clone(), read_table(&con, path, t, cols)?));
    }
    let mut s = Snapshot { tables: Arc::new(tables), ..Default::default() };
    // _で始まり、列が「項目・値」の表は台帳の情報（作成日時など）
    for (name, t) in s.tables.iter() {
        if name.starts_with('_') && t.columns.len() >= 2 && t.columns[..2] == ["項目".to_string(), "値".to_string()] {
            for r in &t.rows {
                let (k, v) = (r.get(0), r.get(1));
                let k = if k.is_empty() { "None".to_string() } else { k.to_string() };
                match s.meta.iter_mut().find(|(x, _)| *x == k) {
                    Some(e) => e.1 = v.to_string(),
                    None => s.meta.push((k, v.to_string())),
                }
            }
        }
    }
    (s.auto_table, s.auto_key) = s.auto_detect();
    s.apply(settings);
    s.loaded = true;
    Ok(s)
}

// ---------------------------------------------------------------- 設定（master_settings.json）

pub fn default_settings() -> Value {
    json!({"table": "", "keys": [{"schedule": "lotNo", "master": ""}], "display_columns": []})
}

/// 設定を読む（無い・壊れていれば既定。旧い形 {"key_column": …} も読む）。
pub fn load_settings(path: &Path) -> Value {
    let s = crate::store::read_json(path).filter(Value::is_object).unwrap_or_else(|| json!({}));
    let mut out = default_settings();
    for k in ["table", "keys", "display_columns"] {
        if let Some(v) = s.get(k) {
            out[k] = v.clone();
        }
    }
    if s.get("keys").is_none() && s.get("key_column").is_some_and(crate::pyfmt::truthy) {
        out["keys"] = json!([{"schedule": "lotNo", "master": s["key_column"]}]);
    }
    let keys: Vec<Value> = out["keys"]
        .as_array()
        .map(|a| a.iter().filter(|k| k.is_object() && k.get("schedule").is_some_and(crate::pyfmt::truthy)).cloned().collect())
        .unwrap_or_default();
    out["keys"] = if keys.is_empty() { default_settings()["keys"].clone() } else { json!(keys) };
    out
}

// ---------------------------------------------------------------- 係（裏の糸で写し・読み込み）

/// config.json の master。
#[derive(Debug, Clone)]
pub struct MasterConfig {
    pub enabled: bool,
    pub source: String,
    pub local: PathBuf,
    pub interval: Duration,
    pub settings_file: PathBuf,
}

/// 状態（画面の「状態」「コピー日時」など）。
#[derive(Debug, Clone, Default)]
struct Status {
    state: String,
    message: String,
    copied_at: Option<String>,
    source_mtime: Option<String>,
    loaded_at: Option<String>,
    loading: bool,
}

pub struct LotMaster {
    pub cfg: MasterConfig,
    snap: RwLock<Arc<Snapshot>>,
    status: Mutex<Status>,
    settings: Mutex<Value>,
    /// 写し・読み込みは 1 つずつ（裏の糸と「今すぐ再コピー」が重ならない）
    work: Mutex<()>,
    wake: (Mutex<bool>, Condvar),
    last_check: Mutex<Option<Instant>>,
}

fn now_text() -> String {
    chrono::Local::now().format("%Y/%m/%d %H:%M:%S").to_string()
}

impl LotMaster {
    pub fn new(cfg: MasterConfig) -> Arc<LotMaster> {
        let settings = load_settings(&cfg.settings_file);
        Arc::new(LotMaster {
            cfg,
            snap: RwLock::new(Arc::new(Snapshot::default())),
            status: Mutex::new(Status { state: "未読込".into(), ..Default::default() }),
            settings: Mutex::new(settings),
            work: Mutex::new(()),
            wake: (Mutex::new(false), Condvar::new()),
            last_check: Mutex::new(None),
        })
    }

    /// いまの控え（読み終えた物）。
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snap.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn publish(&self, s: Snapshot) {
        *self.snap.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(s);
    }

    fn info_file(&self) -> PathBuf {
        let mut s = self.cfg.local.as_os_str().to_os_string();
        s.push(".copyinfo.json");
        PathBuf::from(s)
    }

    /// 元ファイルの大きさ・更新時刻が変わっていれば手元へ写す。→ 写したか
    fn copy_if_changed(&self, force: bool) -> Result<bool, String> {
        let src = PathBuf::from(&self.cfg.source);
        let st = std::fs::metadata(&src).map_err(|e| io_message(&e, &self.cfg.source))?;
        let mtime = st.modified().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        let sig = json!([st.len(), mtime]);
        let info = crate::store::read_json(&self.info_file()).unwrap_or_else(|| json!({}));
        let mtime_text = st.modified().ok().map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y/%m/%d %H:%M:%S").to_string());
        self.status.lock().unwrap_or_else(|e| e.into_inner()).source_mtime = mtime_text;
        if !force && info["sig"] == sig && self.cfg.local.exists() {
            self.status.lock().unwrap_or_else(|e| e.into_inner()).copied_at = info["copiedAt"].as_str().map(str::to_string);
            return Ok(false);
        }
        // 書き手が更新の途中（ジャーナルがある）なら写さない
        for suffix in ["-journal", "-wal"] {
            let mut p = src.as_os_str().to_os_string();
            p.push(suffix);
            if Path::new(&p).exists() {
                return Err("元ファイルが更新中のため、前回のコピーを使用しています".into());
            }
        }
        if let Some(d) = self.cfg.local.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("元ファイルにアクセスできません: {e}"))?;
        }
        let mut tmp = self.cfg.local.as_os_str().to_os_string();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        // ただのファイルの写し（共有のファイルに SQLite の錠を取らない）
        std::fs::copy(&src, &tmp).map_err(|e| io_message(&e, &self.cfg.source))?;
        let st2 = std::fs::metadata(&src).map_err(|e| io_message(&e, &self.cfg.source))?;
        let mtime2 = st2.modified().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        if json!([st2.len(), mtime2]) != sig {
            let _ = std::fs::remove_file(&tmp);
            return Err("コピー中に元ファイルが更新されたため、次回再取得します".into());
        }
        // 前の写しと入れ替える前に、写しが壊れていないか確かめる
        let ok: Result<String, String> =
            open_ro(&tmp).and_then(|c| c.query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))).map_err(|e| e.to_string());
        match ok {
            Ok(s) if s == "ok" => {}
            Ok(_) => {
                let _ = std::fs::remove_file(&tmp);
                return Err("コピーしたファイルが壊れているため、前回のコピーを使用しています".into());
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
        }
        crate::store::replace(&tmp, &self.cfg.local).map_err(|e| format!("元ファイルにアクセスできません: {e}"))?;
        let copied_at = now_text();
        let _ = crate::store::write_json(&self.info_file(), &json!({"sig": sig, "copiedAt": copied_at, "source": self.cfg.source}));
        self.status.lock().unwrap_or_else(|e| e.into_inner()).copied_at = Some(copied_at);
        Ok(true)
    }

    fn set_status(&self, state: &str, message: &str) {
        let mut s = self.status.lock().unwrap_or_else(|e| e.into_inner());
        s.state = state.into();
        s.message = message.into();
    }

    /// 手元の写しを読み直して差し替える。
    fn reload_local(&self) {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).loading = true;
        let settings = self.settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
        match load_snapshot(&self.cfg.local, &settings) {
            Ok(s) => {
                self.publish(s);
                self.status.lock().unwrap_or_else(|e| e.into_inner()).loaded_at = Some(now_text());
            }
            Err(e) => self.set_status("エラー", &format!("手元コピーを読めません: {e}")),
        }
        self.status.lock().unwrap_or_else(|e| e.into_inner()).loading = false;
    }

    /// 確かめて、変わっていれば写して読み直す（旧版の refresh。間隔の内なら何もしない）。
    pub fn refresh(&self, force: bool) {
        if !self.cfg.enabled {
            self.set_status("無効", "config.json の master.enabled が false です");
            return;
        }
        let _work = self.work.lock().unwrap_or_else(|e| e.into_inner());
        {
            let mut last = self.last_check.lock().unwrap_or_else(|e| e.into_inner());
            if !force && self.snapshot().loaded && last.is_some_and(|t| t.elapsed() < self.cfg.interval) {
                return;
            }
            *last = Some(Instant::now());
        }
        // 起動の直後: 前に写した物があれば、共有を見に行く前に読む（共有が遅くても、すぐ一覧に台帳が付く）
        let first = !self.snapshot().loaded;
        if first && self.cfg.local.exists() {
            self.reload_local();
        }
        let copied = match self.copy_if_changed(force) {
            Ok(c) => {
                self.set_status("正常", "");
                c
            }
            Err(e) => {
                self.set_status("警告", &e);
                false
            }
        };
        if !self.cfg.local.exists() {
            self.set_status("エラー", &self.status.lock().unwrap_or_else(|e| e.into_inner()).message.clone());
            self.publish(Snapshot::default());
            return;
        }
        if copied || force || !self.snapshot().loaded {
            self.reload_local();
        }
    }

    /// 裏の糸: 起動の直後に読み、間隔ごと（と wake）に確かめる。
    pub fn spawn(self: &Arc<Self>) {
        let me = self.clone();
        std::thread::Builder::new()
            .name("master".into())
            .spawn(move || loop {
                me.refresh(false);
                let (m, cv) = (&me.wake.0, &me.wake.1);
                let g = m.lock().unwrap_or_else(|e| e.into_inner());
                let (mut g, _) = cv.wait_timeout_while(g, me.cfg.interval, |w| !*w).unwrap_or_else(|e| e.into_inner());
                *g = false;
            })
            .expect("台帳の糸");
    }

    /// 読み込み中か（画面が少し待って読み直す）。
    pub fn loading(&self) -> bool {
        let s = self.status.lock().unwrap_or_else(|e| e.into_inner());
        s.loading || (s.state == "未読込" && self.cfg.enabled)
    }

    pub fn settings(&self) -> Value {
        self.settings.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// キーを保存して当てる（メモリの行から索引だけ作り直す）。
    pub fn apply_settings(&self, new: &Value) -> Result<(), String> {
        let _work = self.work.lock().unwrap_or_else(|e| e.into_inner());
        let keys: Vec<Value> = new["keys"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|k| k.get("schedule").is_some_and(crate::pyfmt::truthy))
            .map(|k| json!({"schedule": str_of(&k["schedule"]), "master": str_of(&k["master"])}))
            .collect();
        let old = self.settings();
        let settings = json!({
            "table": new.get("table").cloned().unwrap_or(json!("")),
            "keys": if keys.is_empty() { default_settings()["keys"].clone() } else { json!(keys) },
            "display_columns": new.get("display_columns").cloned().unwrap_or_else(|| old.get("display_columns").cloned().unwrap_or(json!([]))),
        });
        crate::store::write_json(&self.cfg.settings_file, &settings)?;
        *self.settings.lock().unwrap_or_else(|e| e.into_inner()) = settings.clone();
        let cur = self.snapshot();
        if cur.loaded {
            // 行は読み直さず分け合い、索引だけ作り直す
            let mut next = Snapshot {
                tables: cur.tables.clone(),
                meta: cur.meta.clone(),
                auto_table: cur.auto_table.clone(),
                auto_key: cur.auto_key.clone(),
                loaded: true,
                ..Default::default()
            };
            next.apply(&settings);
            self.publish(next);
        }
        Ok(())
    }

    /// キーを保存せずに試す。lots はスケジュールの製造行。
    pub fn test_keys(&self, settings: &Value, lots: &[Map<String, Value>]) -> Value {
        let s = self.snapshot();
        let (table, pairs, warn) = s.resolve(settings);
        if !self.cfg.local.exists() || pairs.is_empty() {
            return json!({"table": table, "pairs": [], "warning": if warn.is_empty() { "キーがありません".to_string() } else { warn },
                          "hit": 0, "total": 0, "masterKeys": 0, "duplicates": 0});
        }
        let ix = s.table(&table).map(|t| build_index(t, &pairs)).unwrap_or_default();
        let mut keys = HashSet::new();
        for it in lots {
            let parts: Vec<String> = pairs.iter().map(|(sf, _)| norm_key(&item_str(it.get(sf)))).collect();
            if parts.iter().all(|p| !p.is_empty()) {
                keys.insert(join_key(parts.iter().map(String::as_str)));
            }
        }
        json!({
            "table": table,
            "pairs": pairs.iter().map(|(sf, m)| json!({"schedule": sf, "master": m, "masterLabel": label(m)})).collect::<Vec<_>>(),
            "warning": warn,
            "hit": keys.iter().filter(|k| ix.rows.contains_key(*k)).count(),
            "total": keys.len(),
            "masterKeys": ix.rows.len(),
            "duplicates": ix.dup,
        })
    }

    /// ロット番号の列の候補（スケジュールのロットとの一致が多い順）。
    pub fn candidates(&self, lots: &HashSet<String>) -> Vec<Value> {
        if !self.cfg.local.exists() {
            return vec![];
        }
        let s = self.snapshot();
        let mut res: Vec<Value> = s.all_stats(Some(lots)).into_iter().filter(|st| st["isCandidate"] == true).collect();
        res.sort_by(|a, b| {
            let key = |x: &Value| {
                (
                    x["scheduleHit"].as_u64().unwrap_or(0),
                    x["nameMatch"] == true,
                    x["patternRate"].as_f64().unwrap_or(0.0),
                    x["uniqueRate"].as_f64().unwrap_or(0.0),
                )
            };
            key(b).partial_cmp(&key(a)).unwrap_or(std::cmp::Ordering::Equal)
        });
        res
    }

    /// 画面の「状態」のまとめ（スケジュールの答えに付ける）。
    pub fn summary(&self) -> Value {
        let s = self.snapshot();
        let st = self.status.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let created = s.meta.iter().find(|(k, _)| k == "作成日時").map(|(_, v)| v.replace('T', " ").replace('-', "/")).unwrap_or_default();
        let message =
            [st.message.as_str(), s.key_warning.as_str()].iter().filter(|x| !x.is_empty()).copied().collect::<Vec<_>>().join(" / ");
        json!({"state": st.state, "message": message, "keyLabel": s.key_label(), "copiedAt": st.copied_at, "created": created,
               "rowCount": s.row_count(), "loading": self.loading()})
    }

    /// 台帳設定の画面への答え。
    pub fn describe(&self) -> Value {
        let s = self.snapshot();
        let st = self.status.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut tables = Map::new();
        for (t, tb) in s.tables.iter() {
            tables.insert(t.clone(), json!(tb.columns.iter().map(|c| json!({"name": c, "label": label(c)})).collect::<Vec<_>>()));
        }
        let mut meta = Map::new();
        for (k, v) in &s.meta {
            meta.insert(k.clone(), json!(v));
        }
        json!({
            "enabled": self.cfg.enabled,
            "source": self.cfg.source,
            "local": self.cfg.local,
            "table": s.table,
            "autoTable": s.auto_table,
            "autoKey": s.auto_key,
            "autoKeyLabel": label(&s.auto_key),
            "keys": s.keys.iter().map(|(sf, m)| json!({"schedule": sf, "master": m, "masterLabel": label(m)})).collect::<Vec<_>>(),
            "keyLabel": s.key_label(),
            "keyWarning": s.key_warning,
            "settings": self.settings(),
            "rowCount": s.row_count(),
            "duplicates": s.index.dup,
            "meta": meta,
            "tables": tables,
            "status": {"state": st.state, "message": st.message, "copiedAt": st.copied_at, "sourceMtime": st.source_mtime,
                       "loadedAt": st.loaded_at, "loading": st.loading},
        })
    }
}

fn str_of(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        v if !crate::pyfmt::truthy(v) => String::new(),
        v => crate::pyfmt::json_py_str(v),
    }
}

/// ファイルの誤りの文言（旧版と同じ言い方）。
fn io_message(e: &std::io::Error, source: &str) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => format!("元ファイルが見つかりません: {source}"),
        std::io::ErrorKind::PermissionDenied => "元ファイルにアクセスできません（権限）".into(),
        _ => format!("元ファイルにアクセスできません: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_keep_cells_apart() {
        let r = Row::new(&["L1".into(), "".into(), "値".into()]);
        assert_eq!((r.get(0), r.get(1), r.get(2), r.get(3)), ("L1", "", "値", ""));
        assert!(lot_pattern("N8230Z0") && !lot_pattern("n8230z0") && !lot_pattern("N8230Z"));
    }
}
