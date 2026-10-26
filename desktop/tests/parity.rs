//! 移行の評価関数: 旧版（Python・Flask）の答え（tests/golden/python.json。program/tools/golden_from_python.py が記録）と、
//! Rust の答えを突き合わせる。同じ作り物のデータ・同じ順の問い合わせで、JSON・CSV・Excel の中身が一字一句同じであること。
//!
//! 毎回変わる値（時刻・手元のパス）は旧版の記録と同じく印に置き換えて比べる。Rust で足した鍵（loading: 台帳を裏で読んでいる最中か）は除いて比べる。

mod common;

use serde_json::{json, Value};
use std::time::Duration;
use sv_core::app::Request;

const VOLATILE: &str = "<VOLATILE>";
const VOLATILE_KEYS: [&str; 6] = ["fetchedAt", "copiedAt", "loadedAt", "sourceMtime", "local", "source"];
/// Rust で足した鍵（旧版には無い）
const ADDED_KEYS: [&str; 1] = ["loading"];

fn golden() -> Value {
    serde_json::from_slice(&std::fs::read(common::repo().join("desktop/tests/golden/python.json")).unwrap()).unwrap()
}

fn scrub(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .filter(|(k, _)| !ADDED_KEYS.contains(&k.as_str()))
                .map(|(k, x)| {
                    let blank = x.is_null() || x == "";
                    (k.clone(), if VOLATILE_KEYS.contains(&k.as_str()) && !blank { json!(VOLATILE) } else { scrub(x) })
                })
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(scrub).collect()),
        v => v.clone(),
    }
}

/// 違いを道筋つきで集める（最初の数件）。
fn diff(path: &str, a: &Value, b: &Value, out: &mut Vec<String>) {
    if out.len() >= 15 {
        return;
    }
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys().filter(|k| !x.contains_key(*k))) {
                match (x.get(k), y.get(k)) {
                    (Some(p), Some(q)) => diff(&format!("{path}.{k}"), p, q, out),
                    (p, q) => out.push(format!("{path}.{k}: 旧版 {p:?} / Rust {q:?}")),
                }
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                out.push(format!("{path}: 数が違う 旧版 {} / Rust {}", x.len(), y.len()));
            }
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                diff(&format!("{path}[{i}]"), p, q, out);
            }
        }
        (Value::Number(x), Value::Number(y)) if x.as_f64() == y.as_f64() => {}
        _ if a == b => {}
        _ => out.push(format!("{path}: 旧版 {a} / Rust {b}")),
    }
}

fn assert_same(what: &str, want: &Value, got: &Value) {
    let mut out = vec![];
    diff("", want, &scrub(got), &mut out);
    assert!(out.is_empty(), "{what} が旧版と違います:\n  {}", out.join("\n  "));
}

/// 伏せずに比べる（設定の形など、毎回変わる値を持たない物）。
fn assert_exact(what: &str, want: &Value, got: &Value) {
    let mut out = vec![];
    diff("", want, got, &mut out);
    assert!(out.is_empty(), "{what} が旧版と違います:\n  {}", out.join("\n  "));
}

#[test]
fn text_rules_match_python() {
    let g = golden();
    for c in g["text"].as_array().unwrap() {
        assert_eq!(sv_core::text::fix_text(c[0].as_str().unwrap()), c[1].as_str().unwrap(), "fix_text({})", c[0]);
    }
    for c in g["normKey"].as_array().unwrap() {
        assert_eq!(sv_core::text::norm_key(c[0].as_str().unwrap()), c[1].as_str().unwrap(), "norm_key({})", c[0]);
    }
    for c in g["normName"].as_array().unwrap() {
        assert_eq!(sv_core::text::norm_name(c[0].as_str().unwrap()), c[1].as_str().unwrap(), "norm_name({})", c[0]);
        assert_eq!(sv_core::text::label(c[0].as_str().unwrap()), c[2].as_str().unwrap(), "label({})", c[0]);
    }
    for c in g["display"].as_array().unwrap() {
        let s = if c[0].is_null() { String::new() } else { sv_core::pyfmt::json_py_str(&c[0]) };
        assert_eq!(sv_core::text::display_value(&s), c[1].as_str().unwrap(), "display_value({})", c[0]);
    }
}

#[test]
fn schedules_and_apis_match_python() {
    let g = golden();
    let data = common::tmp("parity-data");
    common::fixture(&data, 6881);
    // 作り物のデータが旧版の記録と同じこと（乱数の種を固定してある）
    for (eq, want) in g["normalize"].as_object().unwrap() {
        let raw: Value = serde_json::from_slice(&std::fs::read(data.join("cm3").join(format!("{eq}.json"))).unwrap()).unwrap();
        let got = sv_core::cm3::normalize(&raw, eq, &["点検板".to_string()]).unwrap();
        assert_same(&format!("normalize({eq})"), want, &got);
    }
    for c in g["normalizeErrors"].as_array().unwrap() {
        let got = sv_core::cm3::normalize(&c[0], "L-1", &[]).err().map(|e| e.0);
        assert_eq!(got.as_deref(), c[1].as_str(), "normalize の誤り {}", c[0]);
    }

    let work = common::tmp("parity-work");
    let url = common::mock_cm3(&data, Duration::ZERO);
    let app = common::app(&work, &data, &url, |_| {});
    // 旧版は最初の問い合わせで台帳を読んでいた。ここでは先に読み終えておく（裏の糸の代わり）
    app.master.refresh(false);
    for c in g["calls"].as_array().unwrap() {
        let url = c["url"].as_str().unwrap();
        let (path, query) = url.split_once('?').unwrap_or((url, ""));
        let body = if c["body"].is_null() { vec![] } else { serde_json::to_vec(&c["body"]).unwrap() };
        let method = c["method"].as_str().unwrap();
        let r = app.handle(&Request { method, path, query, body: &body });
        let what = format!("{method} {url}");
        assert_eq!(r.status as i64, c["status"].as_i64().unwrap(), "{what} の状態");
        assert_eq!(r.ctype.split(';').next().unwrap(), c["type"].as_str().unwrap(), "{what} の種類");
        let disp = r.headers.iter().find(|(k, _)| k == "Content-Disposition").map(|(_, v)| v.clone()).unwrap_or_default();
        let want_disp = c["disposition"].as_str().unwrap();
        // 名前の時刻（yyyymmdd_hhmm）は除いて比べる
        let strip = |s: &str| s.chars().filter(|ch| !ch.is_ascii_digit()).collect::<String>();
        assert_eq!(strip(&disp), strip(want_disp), "{what} のファイル名");
        let want = &c["payload"];
        match c["type"].as_str().unwrap() {
            "application/json" => assert_same(&what, want, &serde_json::from_slice(&r.body).unwrap()),
            "text/csv" => {
                let text = String::from_utf8(r.body.clone()).unwrap();
                assert!(text.starts_with('\u{feff}'), "CSV は BOM つき");
                assert_eq!(&text['\u{feff}'.len_utf8()..], want.as_str().unwrap(), "{what}");
            }
            t if t.contains("spreadsheetml") => {
                let mut z = zip::ZipArchive::new(std::io::Cursor::new(r.body.clone())).unwrap();
                let names: Vec<String> = (0..z.len()).map(|i| z.by_index(i).unwrap().name().unwrap().to_string()).collect();
                let want_names: Vec<&String> = want.as_object().unwrap().keys().collect();
                assert_eq!(names.iter().collect::<Vec<_>>(), want_names, "Excel の部品の並び");
                for n in names {
                    let mut text = String::new();
                    std::io::Read::read_to_string(&mut z.by_name(&n).unwrap(), &mut text).unwrap();
                    if n == "docProps/core.xml" {
                        let (i, j) = (text.find("<dcterms:created").unwrap(), text.find("</dcterms:created>").unwrap());
                        text = format!("{}{VOLATILE}{}", &text[..i], &text[j + "</dcterms:created>".len()..]);
                    }
                    assert_eq!(text, want[&n].as_str().unwrap(), "{what} の {n}");
                }
            }
            t => panic!("知らない種類 {t}"),
        }
    }
    // 旧い列の設定（版 1）に前工程・釜番を足す
    let mig = g["migrate"].as_array().unwrap();
    let mut v = mig[0].clone();
    assert!(sv_core::view::migrate_view(&mut v, &app.hot));
    assert_exact("migrate_view", &mig[1], &v);
    // 画面に埋める初めの値
    let boot = app.boot();
    for k in ["equipments", "scheduleColumns", "scheduleFields", "uiDefault"] {
        assert_exact(&format!("boot.{k}"), &g["boot"][k], &boot[k]);
    }
    let _ = std::fs::remove_dir_all(&data);
    let _ = std::fs::remove_dir_all(&work);
}

/// 評価関数が甘くないか: わざと違う答え（台帳のキーを外した）にすると、突き合わせが落ちること。
#[test]
fn the_comparison_catches_a_difference() {
    let g = golden();
    let want = g["calls"].as_array().unwrap().iter().find(|c| c["url"] == "/api/cm3/schedule?setsubi=L-2").unwrap()["payload"].clone();
    let mut broken = want.clone();
    broken["items"][0]["lotNo"] = json!("X0000000");
    let mut out = vec![];
    diff("", &want, &scrub(&broken), &mut out);
    assert_eq!(out.len(), 1, "{out:?}");
    broken = want.clone();
    broken["masterHit"] = json!(0);
    out.clear();
    diff("", &want, &scrub(&broken), &mut out);
    assert_eq!(out.len(), 1);
}
