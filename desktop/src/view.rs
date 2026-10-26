//! 列の設定（view_settings.json）と配色（ui_settings.json）。旧版 app.py の同じ名前の処理と同じ答え。
//! 仕様は program/docs/CM3_API_spec.md の付録 B。

use crate::text::norm_name;
use serde_json::{json, Map, Value};
use std::collections::HashSet;

/// スケジュールの項目（鍵, 見出し）。列・突合せのキー・式の参照に使う。
pub const SCHEDULE_COLUMNS: [(&str, &str); 16] = [
    ("seq", "No"),
    ("kanryoYotei", "完了予定"),
    ("lotNo", "ロットNo"),
    ("passNo", "パス"),
    ("zaishitsu", "材質"),
    ("inItaAtsu", "入側板厚"),
    ("outItaAtsu", "出側板厚"),
    ("reductionRate", "圧下率"),
    ("reienHaba", "冷延幅"),
    ("seihinItaAtsu", "製品板厚"),
    ("seihinItaHaba", "製品幅"),
    ("keijo", "形状"),
    ("yotoMei", "用途名"),
    ("jiKotei", "次工程"),
    ("setsubi", "設備"),
    ("startYotei", "開始予定(推定)"),
];
const HIDDEN_BY_DEFAULT: [&str; 2] = ["setsubi", "startYotei"];
const NOT_KEYS: [&str; 6] = ["seq", "kanryoYotei", "passNo", "reductionRate", "setsubi", "startYotei"];

/// 突合せのキーに使えるスケジュールの項目。
pub fn schedule_fields() -> Vec<(&'static str, &'static str)> {
    SCHEDULE_COLUMNS.iter().copied().filter(|(k, _)| !NOT_KEYS.contains(k)).collect()
}

pub const VIEW_VERSION: i64 = 2;
const DEFAULT_MASTER_COLUMNS: [&str; 9] =
    ["製造調質", "ｵｰﾀﾞｰ板厚", "ｵｰﾀﾞｰ板幅", "BOX実績_良品重量", "残仕掛設備ｺｰｽ", "特急区分", "保留_区分", "工場納期", "約束納期"];

/// HOT コイル作業順の設定（config.json の hot で上書き）。
#[derive(Debug, Clone)]
pub struct Hot {
    pub title: String,
    pub equipments: Vec<String>,
    pub prev_value: String,
    pub result_course_column: String,
    pub remain_course_column: String,
    pub kama_column: String,
}

impl Hot {
    pub fn from_config(cfg: &Value, known: &[String]) -> Hot {
        let h = &cfg["hot"];
        let s = |k: &str, d: &str| h.get(k).and_then(Value::as_str).unwrap_or(d).to_string();
        let eqs: Vec<String> = match h.get("equipments").and_then(Value::as_array) {
            Some(a) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
            None => vec!["L-1".into(), "L-2".into(), "L-3".into()],
        };
        Hot {
            title: s("title", "HOTコイル作業順"),
            equipments: eqs.into_iter().filter(|e| known.contains(e)).collect(),
            prev_value: s("prev_value", "HOT"),
            result_course_column: s("result_course_column", "実績_設備ｺｰｽ"),
            remain_course_column: s("remain_course_column", "残仕掛設備ｺｰｽ"),
            kama_column: s("kama_column", "設計_PIT炉番炉順"),
        }
    }
}

/// 列 1 つ（既定の形。kw で上書き・足す）。
fn col(id: &str, source: &str, field: &str, kw: &[(&str, Value)]) -> Value {
    let mut c = Map::new();
    for (k, v) in [
        ("id", json!(id)),
        ("source", json!(source)),
        ("field", json!(field)),
        ("label", json!("")),
        ("visible", json!(true)),
        ("width", Value::Null),
        ("align", json!("auto")),
        ("bold", json!(false)),
        ("format", json!({"type": "none"})),
        ("map", json!("")),
    ] {
        c.insert(k.into(), v);
    }
    for (k, v) in kw {
        c.insert((*k).into(), v.clone());
    }
    Value::Object(c)
}

/// 前工程 = 実績_設備ｺｰｽ（空白区切り）の最後。
fn prev_process_col(hot: &Hot) -> Value {
    col(
        "c:prev",
        "calc",
        "",
        &[
            ("label", json!("前工程")),
            ("align", json!("center")),
            ("expr", json!(format!("split([{}],\" \",-1)", hot.result_course_column))),
        ],
    )
}

/// 設計_PIT炉番炉順 を「釜番」として。
fn kama_col(hot: &Hot) -> Value {
    col(&format!("m:{}", hot.kama_column), "master", &hot.kama_column, &[("label", json!("釜番")), ("align", json!("center"))])
}

/// 既定の列の設定。display_columns は台帳設定（master_settings.json）に前の版が覚えた台帳の列。
pub fn default_view(hot: &Hot, display_columns: &Value) -> Value {
    let mut cols = vec![];
    for (k, _) in SCHEDULE_COLUMNS {
        let mut kw: Vec<(&str, Value)> = vec![];
        if k == "reductionRate" {
            kw.push(("format", json!({"type": "number", "decimals": 1, "suffix": "%"})));
        }
        if HIDDEN_BY_DEFAULT.contains(&k) {
            kw.push(("visible", json!(false)));
        }
        cols.push(col(&format!("s:{k}"), "schedule", k, &kw));
        if k == "passNo" {
            cols.push(prev_process_col(hot));
            cols.push(kama_col(hot));
        }
    }
    let old: Vec<Value> = match display_columns.as_array() {
        Some(a) if !a.is_empty() => a.clone(),
        _ => DEFAULT_MASTER_COLUMNS.iter().map(|s| json!(s)).collect(),
    };
    for m in old {
        let name = crate::pyfmt::json_py_str(&m);
        if name != hot.kama_column {
            cols.push(col(&format!("m:{name}"), "master", &name, &[]));
        }
    }
    // 式の列の例（隠しておく）
    cols.push(col(
        "c:1",
        "calc",
        "",
        &[
            ("label", json!("次の設備")),
            ("visible", json!(false)),
            ("expr", json!("if(split([残仕掛設備ｺｰｽ],\" \",1)=\"\",\"完了\",split([残仕掛設備ｺｰｽ],\" \",1))")),
        ],
    ));
    json!({"version": VIEW_VERSION, "columns": cols, "maps": {}, "rules": []})
}

/// 前の版で保存した設定に、新しい既定の列（前工程・釜番）を 1 度だけ足す。→ 変えたか
pub fn migrate_view(v: &mut Value, hot: &Hot) -> bool {
    let version = v.get("version").and_then(|x| x.as_i64().or_else(|| x.as_str().and_then(|s| s.parse().ok()))).unwrap_or(1);
    if version >= VIEW_VERSION {
        return false;
    }
    let Some(cols) = v.get_mut("columns").and_then(Value::as_array_mut) else { return false };
    let pos = cols.iter().position(|c| c["id"] == "s:passNo").map(|i| i + 1).unwrap_or(cols.len());
    let has_prev =
        cols.iter().any(|c| c["id"] == "c:prev" || (c["source"] == "calc" && c["label"].as_str().unwrap_or("").trim() == "前工程"));
    let kid = format!("m:{}", hot.kama_column);
    let mut add = vec![];
    if !has_prev {
        add.push(prev_process_col(hot));
    }
    match cols.iter_mut().find(|c| c["id"] == json!(kid)) {
        None => add.push(kama_col(hot)),
        Some(k) => {
            if k["label"].as_str().unwrap_or("").trim().is_empty() {
                k["label"] = json!("釜番");
            }
            k["visible"] = json!(true);
        }
    }
    cols.splice(pos..pos, add);
    v["version"] = json!(VIEW_VERSION);
    true
}

/// 保存してよい形か（誤りの文言。良ければ空）。
pub fn valid_view(v: &Value) -> &'static str {
    let Some(o) = v.as_object() else { return "形式が不正です" };
    let Some(cols) = o.get("columns").and_then(Value::as_array).filter(|c| !c.is_empty()) else { return "列がありません" };
    let mut ids = HashSet::new();
    for c in cols {
        let ok = c.is_object()
            && matches!(c["source"].as_str(), Some("schedule" | "master" | "calc"))
            && c.get("id").is_some_and(crate::pyfmt::truthy);
        if !ok {
            return "列の形式が不正です";
        }
        let id = crate::pyfmt::json_py_str(&c["id"]);
        if !ids.insert(id) {
            return "列IDが重複しています";
        }
    }
    if o.get("maps").is_some_and(|m| !m.is_object()) || o.get("rules").is_some_and(|r| !r.is_array()) {
        return "変換表またはルールの形式が不正です";
    }
    if serde_json::to_string(v).map(|s| s.chars().count()).unwrap_or(0) > 2_000_000 {
        return "設定が大きすぎます";
    }
    ""
}

/// 重複した列 ID（誤りの文言に添える）。
pub fn duplicated_id(v: &Value) -> String {
    let mut ids = HashSet::new();
    for c in v["columns"].as_array().into_iter().flatten() {
        let id = crate::pyfmt::json_py_str(&c["id"]);
        if !ids.insert(id.clone()) {
            return id;
        }
    }
    String::new()
}

/// `[名前]` の名前を全部（`\[([^\]]+)\]`）。
fn refs(text: &str) -> Vec<&str> {
    let mut out = vec![];
    let mut rest = text;
    while let Some(i) = rest.find('[') {
        let tail = &rest[i + 1..];
        match tail.find(']') {
            Some(j) if j > 0 => {
                out.push(&tail[..j]);
                rest = &tail[j + 1..];
            }
            Some(_) => rest = &tail[1..],
            None => break,
        }
    }
    out
}

/// 画面が要る台帳の列: 表示する台帳の列 + 式・ルールが参照する列（+ extra）。並びは台帳の列の順。
pub fn needed_master_columns(view: &Value, cols: &[String], extra: &[String]) -> Vec<String> {
    if cols.is_empty() {
        return vec![];
    }
    let in_cols = |c: &str| cols.iter().any(|x| x == c);
    let mut by_norm = std::collections::HashMap::new();
    for c in cols {
        by_norm.insert(norm_name(c), c.clone());
    }
    let mut need: HashSet<String> = extra.iter().filter(|c| in_cols(c)).cloned().collect();
    let mut texts: Vec<String> = vec![];
    for c in view["columns"].as_array().into_iter().flatten() {
        let field = c["field"].as_str().unwrap_or("");
        if c["source"] == "master" && in_cols(field) && c.get("visible").is_none_or(crate::pyfmt::truthy) {
            need.insert(field.to_string());
        }
        if c["source"] == "calc" {
            texts.push(c["expr"].as_str().unwrap_or("").to_string());
        }
    }
    for r in view["rules"].as_array().into_iter().flatten() {
        texts.push(r["expr"].as_str().unwrap_or("").to_string());
        for k in ["col", "cellCol"] {
            let cid = r[k].as_str().unwrap_or("");
            if let Some(name) = cid.strip_prefix("m:").filter(|n| in_cols(n)) {
                need.insert(name.to_string());
            }
        }
    }
    for t in &texts {
        for name in refs(t) {
            let hit = if in_cols(name) { Some(name.to_string()) } else { by_norm.get(&norm_name(name)).cloned() };
            if let Some(h) = hit {
                need.insert(h);
            }
        }
    }
    cols.iter().filter(|c| need.contains(*c)).cloned().collect()
}

// ---------------------------------------------------------------- 配色・点検キーワード

pub fn default_ui() -> Value {
    json!({
        "inspect_keywords": ["点検板"],
        "colors": {
            "normal_bg": "#ffffff", "stripe_bg": "#eef8ee", "self_next_text": "#8a5300", "inspect_bg": "#fff4c2",
            "inspect_text": "#6b4e00", "inspect_tag": "#c99400", "stop_bg": "#fdecec", "stop_text": "#8a1c13",
            "stop_tag": "#b42318", "date_line": "#9aa5b1", "accent": "#0f6cbd",
        },
    })
}

fn is_hex(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// 送られた・保存された配色を既定に重ねる（点検キーワードは前後の空白を除き空を捨てる・色は #RRGGBB だけ）。
pub fn merge_ui(saved: &Value) -> Value {
    let mut ui = default_ui();
    if let Some(kws) = saved.get("inspect_keywords").and_then(Value::as_array) {
        ui["inspect_keywords"] = json!(kws
            .iter()
            .map(|k| crate::text::py_strip(&crate::pyfmt::json_py_str(k)).to_string())
            .filter(|k| !k.is_empty())
            .collect::<Vec<_>>());
    }
    if let Some(colors) = saved.get("colors").and_then(Value::as_object) {
        for (k, v) in colors {
            if ui["colors"].get(k).is_some() {
                if let Some(s) = v.as_str().filter(|s| is_hex(s)) {
                    ui["colors"][k] = json!(s.to_lowercase());
                }
            }
        }
    }
    ui
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_in_formulas() {
        assert_eq!(refs("split([実績_設備ｺｰｽ],\" \",-1) & [a] & [] & [b"), vec!["実績_設備ｺｰｽ", "a"]);
    }
}
