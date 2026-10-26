//! Python と同じ書き方・読み方（旧版の答えと一字一句そろえるため）。
//!
//! - py_float_repr: 小数の `repr`（最短で戻る桁。小数点の位置が -4 以下か 16 を超えれば指数: `1e+16`・`1e-05`）
//! - json_py_str: JSON の値の `str()`（null → `None`・true → `True`・小数は repr）
//! - py_float: `float(値)`（文字は前後の空白を除き、数字の間の `_` も読む・inf・nan）
//! - truthy: Python の真偽（空の文字・0・空の並びは偽）
//! - py_round: `round(x, n)`（2 進の値をそのまま 10 進で丸める）
//! - sqlite_str: SQLite の値の `str()`（整数・小数・文字・バイト列。NULL は空）

use rusqlite::types::ValueRef;
use serde_json::Value;

/// 小数の repr。
pub fn py_float_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    // Rust の {:e} は最短で戻る桁（例 1.5e-5）
    let e = format!("{:e}", x.abs());
    let (mant, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mant.chars().filter(|c| c.is_ascii_digit()).collect();
    let sign = if x < 0.0 { "-" } else { "" };
    let decpt = exp + 1;
    if decpt <= -4 || decpt > 16 {
        let m = if digits.len() > 1 { format!("{}.{}", &digits[..1], &digits[1..]) } else { digits.clone() };
        return format!("{sign}{m}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs());
    }
    if decpt <= 0 {
        return format!("{sign}0.{}{}", "0".repeat((-decpt) as usize), digits);
    }
    let d = decpt as usize;
    if digits.len() <= d {
        format!("{sign}{}{}.0", digits, "0".repeat(d - digits.len()))
    } else {
        format!("{sign}{}.{}", &digits[..d], &digits[d..])
    }
}

/// JSON の値の str()。
pub fn json_py_str(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => i.to_string(),
            (_, Some(u)) => u.to_string(),
            _ => py_float_repr(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Python の真偽。
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// 文字の float()。読めなければ None。
pub fn py_float_str(s: &str) -> Option<f64> {
    let t = crate::text::py_strip(s);
    if t.is_empty() {
        return None;
    }
    let t: std::borrow::Cow<str> = if t.contains('_') {
        // `_` は数字と数字の間だけ
        let b = t.as_bytes();
        let ok = b.iter().enumerate().all(|(i, &c)| {
            c != b'_' || (i > 0 && i + 1 < b.len() && b[i - 1].is_ascii_digit() && b[i + 1].is_ascii_digit())
        });
        if !ok {
            return None;
        }
        t.replace('_', "").into()
    } else {
        t.into()
    };
    let body = t.trim_start_matches(['+', '-']);
    if t.len() - body.len() > 1 {
        return None;
    }
    let low = body.to_ascii_lowercase();
    if matches!(low.as_str(), "inf" | "infinity" | "nan") {
        return t.parse::<f64>().ok();
    }
    // Rust だけが読む形（"1e" など）を除くため、数字・点・指数だけの形を先に確かめる
    let (mant, exp) = match low.split_once('e') {
        Some((m, e)) => (m, Some(e)),
        None => (low.as_str(), None),
    };
    let mant_ok = {
        let (a, b) = mant.split_once('.').unwrap_or((mant, ""));
        (a.bytes().all(|c| c.is_ascii_digit()) && b.bytes().all(|c| c.is_ascii_digit())) && !(a.is_empty() && b.is_empty())
    };
    let exp_ok = exp.is_none_or(|e| {
        let d = e.strip_prefix(['+', '-']).unwrap_or(e);
        !d.is_empty() && d.bytes().all(|c| c.is_ascii_digit())
    });
    if !(mant_ok && exp_ok) {
        return None;
    }
    t.parse::<f64>().ok()
}

/// 値の float()。
pub fn py_float(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => py_float_str(s),
        _ => None,
    }
}

/// round(x, n)。結果が有限でなければ None。
pub fn py_round(x: f64, n: usize) -> Option<f64> {
    if !x.is_finite() {
        return None;
    }
    format!("{x:.n$}").parse().ok()
}

/// バイト列の repr（b'…'）。
fn bytes_repr(b: &[u8]) -> String {
    let quote = if b.contains(&b'\'') && !b.contains(&b'"') { '"' } else { '\'' };
    let mut s = format!("b{quote}");
    for &c in b {
        match c {
            b'\t' => s.push_str("\\t"),
            b'\n' => s.push_str("\\n"),
            b'\r' => s.push_str("\\r"),
            b'\\' => s.push_str("\\\\"),
            c if c == quote as u8 => {
                s.push('\\');
                s.push(c as char);
            }
            0x20..=0x7e => s.push(c as char),
            _ => s.push_str(&format!("\\x{c:02x}")),
        }
    }
    s.push(quote);
    s
}

/// SQLite の値の str()（NULL は空）。
pub fn sqlite_str(v: ValueRef<'_>) -> String {
    match v {
        ValueRef::Null => String::new(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => py_float_repr(f),
        ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
        ValueRef::Blob(b) => bytes_repr(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_repr_like_python() {
        for (x, want) in [
            (1.5, "1.5"),
            (3.0, "3.0"),
            (0.1, "0.1"),
            (1e16, "1e+16"),
            (1234567890123456.0, "1234567890123456.0"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (-2.5e-7, "-2.5e-07"),
            (123.456, "123.456"),
            (1e22, "1e+22"),
            (-0.0, "-0.0"),
        ] {
            assert_eq!(py_float_repr(x), want, "{x}");
        }
    }

    #[test]
    fn float_parse_like_python() {
        assert_eq!(py_float_str(" 840 "), Some(840.0));
        assert_eq!(py_float_str("1_000"), Some(1000.0));
        assert_eq!(py_float_str("1__0"), None);
        assert_eq!(py_float_str("1."), Some(1.0));
        assert_eq!(py_float_str(".5"), Some(0.5));
        assert_eq!(py_float_str("1e3"), Some(1000.0));
        assert_eq!(py_float_str("1e"), None);
        assert_eq!(py_float_str("."), None);
        assert_eq!(py_float_str("--1"), None);
        assert!(py_float_str("-Infinity").is_some_and(|f| f.is_infinite()));
        assert_eq!(py_float_str("0x10"), None);
    }

    #[test]
    fn rounding_and_text() {
        assert_eq!(py_round(45.45, 1), Some(45.5), "45.45 は 2 進では 45.4500000000000028…");
        assert_eq!(py_round(0.25, 1), Some(0.2), "ちょうど半分は偶数へ");
        assert_eq!(json_py_str(&serde_json::json!(null)), "None");
        assert_eq!(json_py_str(&serde_json::json!(7)), "7");
        assert_eq!(bytes_repr(b"a'\x00"), "b\"a'\\x00\"");
    }
}
