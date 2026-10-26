//! アプリの中身（旧版 app.py の受け持ち）: 設定・スケジュールの取得と覚え・台帳との結合・HOT コイル作業順・画面と API。
//!
//! 画面からの問い合わせ（`fetch("/api/...")`）は、窓（main.rs の自前の仕組み）と開発用の HTTP（examples/serve.rs）の
//! どちらからも `App::handle` に来る。答えは旧版（Flask）と同じ形（JSON の鍵は名前の順に並べる: Flask の jsonify と同じ）。
//!
//! 速さのための作り（旧版との違い）:
//! - 台帳の写し・読み込みは裏の糸（master.rs）。問い合わせは台帳を待たない
//! - HOT コイル作業順は、設備ごとの取得を同時に行う（待ち時間はいちばん遅い設備の分だけ）
//! - 同じ設備を同時に取りに行かない（先に取りに行った答えを待って使う）

use crate::cm3::{self, Cm3Error, Endpoint};
use crate::master::{LotMaster, MasterConfig};
use crate::text::{label, norm_key};
use crate::view::{self, Hot};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

/// 置き場所。
#[derive(Debug, Clone)]
pub struct Paths {
    /// program フォルダ（app/web・app/config.json・app/defaults）
    pub program: PathBuf,
    /// この PC の作業場所（設定・台帳の写し・記録）
    pub local_root: PathBuf,
    /// 現場の設定（config.json）
    pub config: PathBuf,
}

impl Paths {
    pub fn web(&self) -> PathBuf {
        self.program.join("app").join("web")
    }
    pub fn defaults(&self) -> PathBuf {
        self.program.join("app").join("defaults")
    }
}

/// 答え。
#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    pub ctype: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(status: u16, v: &Value) -> Reply {
        Reply { status, ctype: "application/json".into(), headers: vec![], body: dumps(v).into_bytes() }
    }
    pub fn text(status: u16, ctype: &str, body: impl Into<Vec<u8>>) -> Reply {
        Reply { status, ctype: ctype.into(), headers: vec![], body: body.into() }
    }
    fn file(ctype: &str, name: &str, body: Vec<u8>) -> Reply {
        Reply {
            status: 200,
            ctype: ctype.into(),
            headers: vec![("Content-Disposition".into(), format!("attachment; filename={name}"))],
            body,
        }
    }
}

/// 鍵を名前の順に並べ直す（Flask の jsonify・tojson と同じ並び。画面は変換表などをこの順で出す）。
pub fn sort_keys(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            Value::Object(keys.into_iter().map(|k| (k.clone(), sort_keys(&m[k]))).collect())
        }
        Value::Array(a) => Value::Array(a.iter().map(sort_keys).collect()),
        v => v.clone(),
    }
}

/// JSON の文字（鍵は名前の順）。
pub fn dumps(v: &Value) -> String {
    serde_json::to_string(&sort_keys(v)).unwrap_or_default()
}

/// 問い合わせ。
pub struct Request<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub query: &'a str,
    pub body: &'a [u8],
}

/// `a=1&b=%E3%81%82` → [(a, 1), (b, あ)]
pub fn parse_query(q: &str) -> Vec<(String, String)> {
    fn dec(s: &str) -> String {
        let b = s.as_bytes();
        let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            match (b[i], b.get(i + 1).copied().and_then(hex), b.get(i + 2).copied().and_then(hex)) {
                (b'+', _, _) => out.push(b' '),
                (b'%', Some(h), Some(l)) => {
                    out.push(h * 16 + l);
                    i += 2;
                }
                (c, _, _) => out.push(c),
            }
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| p.split_once('=').map(|(k, v)| (dec(k), dec(v))).unwrap_or_else(|| (dec(p), String::new())))
        .collect()
}

fn arg<'a>(q: &'a [(String, String)], k: &str) -> Option<&'a str> {
    q.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str())
}

fn now_text() -> String {
    chrono::Local::now().format("%Y/%m/%d %H:%M:%S").to_string()
}

/// 設備。
#[derive(Debug, Clone)]
pub struct Equipment {
    pub code: String,
    pub term_kbn: String,
    pub raw: Value,
}

/// 覚えたスケジュール（設備ごと）。
type Cached = (Instant, Arc<Value>);

pub struct App {
    pub paths: Paths,
    pub config: Value,
    pub equipments: Vec<Equipment>,
    pub hot: Hot,
    pub master: Arc<LotMaster>,
    endpoint: Endpoint,
    ui: RwLock<Value>,
    view: RwLock<Value>,
    cache: Mutex<Vec<(String, Cached)>>,
    /// 設備ごとの取りに行く錠（同じ設備を同時に取りに行かない）
    fetching: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    /// 画面の版の印（静的ファイルの ?v= に付ける）
    pub build: String,
}

fn settings_dir(p: &Paths, cfg: &Value) -> PathBuf {
    match cfg["settings_dir"].as_str().map(str::trim).filter(|s| !s.is_empty()) {
        Some(d) => {
            let d = PathBuf::from(crate::release::expand_vars(d));
            if d.is_absolute() {
                d
            } else {
                p.local_root.join(d)
            }
        }
        None => p.local_root.join("settings"),
    }
}

/// 設定の初期値（program/app/defaults）を、設定の置き場に無い物だけ写す。
fn seed_settings(p: &Paths, dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    for e in std::fs::read_dir(p.defaults()).into_iter().flatten().flatten() {
        let to = dir.join(e.file_name());
        if !to.exists() && e.path().extension().is_some_and(|x| x == "json") {
            let _ = std::fs::copy(e.path(), &to);
        }
    }
}

impl App {
    /// 設定を読んで作る（台帳の裏の糸はまだ起こさない: start で起こす）。
    pub fn new(paths: Paths, build: String) -> Result<Arc<App>, String> {
        let config = crate::store::read_json(&paths.config).ok_or_else(|| format!("設定（{}）を読めません", paths.config.display()))?;
        let equipments: Vec<Equipment> = config["equipments"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|e| {
                Some(Equipment {
                    code: e["code"].as_str()?.to_string(),
                    term_kbn: e.get("term_kbn").map(crate::pyfmt::json_py_str).unwrap_or_else(|| "4".into()),
                    raw: e.clone(),
                })
            })
            .collect();
        if equipments.is_empty() {
            return Err("config.json の equipments に設備がありません".into());
        }
        let codes: Vec<String> = equipments.iter().map(|e| e.code.clone()).collect();
        let hot = Hot::from_config(&config, &codes);
        let sdir = settings_dir(&paths, &config);
        seed_settings(&paths, &sdir);
        let m = &config["master"];
        let local = m["local_copy"].as_str().unwrap_or("data/LOTNOW.sqlite3");
        let local = if Path::new(local).is_absolute() { PathBuf::from(local) } else { paths.local_root.join(local) };
        let master = LotMaster::new(MasterConfig {
            enabled: m.get("enabled").is_none_or(crate::pyfmt::truthy),
            source: crate::release::expand_vars(m["source_path"].as_str().unwrap_or("")),
            local,
            interval: Duration::from_secs(m["check_interval_sec"].as_u64().unwrap_or(60).max(1)),
            settings_file: sdir.join("master_settings.json"),
        });
        let endpoint = Endpoint {
            base_url: config["cm3_base_url"].as_str().unwrap_or("").to_string(),
            init_path: config["init_path"].as_str().unwrap_or("/Cm3SchWeb/service/schapp/cmschview/init").to_string(),
            client_ip: config.get("client_ip").cloned().unwrap_or(Value::Null),
            timeout: Duration::from_secs_f64(config["timeout_sec"].as_f64().unwrap_or(15.0).max(0.5)),
            use_system_proxy: crate::pyfmt::truthy(&config["use_system_proxy"]),
        };
        let ui = view::merge_ui(&crate::store::read_json(&sdir.join("ui_settings.json")).unwrap_or(json!({})));
        let app = App {
            paths,
            config,
            equipments,
            hot,
            master,
            endpoint,
            ui: RwLock::new(ui),
            view: RwLock::new(Value::Null),
            cache: Mutex::new(vec![]),
            fetching: Mutex::new(HashMap::new()),
            build,
        };
        let v = app.load_view(&sdir);
        *app.view.write().unwrap_or_else(|e| e.into_inner()) = v;
        Ok(Arc::new(app))
    }

    /// 裏の糸を起こす（台帳の写し・読み込み）。
    pub fn start(&self) {
        self.master.spawn();
    }

    pub fn settings_dir(&self) -> PathBuf {
        settings_dir(&self.paths, &self.config)
    }

    fn view_file(&self) -> PathBuf {
        self.settings_dir().join("view_settings.json")
    }

    fn ui_file(&self) -> PathBuf {
        self.settings_dir().join("ui_settings.json")
    }

    pub fn ui(&self) -> Value {
        self.ui.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn view(&self) -> Value {
        self.view.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn default_view(&self) -> Value {
        view::default_view(&self.hot, &self.master.settings()["display_columns"])
    }

    fn load_view(&self, dir: &Path) -> Value {
        let file = dir.join("view_settings.json");
        let mut v = match crate::store::read_json(&file) {
            Some(v) if v.is_object() && v["columns"].is_array() => v,
            _ => return self.default_view(),
        };
        for k in ["maps", "rules"] {
            if v.get(k).is_none() {
                v[k] = if k == "maps" { json!({}) } else { json!([]) };
            }
        }
        if view::migrate_view(&mut v, &self.hot) {
            let _ = crate::store::write_json(&file, &v);
        }
        v
    }

    fn equipment(&self, code: &str) -> Option<&Equipment> {
        self.equipments.iter().find(|e| e.code == code)
    }

    fn default_setsubi(&self) -> String {
        self.config["default_setsubi"].as_str().unwrap_or(&self.equipments[0].code).to_string()
    }

    // ------------------------------------------------------------ スケジュール

    fn cached(&self, setsubi: &str, ttl: Duration) -> Option<Arc<Value>> {
        let c = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        c.iter().find(|(k, _)| k == setsubi).filter(|(_, (t, _))| t.elapsed() < ttl).map(|(_, (_, v))| v.clone())
    }

    /// 設備のスケジュール（cache_sec の間は覚えた物）。
    pub fn schedule_cached(&self, setsubi: &str, force: bool) -> Result<Arc<Value>, Cm3Error> {
        let eq = self.equipment(setsubi).ok_or_else(|| Cm3Error(format!("未登録の設備です: {setsubi}")))?;
        let ttl = Duration::from_secs_f64(self.config["cache_sec"].as_f64().unwrap_or(10.0).max(0.0));
        if !force {
            if let Some(v) = self.cached(setsubi, ttl) {
                return Ok(v);
            }
        }
        let started = Instant::now();
        let lock = self.fetching.lock().unwrap_or_else(|e| e.into_inner()).entry(setsubi.to_string()).or_default().clone();
        let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
        // 待っている間に、ほかの問い合わせが取ってきていれば、それを使う
        {
            let c = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((_, (t, v))) = c.iter().find(|(k, _)| k == setsubi) {
                let fresh_enough = if force { *t >= started } else { t.elapsed() < ttl };
                if fresh_enough {
                    return Ok(v.clone());
                }
            }
        }
        let kws: Vec<String> =
            self.ui()["inspect_keywords"].as_array().into_iter().flatten().filter_map(|k| k.as_str().map(str::to_string)).collect();
        let raw = cm3::fetch_raw(&self.endpoint, setsubi, &eq.term_kbn)?;
        let mut data = cm3::normalize(&raw, setsubi, &kws)?;
        data["fetchedAt"] = json!(now_text());
        let data = Arc::new(data);
        let mut c = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        c.retain(|(k, _)| k != setsubi);
        c.push((setsubi.to_string(), (Instant::now(), data.clone())));
        Ok(data)
    }

    fn items_of(data: &Value) -> Vec<Map<String, Value>> {
        data["items"].as_array().into_iter().flatten().filter_map(|i| i.as_object().cloned()).collect()
    }

    /// スケジュール ＋ 画面が要る台帳の列。
    pub fn schedule_with_master(&self, setsubi: &str, force: bool) -> Result<Value, Cm3Error> {
        let src = self.schedule_cached(setsubi, force)?;
        let mut data = (*src).clone();
        let snap = self.master.snapshot();
        let mut items = Self::items_of(&src);
        let need = view::needed_master_columns(&self.view(), &snap.columns(), &[]);
        let hit = snap.join(&mut items, &need);
        data["items"] = json!(items);
        data["masterHit"] = json!(hit);
        data["masterAll"] = json!(snap.columns());
        data["masterKeyCols"] = json!(snap.key_columns());
        data["master"] = self.master.summary();
        Ok(data)
    }

    /// HOT コイル作業順（L-1・L-2・L-3 の前工程が HOT のロットを、完了予定の順に）。設備ごとの取得は同時に行う。
    pub fn hot_list(&self, force: bool) -> Value {
        let snap = self.master.snapshot();
        let res_col = snap.find_column(&self.hot.result_course_column);
        let rem_col = snap.find_column(&self.hot.remain_course_column);
        let kama = snap.find_column(&self.hot.kama_column);
        let extra: Vec<String> = [&res_col, &rem_col, &kama].into_iter().filter(|c| !c.is_empty()).cloned().collect();
        let need = view::needed_master_columns(&self.view(), &snap.columns(), &extra);
        let prev_value = norm_key(&self.hot.prev_value);
        let got: Vec<Result<Arc<Value>, Cm3Error>> = std::thread::scope(|s| {
            let hs: Vec<_> = self.hot.equipments.iter().map(|eq| s.spawn(move || self.schedule_cached(eq, force))).collect();
            hs.into_iter().map(|h| h.join().unwrap_or_else(|_| Err(Cm3Error("取得の途中で止まりました".into())))).collect()
        });
        let (mut items, mut errors, mut per) = (vec![], vec![], Map::new());
        let (mut lots, mut no_master, mut other_next, mut later_pass) = (0, 0, 0, 0);
        let (mut fetched, mut updated): (Vec<String>, Vec<(String, Value)>) = (vec![], vec![]);
        let tokens = |v: Option<&Value>| -> Vec<String> {
            let s = v.and_then(Value::as_str).unwrap_or("");
            s.split(crate::text::py_space).filter(|t| !t.is_empty()).map(str::to_string).collect()
        };
        for (eq, r) in self.hot.equipments.iter().zip(got) {
            per.insert(eq.clone(), json!(0));
            let data = match r {
                Ok(d) => d,
                Err(e) => {
                    errors.push(format!("{eq}: {e}"));
                    continue;
                }
            };
            fetched.push(data["fetchedAt"].as_str().unwrap_or("").to_string());
            updated.push((eq.clone(), data.get("updateTime").cloned().unwrap_or(Value::Null)));
            let mut its = Self::items_of(&data);
            snap.join(&mut its, &need);
            for mut it in its {
                if it["rowKind"] != "lot" {
                    continue;
                }
                lots += 1;
                if it["mHit"] != true || res_col.is_empty() {
                    no_master += 1;
                    continue;
                }
                let m = it["m"].clone();
                let hist = tokens(m.get(&res_col));
                let prev = hist.last().cloned().unwrap_or_default();
                if norm_key(&prev) != prev_value {
                    continue;
                }
                if !rem_col.is_empty() {
                    let rem = tokens(m.get(&rem_col));
                    if rem.first().is_some_and(|r| norm_key(r) != norm_key(eq)) {
                        other_next += 1;
                        continue;
                    }
                }
                let pass = it["passNo"].as_i64().filter(|p| *p != 0).unwrap_or(1);
                if pass != 1 {
                    later_pass += 1;
                    continue;
                }
                it.insert("prevProcess".into(), json!(prev));
                it.insert("kama".into(), if kama.is_empty() { json!("") } else { m.get(&kama).cloned().unwrap_or(json!("")) });
                items.push(it);
                per[eq] = json!(per[eq].as_i64().unwrap_or(0) + 1);
            }
        }
        let order = |s: &str| self.hot.equipments.iter().position(|e| e == s).unwrap_or(99);
        items.sort_by(|a, b| {
            let key = |x: &Map<String, Value>| {
                (
                    x["kanryoYoteiIso"].as_str().unwrap_or("9999").to_string(),
                    order(x["setsubi"].as_str().unwrap_or("")),
                    x["seq"].as_i64().unwrap_or(0),
                )
            };
            key(a).cmp(&key(b))
        });
        for (i, it) in items.iter_mut().enumerate() {
            it.insert("hotSeq".into(), json!(i + 1));
            it.insert("rowStyle".into(), json!(if (i + 1) % 2 == 0 { "cmschview-data-palegreen" } else { "cmschview-data-white" }));
        }
        let update_time = updated
            .iter()
            .map(|(k, v)| {
                let t = v.as_str().map(str::to_string).filter(|s| !s.is_empty()).unwrap_or_else(|| "-".into());
                format!("{k} {}", t.chars().skip(5).take(11).collect::<String>())
            })
            .collect::<Vec<_>>()
            .join(" / ");
        json!({
            "setsubi": "__HOT__",
            "count": items.len(),
            "items": items,
            "inspectCount": 0,
            "stopCount": 0,
            "fetchedAt": fetched.iter().max().cloned().unwrap_or_else(now_text),
            "updateTime": update_time,
            "masterAll": snap.columns(),
            "masterKeyCols": snap.key_columns(),
            "master": self.master.summary(),
            "hot": {
                "title": self.hot.title, "equipments": self.hot.equipments, "prevValue": self.hot.prev_value,
                "resultColumn": res_col, "remainColumn": rem_col, "kamaColumn": kama, "kamaConfigured": self.hot.kama_column,
                "perEquipment": per, "counts": {"lots": lots, "noMaster": no_master, "otherNext": other_next, "laterPass": later_pass},
                "errors": errors,
            },
        })
    }

    /// 取得済みのスケジュールの製造行（キーの試し・候補の一致に使う）。
    fn cached_lot_items(&self) -> Vec<Map<String, Value>> {
        let c = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        c.iter().flat_map(|(_, (_, v))| Self::items_of(v)).filter(|i| i["rowKind"] == "lot").collect()
    }

    fn setsubi_of(&self, q: &[(String, String)]) -> Result<String, String> {
        let s = arg(q, "setsubi").filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(|| self.default_setsubi());
        if self.equipment(&s).is_none() {
            return Err(format!("未登録の設備です: {s}"));
        }
        Ok(s)
    }

    // ------------------------------------------------------------ 画面

    /// 画面に埋める初めの値（旧版のテンプレートに渡していた物）。
    pub fn boot(&self) -> Value {
        json!({
            "refreshSec": self.config["auto_refresh_sec"].as_i64().unwrap_or(60),
            "defaultSetsubi": self.default_setsubi(),
            "lotUrlTemplate": self.config["lot_url_template"].as_str().unwrap_or(""),
            "cm3ViewUrlTemplate": self.config["cm3_view_url_template"].as_str().unwrap_or(""),
            "equipments": self.equipments.iter().map(|e| e.raw.clone()).collect::<Vec<_>>(),
            "ui": self.ui(),
            "uiDefault": view::default_ui(),
            "hot": {"title": self.hot.title, "equipments": self.hot.equipments},
            "scheduleFields": view::schedule_fields().iter().map(|(k, v)| json!({"key": k, "label": v})).collect::<Vec<_>>(),
            "scheduleColumns": view::SCHEDULE_COLUMNS.iter().map(|(k, v)| json!({"key": k, "label": v})).collect::<Vec<_>>(),
            "view": self.view(),
            "build": self.build,
        })
    }

    /// 画面（index.html の `{{APP}}` に初めの値、`{{V}}` に版の印）。
    fn index(&self) -> Reply {
        let tpl = match std::fs::read_to_string(self.paths.web().join("index.html")) {
            Ok(t) => t,
            Err(e) => return Reply::text(500, "text/plain; charset=utf-8", format!("画面（index.html）を読めません: {e}")),
        };
        // <script> の中に埋めるので、< > & ' は \u で書く（Flask の tojson と同じ）
        let app = dumps(&self.boot()).replace('<', "\\u003c").replace('>', "\\u003e").replace('&', "\\u0026").replace('\'', "\\u0027");
        let html = tpl.replace("{{APP}}", &app).replace("{{V}}", &self.build);
        let mut r = Reply::text(200, "text/html; charset=utf-8", html);
        r.headers.push(("Cache-Control".into(), "no-cache".into()));
        r
    }

    /// 静的ファイル（app/web の下だけ。外へは出ない）。
    fn static_file(&self, rel: &str, versioned: bool) -> Reply {
        let rel = rel.trim_start_matches('/');
        let bad = rel.is_empty() || rel.split(['/', '\\']).any(|p| p.is_empty() || p == ".." || p == "." || p.contains(':'));
        if bad {
            return Reply::text(404, "text/plain; charset=utf-8", "not found");
        }
        let path = self.paths.web().join(rel);
        match std::fs::read(&path) {
            Ok(body) => {
                let ctype = match path.extension().and_then(|x| x.to_str()).unwrap_or("") {
                    "js" | "mjs" => "text/javascript; charset=utf-8",
                    "css" => "text/css; charset=utf-8",
                    "html" => "text/html; charset=utf-8",
                    "json" => "application/json",
                    "png" => "image/png",
                    "svg" => "image/svg+xml",
                    "ico" => "image/x-icon",
                    "woff2" => "font/woff2",
                    _ => "application/octet-stream",
                };
                let mut r = Reply::text(200, ctype, body);
                // 版の印つき（?v=）は長く使い回す。印の無い物は毎回確かめる
                r.headers.push(("Cache-Control".into(), if versioned { "public, max-age=31536000, immutable" } else { "no-cache" }.into()));
                r
            }
            Err(_) => Reply::text(404, "text/plain; charset=utf-8", "not found"),
        }
    }

    // ------------------------------------------------------------ 受け口

    /// 問い合わせに答える（無い道は 404）。
    pub fn handle(&self, req: &Request) -> Reply {
        let q = parse_query(req.query);
        let body_json = || -> Option<Value> { serde_json::from_slice(req.body).ok() };
        let ok = |mut v: Value| {
            v["success"] = json!(true);
            Reply::json(200, &v)
        };
        let fail = |status: u16, msg: String| Reply::json(status, &json!({"success": false, "message": msg}));
        match (req.method, req.path) {
            ("GET" | "HEAD", "/") => self.index(),
            ("GET", "/favicon.ico") => Reply::text(204, "text/plain", ""),
            ("GET" | "HEAD", p) if p.starts_with("/static/") => self.static_file(&p["/static/".len()..], arg(&q, "v").is_some()),
            ("GET", "/api/boot") => Reply::json(200, &self.boot()),
            ("GET", "/api/cm3/schedule") => {
                let setsubi = match self.setsubi_of(&q) {
                    Ok(s) => s,
                    Err(e) => return fail(400, e),
                };
                match self.schedule_with_master(&setsubi, arg(&q, "force") == Some("1")) {
                    Ok(d) => ok(d),
                    Err(e) => fail(502, e.0),
                }
            }
            ("GET", "/api/hot") => {
                let d = self.hot_list(arg(&q, "force") == Some("1"));
                let errors = d["hot"]["errors"].as_array().cloned().unwrap_or_default();
                if d["items"].as_array().is_some_and(|a| a.is_empty()) && errors.len() == self.hot.equipments.len() {
                    let msg = errors.iter().filter_map(|e| e.as_str()).collect::<Vec<_>>().join(" / ");
                    return fail(502, msg);
                }
                ok(d)
            }
            ("GET", "/api/cm3/schedule.csv") => self.raw_csv(&q),
            ("POST", "/api/export/xlsx") => {
                let body = body_json().filter(Value::is_object).unwrap_or_else(|| json!({}));
                match crate::xlsx::build(&body) {
                    Ok(data) => {
                        let key = body
                            .get("fileKey")
                            .filter(|v| crate::pyfmt::truthy(v))
                            .or_else(|| body.get("sheet").filter(|v| crate::pyfmt::truthy(v)));
                        let key = key.map(crate::pyfmt::json_py_str).unwrap_or_else(|| "cm3".into());
                        let key: String =
                            key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
                        let name = format!("cm3_{key}_{}.xlsx", chrono::Local::now().format("%Y%m%d_%H%M"));
                        Reply::file("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", &name, data)
                    }
                    Err(e) => fail(400, format!("Excelを作れませんでした: {e}")),
                }
            }
            ("GET", "/api/cm3/raw") => {
                let r = self.setsubi_of(&q).map_err(Cm3Error).and_then(|s| {
                    let eq = self.equipment(&s).expect("設備");
                    cm3::fetch_raw(&self.endpoint, &s, &eq.term_kbn)
                });
                match r {
                    Ok(raw) => Reply::text(200, "application/json; charset=utf-8", serde_json::to_string_pretty(&raw).unwrap_or_default()),
                    Err(e) => Reply::text(502, "text/plain; charset=utf-8", e.0),
                }
            }
            ("GET" | "POST", "/api/ui/settings") => {
                if req.method == "POST" {
                    let body = body_json().filter(crate::pyfmt::truthy).unwrap_or_else(|| json!({}));
                    let ui = view::merge_ui(&body);
                    if let Err(e) = crate::store::write_json(&self.ui_file(), &ui) {
                        return fail(500, e);
                    }
                    *self.ui.write().unwrap_or_else(|e| e.into_inner()) = ui;
                    // 行の区分は点検キーワードで変わるので、覚えたスケジュールを捨てる
                    self.cache.lock().unwrap_or_else(|e| e.into_inner()).clear();
                }
                ok(json!({"settings": self.ui(), "default": view::default_ui()}))
            }
            ("GET" | "POST", "/api/view/settings") => {
                if req.method == "POST" {
                    let mut body = body_json().unwrap_or(Value::Null);
                    let err = view::valid_view(&body);
                    if !err.is_empty() {
                        let msg = if err == "列IDが重複しています" {
                            format!("{err}: {}", view::duplicated_id(&body))
                        } else {
                            err.to_string()
                        };
                        return fail(400, msg);
                    }
                    if body.get("maps").is_none() {
                        body["maps"] = json!({});
                    }
                    if body.get("rules").is_none() {
                        body["rules"] = json!([]);
                    }
                    body["version"] = json!(view::VIEW_VERSION);
                    if let Err(e) = crate::store::write_json(&self.view_file(), &body) {
                        return fail(500, e);
                    }
                    *self.view.write().unwrap_or_else(|e| e.into_inner()) = body;
                }
                ok(json!({"settings": self.view()}))
            }
            ("GET", "/api/view/default") => ok(json!({"settings": self.default_view()})),
            ("GET", "/api/view/sample") => self.view_sample(&q),
            ("GET", "/api/master") => {
                let lots: HashSet<String> =
                    self.cached_lot_items().iter().filter_map(|it| it["lotNo"].as_str().filter(|s| !s.is_empty()).map(norm_key)).collect();
                let mut d = self.master.describe();
                d["scheduleLots"] = json!(lots.len());
                d["candidates"] = json!(self.master.candidates(&lots));
                ok(d)
            }
            ("POST", "/api/master/reload") => {
                self.master.refresh(true);
                ok(self.master.describe())
            }
            ("POST", "/api/master/test") => {
                let body = body_json().filter(crate::pyfmt::truthy).unwrap_or_else(|| json!({}));
                if let Err(e) = self.valid_key_body(&body) {
                    return fail(400, e);
                }
                ok(self.master.test_keys(&body, &self.cached_lot_items()))
            }
            ("POST", "/api/master/settings") => {
                let body = body_json().filter(crate::pyfmt::truthy).unwrap_or_else(|| json!({}));
                if let Err(e) = self.valid_key_body(&body) {
                    return fail(400, e);
                }
                if let Err(e) = self.master.apply_settings(&body) {
                    return fail(500, e);
                }
                ok(self.master.describe())
            }
            _ => Reply::json(404, &json!({"success": false, "message": format!("見つかりません: {} {}", req.method, req.path)})),
        }
    }

    /// 元の値の CSV（読み替え・書式の前。ほかの道具向け）。
    fn raw_csv(&self, q: &[(String, String)]) -> Reply {
        let setsubi = match self.setsubi_of(q) {
            Ok(s) => s,
            Err(e) => return Reply::text(502, "text/plain; charset=utf-8", e),
        };
        let data = match self.schedule_with_master(&setsubi, false) {
            Ok(d) => d,
            Err(e) => return Reply::text(502, "text/plain; charset=utf-8", e.0),
        };
        let mcols = view::needed_master_columns(&self.view(), &self.master.snapshot().columns(), &[]);
        let cell = |v: Option<&Value>| match v {
            None | Some(Value::Null) => String::new(),
            Some(v) => crate::pyfmt::json_py_str(v),
        };
        let mut rows: Vec<Vec<String>> = vec![];
        let mut head = vec!["区分".to_string()];
        head.extend(view::SCHEDULE_COLUMNS.iter().map(|(_, l)| l.to_string()));
        head.extend(["総パス", "設備停止内容", "台帳一致"].map(String::from));
        head.extend(mcols.iter().map(|c| format!("[台帳]{}", label(c))));
        rows.push(head);
        for it in data["items"].as_array().into_iter().flatten() {
            let kind = match it["rowKind"].as_str() {
                Some("lot") => "製造",
                Some("inspect") => "点検",
                Some("stop") => "設備停止",
                _ => "",
            };
            let mut r = vec![kind.to_string()];
            r.extend(view::SCHEDULE_COLUMNS.iter().map(|(k, _)| cell(it.get(*k))));
            r.push(cell(it.get("passTotal")));
            r.push(it["stopText"].as_str().unwrap_or("").to_string());
            r.push(match &it["mHit"] {
                Value::Bool(true) => "○".into(),
                Value::Bool(false) => "×".into(),
                _ => String::new(),
            });
            r.extend(mcols.iter().map(|c| it["m"].get(c).and_then(Value::as_str).unwrap_or("").to_string()));
            rows.push(r);
        }
        let mut out = String::from("\u{feff}");
        for r in rows {
            out += &r.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(",");
            out += "\r\n";
        }
        let name = format!("cm3_{setsubi}_{}_raw.csv", chrono::Local::now().format("%Y%m%d_%H%M"));
        Reply::file("text/csv", &name, out.into_bytes())
    }

    /// 列の設定の画面のための実データ（製造・点検行に台帳の全部の列。300 行まで）。
    fn view_sample(&self, q: &[(String, String)]) -> Reply {
        let fail = |e: String| Reply::json(502, &json!({"success": false, "message": e}));
        let setsubi = match self.setsubi_of(q) {
            Ok(s) => s,
            Err(e) => return fail(e),
        };
        let data = match self.schedule_cached(&setsubi, false) {
            Ok(d) => d,
            Err(e) => return fail(e.0),
        };
        let snap = self.master.snapshot();
        let mut items = vec![];
        for mut it in Self::items_of(&data) {
            let kind = it["rowKind"].as_str().unwrap_or("").to_string();
            if kind == "stop" {
                continue;
            }
            if kind == "lot" {
                let r = snap.row_for(&it).map(|r| snap.values(r, None));
                it.insert("mHit".into(), json!(r.is_some()));
                it.insert("m".into(), r.map(Value::Object).unwrap_or_else(|| json!({})));
            } else {
                it.insert("m".into(), Value::Null);
                it.insert("mHit".into(), Value::Null);
            }
            items.push(it);
            if items.len() >= 300 {
                break;
            }
        }
        Reply::json(200, &json!({"success": true, "items": items, "masterAll": snap.columns(), "masterKeyCols": snap.key_columns()}))
    }

    /// キーの組の確かめ（誤りの文言）。
    fn valid_key_body(&self, body: &Value) -> Result<(), String> {
        let fields: Vec<&str> = view::schedule_fields().iter().map(|(k, _)| *k).collect();
        let keys: Vec<Value> = body["keys"].as_array().cloned().filter(|k| !k.is_empty()).ok_or("キーを1つ以上指定してください")?;
        for k in &keys {
            let s = k.get("schedule");
            if !s.and_then(Value::as_str).is_some_and(|s| fields.contains(&s)) {
                return Err(format!(
                    "スケジュール側の項目が不正です: {}",
                    s.map(crate::pyfmt::json_py_str).unwrap_or_else(|| "None".into())
                ));
            }
        }
        let snap = self.master.snapshot();
        let t = body["table"].as_str().unwrap_or("");
        if !t.is_empty() && snap.table(t).is_none() {
            return Err(format!("テーブルが見つかりません: {t}"));
        }
        let cols = snap.table(if t.is_empty() { &snap.auto_table } else { t }).map(|x| x.columns.clone()).unwrap_or_default();
        for (i, k) in keys.iter().enumerate() {
            let m = k["master"].as_str().unwrap_or("");
            if (!m.is_empty() || i > 0) && !cols.iter().any(|c| c == m) {
                let l = label(m);
                return Err(format!("台帳の列が見つかりません: {}", if l.is_empty() { "(未選択)".to_string() } else { l }));
            }
        }
        let mut seen = HashSet::new();
        for k in &keys {
            if !seen.insert((k["schedule"].as_str().unwrap_or("").to_string(), k["master"].as_str().unwrap_or("").to_string())) {
                return Err("同じキーが重複しています".into());
            }
        }
        Ok(())
    }
}

/// CSV の 1 つの値（, " 改行を含めば "" で囲む: Python の csv と同じ）。
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_are_decoded() {
        assert_eq!(parse_query("setsubi=L-2&force=1"), vec![("setsubi".into(), "L-2".into()), ("force".into(), "1".into())]);
        assert_eq!(parse_query("a=%E3%81%82+b&c"), vec![("a".into(), "あ b".into()), ("c".into(), String::new())]);
        assert_eq!(parse_query("a=%zz%"), vec![("a".into(), "%zz%".into())]);
    }

    #[test]
    fn keys_are_sorted_like_flask() {
        assert_eq!(dumps(&json!({"b": 1, "a": {"d": 2, "c": 3}})), r#"{"a":{"c":3,"d":2},"b":1}"#);
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("x\"y"), "\"x\"\"y\"");
    }
}
