//! CM3 のスケジュール（旧版 cm3_client.py と同じ答え。仕様は program/docs/CM3_API_spec.md の 4〜5 章）。
//!
//! `POST <cm3_base_url><init_path>` → `entity.meisaiList` を画面の形に整える。
//! 行の区分: lot（製造）・inspect（点検キーワードを含む、板厚・幅のある行）・stop（設備停止。時刻と文字だけの行）。

use crate::pyfmt::{json_py_str, py_float};
use crate::text::{fix_text, nfkc_upper};
use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, TimeZone, Timelike};
use serde_json::{json, Map, Value};
use std::time::Duration;

/// CM3 に届かない・誤りを返したとき。
#[derive(Debug, Clone, PartialEq)]
pub struct Cm3Error(pub String);

impl std::fmt::Display for Cm3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn jst() -> FixedOffset {
    FixedOffset::east_opt(9 * 3600).expect("JST")
}

/// 要求の本文（DevTools の Payload で見た形。設備コードを 2 か所に入れる）。
pub fn build_payload(setsubi: &str, term_kbn: &str, client_ip: &Value) -> Value {
    json!({
        "ip": client_ip, "userID": null, "termID": null, "setsubiName": setsubi, "useKbn": null, "termKbn": term_kbn,
        "authInfo": null, "result": 0, "error": false, "errorMessage": "", "version": null, "setsubi": "",
        "updateTs": null, "meisaiList": [], "urlSetsubi": setsubi, "urlTermKbn": term_kbn,
    })
}

/// 応答のバイト列を文字に（Content-Type の charset → UTF-8 → CP932。どれも駄目なら UTF-8 で置き換えながら）。
pub fn decode_body(bytes: &[u8], content_type: &str) -> String {
    let lower = content_type.to_ascii_lowercase();
    let named = lower.find("charset=").map(|i| {
        let rest = &content_type[i + 8..];
        rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect::<String>()
    });
    let mut cands: Vec<&'static encoding_rs::Encoding> = vec![];
    if let Some(enc) = named.as_deref().and_then(|n| encoding_rs::Encoding::for_label(n.as_bytes())) {
        cands.push(enc);
    }
    cands.extend([encoding_rs::UTF_8, encoding_rs::SHIFT_JIS]);
    for enc in cands {
        if let Some(s) = crate::text::decode_strict(bytes, enc) {
            return s;
        }
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// 接続の設定（config.json）。
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub base_url: String,
    pub init_path: String,
    pub client_ip: Value,
    pub timeout: Duration,
    /// 環境変数のプロキシ（HTTP_PROXY など）を使うか。既定は使わない（社内の CM3 へは直接つなぐ）
    pub use_system_proxy: bool,
}

/// CM3 の応答そのまま（JSON）。
pub fn fetch_raw(ep: &Endpoint, setsubi: &str, term_kbn: &str) -> Result<Value, Cm3Error> {
    let base = ep.base_url.trim_end_matches('/');
    let body = serde_json::to_vec(&build_payload(setsubi, term_kbn, &ep.client_ip)).unwrap_or_default();
    let proxy = if ep.use_system_proxy { ureq::Proxy::try_from_env() } else { None };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(ep.timeout))
        .proxy(proxy)
        .build()
        .into();
    let sent = agent
        .post(&format!("{base}{}", ep.init_path))
        .header("Accept", "application/json, text/plain, */*")
        .header("Content-Type", "application/json;charset=UTF-8")
        .header("Origin", base)
        .header("Referer", &format!("{base}/Cm3SchWeb/"))
        .send(&body[..]);
    let mut res = match sent {
        Ok(r) => r,
        Err(ureq::Error::Timeout(_)) => return Err(Cm3Error("CM3の応答がタイムアウトしました".into())),
        Err(e) => return Err(Cm3Error(format!("CM3に接続できません: {e}"))),
    };
    let status = res.status();
    if status.as_u16() >= 400 {
        return Err(Cm3Error(format!("CM3がHTTPエラーを返しました: {} {}", status.as_u16(), status.canonical_reason().unwrap_or(""))));
    }
    let ctype = res.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let bytes = match res.body_mut().with_config().limit(256 << 20).read_to_vec() {
        Ok(b) => b,
        Err(ureq::Error::Timeout(_)) => return Err(Cm3Error("CM3の応答がタイムアウトしました".into())),
        Err(e) => return Err(Cm3Error(format!("CM3に接続できません: {e}"))),
    };
    serde_json::from_str(&decode_body(&bytes, &ctype)).map_err(|_| Cm3Error("CM3の応答がJSONではありません".into()))
}

// ---------------------------------------------------------------- 行の区分

/// 製造行なら必ずある項目（8 つ）と、板厚・幅（数のはずの 5 つ）。
const LOT_DATA_KEYS: [&str; 8] = ["zaishitsu", "inItaAtsu", "outItaAtsu", "reienHaba", "seihinItaAtsu", "seihinItaHaba", "keijo", "jiKotei"];
const DIMENSION_KEYS: [&str; 5] = ["inItaAtsu", "outItaAtsu", "reienHaba", "seihinItaAtsu", "seihinItaHaba"];
const LOT_MIN_FIELDS: usize = 4;
/// 画面の見た目・リンクのための項目（停止の文言に入れない）
const NON_TEXT_KEYS: [&str; 5] = ["rowStyle", "teisiStyle", "ltNoRendered", "lotToiawaseUrl", "yotoMei"];

fn filled(v: Option<&Value>) -> bool {
    v.is_some_and(|v| !v.is_null() && !crate::text::py_strip(&json_py_str(v)).is_empty())
}

/// 設備停止行か（行のデータの量で決める。見た目の印では決めない）: 8 項目のうち 4 つ以上が埋まり、板厚・幅のどれかが数なら製造。
pub fn is_stop_row(m: &Map<String, Value>) -> bool {
    let n_filled = LOT_DATA_KEYS.iter().filter(|k| filled(m.get(**k))).count();
    let numeric = DIMENSION_KEYS.iter().filter(|k| m.get(**k).and_then(py_float).is_some()).count();
    !(n_filled >= LOT_MIN_FIELDS && numeric >= 1)
}

/// ロット番号か用途名に点検キーワードを含むか（NFKC・大文字でそろえて）。
pub fn is_inspect_row(lot: &str, yoto: &str, keywords: &[String]) -> bool {
    let text = format!("{} {}", nfkc_upper(lot), nfkc_upper(yoto));
    keywords.iter().any(|k| !k.is_empty() && text.contains(&nfkc_upper(k)))
}

/// `^\d+(\.\d+)?\s*(分|h|H|時間)?$`（時間の長さ: 「7分」）
fn is_duration(v: &str) -> bool {
    let num_end = v.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(v.len());
    let (num, rest) = v.split_at(num_end);
    let num_ok = match num.split_once('.') {
        Some((a, b)) => !a.is_empty() && !b.is_empty() && a.bytes().all(|c| c.is_ascii_digit()) && b.bytes().all(|c| c.is_ascii_digit()),
        None => !num.is_empty(),
    };
    let unit = rest.trim_start_matches(crate::text::py_space);
    num_ok && matches!(unit, "" | "分" | "h" | "H" | "時間")
}

/// 停止の文言（応答の項目の順のまま、空白を入れずにつなぐ。時間の長さは最後に空白 1 つで）。
pub fn stop_text(m: &Map<String, Value>) -> String {
    let (mut name, mut extra) = (String::new(), Vec::new());
    for (k, v) in m {
        if NON_TEXT_KEYS.contains(&k.as_str()) || k == "kanryoYotei" || v.is_null() || v.is_boolean() || v.is_object() || v.is_array() {
            continue;
        }
        let v = fix_text(&json_py_str(v));
        if v.is_empty() {
            continue;
        }
        if is_duration(&v) {
            extra.push(v);
        } else {
            name.push_str(&v);
        }
    }
    let joined = std::iter::once(name).chain(extra).collect::<Vec<_>>().join(" ");
    let t = crate::text::py_strip(&joined);
    if t.is_empty() {
        "設備停止".into()
    } else {
        t.into()
    }
}

// ---------------------------------------------------------------- 整える

/// `MM/DD HH:MM`（日付つき）か `HH:MM`（時刻だけ）。
enum When {
    Date(u32, u32, u32, u32),
    Time(u32, u32),
    None,
}

fn parse_when(s: &str) -> When {
    let s = crate::text::py_strip(s);
    let num = |t: &str, exact: bool| -> Option<u32> {
        let ok = !t.is_empty() && t.len() <= 2 && (!exact || t.len() == 2) && t.bytes().all(|b| b.is_ascii_digit());
        ok.then(|| t.parse().ok()).flatten()
    };
    let time = |t: &str| t.split_once(':').and_then(|(h, m)| Some((num(h, false)?, num(m, true)?)));
    let Some((month, rest)) = s.split_once('/') else {
        return time(s).map(|(h, m)| When::Time(h, m)).unwrap_or(When::None);
    };
    // 月/日 と 時刻の間に 1 つ以上の空白
    let Some(gap) = rest.find(crate::text::py_space) else { return When::None };
    let (day, tm) = (&rest[..gap], rest[gap..].trim_start_matches(crate::text::py_space));
    match (num(month, false), num(day, false), time(tm)) {
        (Some(mo), Some(d), Some((h, mi))) => When::Date(mo, d, h, mi),
        _ => When::None,
    }
}

/// CM3 の更新日時（UNIX 時間・ミリ秒）→ 日本時間。
fn update_dt(ts: &Value) -> Option<DateTime<FixedOffset>> {
    let ms: i64 = match ts {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.trunc() as i64))?,
        Value::String(s) => crate::text::py_strip(s).parse().ok()?,
        Value::Bool(b) => *b as i64,
        _ => return None,
    };
    jst().timestamp_millis_opt(ms).single()
}

fn fmt_md_hm(t: &DateTime<FixedOffset>) -> String {
    t.format("%m/%d %H:%M").to_string()
}

/// Python の datetime.isoformat()（秒の小数が 0 なら付けない）。
fn iso(t: &DateTime<FixedOffset>) -> String {
    if t.nanosecond() == 0 {
        t.format("%Y-%m-%dT%H:%M:%S%:z").to_string()
    } else {
        t.format("%Y-%m-%dT%H:%M:%S%.6f%:z").to_string()
    }
}

/// 圧下率（%）= (入側 − 出側) / 入側 × 100。小数 1 桁に丸める。
fn reduction(a: Option<&Value>, b: Option<&Value>) -> Value {
    let (a, b) = (a.and_then(py_float), b.and_then(py_float));
    match (a, b) {
        (Some(a), Some(b)) if a != 0.0 => crate::pyfmt::py_round(((a - b) / a) * 100.0, 1).map(|r| json!(r)).unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

fn text_of(m: &Map<String, Value>, k: &str) -> String {
    match m.get(k) {
        None | Some(Value::Null) | Some(Value::Bool(false)) => String::new(),
        Some(Value::String(s)) => fix_text(s),
        Some(v) if crate::pyfmt::truthy(v) => fix_text(&json_py_str(v)),
        Some(_) => String::new(),
    }
}

/// CM3 の応答を画面の形に整える。
pub fn normalize(raw: &Value, own_setsubi: &str, inspect_keywords: &[String]) -> Result<Value, Cm3Error> {
    let Some(raw) = raw.as_object() else { return Err(Cm3Error("CM3応答の形式が不正です".into())) };
    if raw.get("status") != Some(&json!("SUCCESS")) {
        return Err(Cm3Error(format!("CM3応答 status={}", raw.get("status").map(json_py_str).unwrap_or_else(|| "None".into()))));
    }
    let empty = Map::new();
    let entity = raw.get("entity").filter(|v| crate::pyfmt::truthy(v)).and_then(Value::as_object).unwrap_or(&empty);
    if entity.get("error").is_some_and(crate::pyfmt::truthy) {
        let msg = match entity.get("errorMessage") {
            Some(Value::String(s)) => fix_text(s),
            Some(v) if crate::pyfmt::truthy(v) => json_py_str(v),
            _ => String::new(),
        };
        return Err(Cm3Error(format!("CM3エラー: {}", if msg.is_empty() { "詳細不明".into() } else { msg })));
    }

    let upd = entity.get("updateTs").and_then(update_dt);
    let base_year = upd.map(|t| t.year()).unwrap_or_else(|| chrono::Utc::now().with_timezone(&jst()).year());
    let (mut cur_month, mut cur_day) = (upd.map(|t| t.month()), upd.map(|t| t.day()));
    let mut prev_dt: Option<DateTime<FixedOffset>> = None;
    let mut prev_end: Option<DateTime<FixedOffset>> = None;
    let mut lot_count: Vec<(String, u64)> = vec![];
    let mut items: Vec<Map<String, Value>> = vec![];
    let none = vec![];
    let list = entity.get("meisaiList").and_then(Value::as_array).unwrap_or(&none);

    for (idx, row) in list.iter().enumerate() {
        let empty_row = Map::new();
        let m = row.as_object().unwrap_or(&empty_row);
        let raw_kanryo = text_of(m, "kanryoYotei");
        let mut kanryo: Option<DateTime<FixedOffset>> = None;
        let (hm, is_date) = match parse_when(&raw_kanryo) {
            When::Date(mo, d, h, mi) => {
                cur_month = Some(mo);
                cur_day = Some(d);
                (Some((h, mi)), true)
            }
            When::Time(h, mi) => (Some((h, mi)), false),
            When::None => (None, false),
        };
        if let (Some((h, mi)), Some(mo), Some(d)) = (hm, cur_month, cur_day) {
            let mut year = base_year;
            if upd.is_some_and(|u| (mo as i64) < u.month() as i64 - 6) {
                year += 1;
            }
            let made = NaiveDate::from_ymd_opt(year, mo, d).and_then(|date| date.and_hms_opt(h, mi, 0));
            if let Some(naive) = made {
                let mut t = jst().from_local_datetime(&naive).single().expect("JST は一意");
                if prev_dt.is_some_and(|p| t < p) && !is_date {
                    t += chrono::Duration::days(1);
                    cur_month = Some(t.month());
                    cur_day = Some(t.day());
                }
                prev_dt = Some(t);
                kanryo = Some(t);
            }
        }
        let mut item = Map::new();
        item.insert("seq".into(), json!(idx + 1));
        item.insert("setsubi".into(), json!(own_setsubi));
        item.insert("kanryoYoteiRaw".into(), json!(raw_kanryo));
        item.insert("kanryoYotei".into(), json!(kanryo.as_ref().map(fmt_md_hm).unwrap_or_else(|| raw_kanryo.clone())));
        item.insert("kanryoYoteiIso".into(), kanryo.as_ref().map(|t| json!(iso(t))).unwrap_or(Value::Null));
        item.insert("startYotei".into(), json!(prev_end.as_ref().map(fmt_md_hm).unwrap_or_default()));
        item.insert("startYoteiIso".into(), prev_end.as_ref().map(|t| json!(iso(t))).unwrap_or(Value::Null));
        item.insert("teisiStyle".into(), m.get("teisiStyle").cloned().unwrap_or(Value::Null));
        item.insert("rowStyle".into(), m.get("rowStyle").cloned().unwrap_or(Value::Null));
        item.insert("passNo".into(), Value::Null);
        item.insert("passTotal".into(), Value::Null);
        if kanryo.is_some() {
            prev_end = kanryo;
        }
        if is_stop_row(m) {
            for (k, v) in [
                ("rowKind", json!("stop")),
                ("isStop", json!(true)),
                ("stopText", json!(stop_text(m))),
                ("reductionRate", Value::Null),
                ("selfNext", json!(false)),
            ] {
                item.insert(k.into(), v);
            }
            for k in ["lotNo", "zaishitsu", "inItaAtsu", "outItaAtsu", "reienHaba", "seihinItaAtsu", "seihinItaHaba", "keijo", "yotoMei", "jiKotei"]
            {
                item.insert(k.into(), json!(""));
            }
        } else {
            let lot = text_of(m, "lotNo");
            let yoto = {
                let full = text_of(m, "yotoMeiFull");
                if m.get("yotoMeiFull").is_some_and(crate::pyfmt::truthy) {
                    full
                } else {
                    text_of(m, "yotoMei")
                }
            };
            let ji = text_of(m, "jiKotei");
            let kind = if is_inspect_row(&lot, &yoto, inspect_keywords) { "inspect" } else { "lot" };
            if kind == "lot" {
                let n = match lot_count.iter_mut().find(|(l, _)| *l == lot) {
                    Some((_, c)) => {
                        *c += 1;
                        *c
                    }
                    None => {
                        lot_count.push((lot.clone(), 1));
                        1
                    }
                };
                item.insert("passNo".into(), json!(n));
            }
            item.insert("rowKind".into(), json!(kind));
            item.insert("isStop".into(), json!(false));
            item.insert("stopText".into(), json!(""));
            item.insert("lotNo".into(), json!(lot));
            for k in ["zaishitsu", "inItaAtsu", "outItaAtsu"] {
                item.insert(k.into(), json!(text_of(m, k)));
            }
            item.insert("reductionRate".into(), reduction(m.get("inItaAtsu"), m.get("outItaAtsu")));
            for k in ["reienHaba", "seihinItaAtsu", "seihinItaHaba", "keijo"] {
                item.insert(k.into(), json!(text_of(m, k)));
            }
            item.insert("yotoMei".into(), json!(yoto));
            item.insert("jiKotei".into(), json!(ji));
            item.insert("selfNext".into(), json!(ji == own_setsubi));
        }
        items.push(item);
    }
    for it in items.iter_mut().filter(|it| it["rowKind"] == "lot") {
        let lot = it["lotNo"].as_str().unwrap_or("").to_string();
        let total = lot_count.iter().find(|(l, _)| *l == lot).map(|(_, c)| *c).unwrap_or(1);
        it.insert("passTotal".into(), json!(total));
    }
    let count = |kind: &str| items.iter().filter(|i| i["rowKind"] == kind).count();
    let setsubi = entity.get("setsubiName").filter(|v| crate::pyfmt::truthy(v)).cloned().unwrap_or_else(|| json!(own_setsubi));
    Ok(json!({
        "setsubi": setsubi,
        "termKbn": entity.get("termKbn").cloned().unwrap_or(Value::Null),
        "updateTs": entity.get("updateTs").cloned().unwrap_or(Value::Null),
        "updateTime": upd.map(|t| json!(t.format("%Y/%m/%d %H:%M:%S").to_string())).unwrap_or(Value::Null),
        "count": count("lot"),
        "inspectCount": count("inspect"),
        "stopCount": count("stop"),
        "items": items,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_text_keeps_the_order_of_the_response() {
        let m: Map<String, Value> =
            serde_json::from_str(r#"{"roll":"ワークロ","lotNo":"ール交換計","zaishitsu":"画*","pass":"7分","kanryoYotei":"10/19 14:05","rowStyle":"x"}"#)
                .unwrap();
        assert!(is_stop_row(&m));
        assert_eq!(stop_text(&m), "ワークロール交換計画* 7分");
    }

    #[test]
    fn durations_and_times() {
        assert!(is_duration("7分") && is_duration("1.5 時間") && is_duration("30") && !is_duration("7分間") && !is_duration("1."));
        assert!(matches!(parse_when("10/19 14:05"), When::Date(10, 19, 14, 5)));
        assert!(matches!(parse_when("     14:27"), When::Time(14, 27)));
        assert!(matches!(parse_when("10/19  9:05"), When::Date(10, 19, 9, 5)));
        assert!(matches!(parse_when("10/1914:05"), When::None));
        assert!(matches!(parse_when("14:5"), When::None));
    }

    #[test]
    fn charset_then_utf8_then_cp932() {
        let sjis = encoding_rs::SHIFT_JIS.encode("設備").0.into_owned();
        assert_eq!(decode_body(&sjis, "application/json"), "設備");
        assert_eq!(decode_body("設備".as_bytes(), "application/json;charset=UTF-8"), "設備");
        assert_eq!(decode_body(&sjis, "text/plain; charset=Shift_JIS"), "設備");
    }
}
