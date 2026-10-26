//! 文字の直しと正規化（旧版 cm3_client.py の fix_text、master.py の norm_name・norm_key・label・display_value と同じ答え）。
//!
//! - fix_text: CM3 の文字を表示用に直す（HTML のタグを外す・文字参照を戻す・欧文として読まれた日本語を読み直す・全角空白を半角に・前後の空白を除く）
//! - norm_name: 列名をそろえる（NFKC・大文字・空白と _ - . ・ を除く）
//! - norm_key: 突合せの値をそろえる（NFKC・大文字・空白をすべて除く・数は数として: 0.300 = 0.3）
//! - label: 列名の表示（半角カナを全角に）
//! - display_value: 台帳の値の表示（`2026-10-30 00:00:00` → `2026/10/30`）
//!
//! 突き合わせ: desktop/tests/parity.rs が旧版の答え（tests/golden/python.json の text・normKey・normName・display）と比べる。

use unicode_normalization::UnicodeNormalization;

/// Python の `str.isspace()`（正規表現の `\s`）と同じ空白か。Rust の is_whitespace に、区切りの制御文字（0x1C〜0x1F）を足した物。
pub fn py_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Python の `str.strip()`。
pub fn py_strip(s: &str) -> &str {
    s.trim_matches(py_space)
}

/// NFKC（全角・半角）→ 大文字。
pub fn nfkc_upper(s: &str) -> String {
    s.nfkc().collect::<String>().to_uppercase()
}

/// NFKC だけ（列名の表示: 半角カナ → 全角）。
pub fn label(s: &str) -> String {
    s.nfkc().collect()
}

/// 列名をそろえる。
pub fn norm_name(s: &str) -> String {
    nfkc_upper(s).chars().filter(|&c| !(py_space(c) || matches!(c, '_' | '-' | '.' | '・'))).collect()
}

/// `^[+-]?\d+(\.\d+)?$`（数字は ASCII。NFKC のあとなので全角の数字も入る）
fn is_number(s: &str) -> bool {
    let body = s.strip_prefix(['+', '-']).unwrap_or(s);
    let (int, frac) = match body.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (body, None),
    };
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    digits(int) && frac.is_none_or(digits)
}

/// 突合せの値をそろえる（空・無しは ""）。
pub fn norm_key(v: &str) -> String {
    let s: String = nfkc_upper(v).chars().filter(|&c| !py_space(c)).collect();
    if is_number(&s) {
        if let Ok(f) = s.parse::<f64>() {
            let t = format!("{f:.6}");
            return t.trim_end_matches('0').trim_end_matches('.').to_string();
        }
    }
    s
}

/// 台帳の値の表示（前後の空白を除き、0 時の日時は日付だけ・ほかの日時は分まで）。
pub fn display_value(v: &str) -> String {
    let s = py_strip(v);
    let b = s.as_bytes();
    let dig = |r: std::ops::Range<usize>| r.clone().all(|i| b[i].is_ascii_digit());
    // YYYY-MM-DD HH:MM:SS（19 文字）
    if b.len() == 19
        && dig(0..4)
        && b[4] == b'-'
        && dig(5..7)
        && b[7] == b'-'
        && dig(8..10)
        && b[10] == b' '
        && dig(11..13)
        && b[13] == b':'
        && dig(14..16)
        && b[16] == b':'
        && dig(17..19)
    {
        let date = format!("{}/{}/{}", &s[0..4], &s[5..7], &s[8..10]);
        if &s[11..19] == "00:00:00" {
            return date;
        }
        return format!("{date} {}:{}", &s[11..13], &s[14..16]);
    }
    s.to_string()
}

/// 日本語の文字が入っているか（`[　-ヿ㐀-鿿！-ﾟ]`）。
fn has_japanese(s: &str) -> bool {
    s.chars().any(|c| matches!(c, '\u{3000}'..='\u{30ff}' | '\u{3400}'..='\u{9fff}' | '\u{ff01}'..='\u{ff9f}'))
}

/// 欧文として読まれた形の文字があるか（`[\u0080-ÿŒ-ƒˆ-˜–-™]`）。
fn has_latin_high(s: &str) -> bool {
    s.chars().any(|c| matches!(c, '\u{80}'..='\u{ff}' | '\u{152}'..='\u{192}' | '\u{2c6}'..='\u{2dc}' | '\u{2013}'..='\u{2122}'))
}

/// latin-1 へ戻す（U+00FF を超える文字があれば戻せない）。
fn encode_latin1(s: &str) -> Option<Vec<u8>> {
    s.chars().map(|c| u8::try_from(c as u32).ok()).collect()
}

/// cp1252 へ戻す（Python と同じく、cp1252 に無い 0x81・0x8D・0x8F・0x90・0x9D と、表に無い文字は戻せない）。
fn encode_cp1252(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len());
    for c in s.chars() {
        let u = c as u32;
        if u < 0x80 || (0xa0..=0xff).contains(&u) {
            out.push(u as u8);
            continue;
        }
        let mut buf = [0u8; 4];
        let (bytes, _, bad) = encoding_rs::WINDOWS_1252.encode(c.encode_utf8(&mut buf));
        if bad || bytes.len() != 1 || matches!(bytes[0], 0x81 | 0x8d | 0x8f | 0x90 | 0x9d) || !(0x80..0xa0).contains(&bytes[0]) {
            return None;
        }
        out.push(bytes[0]);
    }
    Some(out)
}

/// バイト列を文字に（誤りがあれば None）。
pub fn decode_strict(bytes: &[u8], enc: &'static encoding_rs::Encoding) -> Option<String> {
    if enc == encoding_rs::UTF_8 {
        return String::from_utf8(bytes.to_vec()).ok();
    }
    enc.decode_without_bom_handling_and_without_replacement(bytes).map(|c| c.into_owned())
}

/// HTML のタグ（`<…>`）を空白 1 つに置き換える。
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        let tail = &rest[i + 1..];
        match tail.find('>') {
            // `<>` は `<[^>]+>` に当たらない（中身が要る）
            Some(j) if j > 0 => {
                out.push_str(&rest[..i]);
                out.push(' ');
                rest = &tail[j + 1..];
            }
            _ => {
                out.push_str(&rest[..i + 1]);
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// 表示用に直す。
pub fn fix_text(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let mut s = value.to_string();
    if s.contains('<') && s.contains('>') {
        s = strip_tags(&s);
    }
    if s.contains('&') {
        s = htmlize::unescape(&s).into_owned();
    }
    if has_latin_high(&s) {
        type Encode = fn(&str) -> Option<Vec<u8>>;
        let tries: [(Encode, &'static encoding_rs::Encoding); 4] = [
            (encode_latin1, encoding_rs::UTF_8),
            (encode_cp1252, encoding_rs::UTF_8),
            (encode_latin1, encoding_rs::SHIFT_JIS),
            (encode_cp1252, encoding_rs::SHIFT_JIS),
        ];
        for (enc, dec) in tries {
            if let Some(t) = enc(&s).and_then(|b| decode_strict(&b, dec)) {
                if has_japanese(&t) {
                    s = t;
                    break;
                }
            }
        }
    }
    py_strip(&s.replace('\u{3000}', " ")).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_compare_numbers_as_numbers() {
        assert_eq!(norm_key("0.300"), norm_key("0.3"));
        assert_eq!(norm_key("840.00"), "840");
        assert_eq!(norm_key(" Ｌ４０６１ Ｓ０ "), "L4061S0");
        assert_eq!(norm_key("1."), "1.", "小数点の後に数字が無ければ数ではない");
        assert_eq!(norm_key("\u{1c}1"), "1", "Python の空白（区切りの制御文字）も除く");
    }

    #[test]
    fn text_is_repaired() {
        assert_eq!(fix_text("缶材<br>(蓋)"), "缶材 (蓋)");
        assert_eq!(fix_text("&#12288;点検板&#12288;"), "点検板");
        assert_eq!(fix_text("è¨ˆç”»"), "計画", "cp1252 として読まれた UTF-8");
        assert_eq!(fix_text("café"), "café", "欧文は欧文のまま");
        assert_eq!(fix_text("a<>b"), "a<>b", "中身の無い <> はタグではない");
    }

    #[test]
    fn master_values_for_display() {
        assert_eq!(display_value("2026-10-30 00:00:00"), "2026/10/30");
        assert_eq!(display_value(" 2026-10-18 22:07:42 "), "2026/10/18 22:07");
        assert_eq!(display_value("2026-1-1 00:00:00"), "2026-1-1 00:00:00");
        assert_eq!(norm_name("ﾛｯﾄ 番号"), "ロット番号");
        assert_eq!(label("ｵｰﾀﾞｰ板厚"), "オーダー板厚");
    }
}
