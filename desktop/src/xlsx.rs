//! Excel（.xlsx）を作る（旧版 xlsx_writer.py と同じ中身）。画面が「表示のまま」の行・色・幅を送り、ここは XML を組んで ZIP にするだけ。
//!
//! 入力: {sheet, headers, widths, aligns, rows: [{cells: [{v}|{n, fmt}|{}], bg, color, bold, styles: {"列": {bg, color, bold}}, merge: [始, 終]}], info}

use serde_json::Value;
use std::io::Write;

/// XML に書けない制御文字を除き、& < > " を書き換える。
fn x(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}' => {}
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// `#RRGGBB`（# は無くてもよい）→ `FFRRGGBB`
fn argb(c: &str) -> Option<String> {
    let h = c.strip_prefix('#').unwrap_or(c);
    (h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit())).then(|| format!("FF{}", h.to_ascii_uppercase()))
}

/// 列の番号（0 から）→ A, B, …, Z, AA
pub fn col_letter(i: usize) -> String {
    let (mut n, mut s) = (i + 1, Vec::new());
    while n > 0 {
        let r = (n - 1) % 26;
        s.push(b'A' + r as u8);
        n = (n - 1) / 26;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

/// シートの名前（使えない文字は _・前後の ' を外す・31 文字まで）
pub fn sheet_name(name: &str) -> String {
    let n: String = name.chars().map(|c| if "[]:*?/\\".contains(c) { '_' } else { c }).collect();
    let n: String = n.trim_matches('\'').chars().take(31).collect();
    if n.is_empty() {
        "Sheet1".into()
    } else {
        n
    }
}

#[derive(PartialEq, Clone)]
struct Xf {
    font: usize,
    fill: usize,
    numfmt: u32,
    border: u32,
    align: String,
    wrap: bool,
}

/// 書式の登録（字・塗り・数の書式・セルの書式）。
struct Styles {
    fonts: Vec<(String, bool)>,
    fills: Vec<String>,
    numfmts: Vec<(String, u32)>,
    xfs: Vec<Xf>,
}

impl Styles {
    fn new() -> Styles {
        Styles {
            fonts: vec![(String::new(), false)],
            // 0: なし・1: gray125（Excel が要る既定の 2 つ）
            fills: vec![String::new(), "gray125".into()],
            numfmts: vec![],
            xfs: vec![Xf { font: 0, fill: 0, numfmt: 0, border: 0, align: String::new(), wrap: false }],
        }
    }
    fn font(&mut self, color: &str, bold: bool) -> usize {
        let k = (argb(color).unwrap_or_default(), bold);
        match self.fonts.iter().position(|f| *f == k) {
            Some(i) => i,
            None => {
                self.fonts.push(k);
                self.fonts.len() - 1
            }
        }
    }
    fn fill(&mut self, bg: &str) -> usize {
        let Some(k) = argb(bg) else { return 0 };
        match self.fills.iter().position(|f| *f == k) {
            Some(i) => i,
            None => {
                self.fills.push(k);
                self.fills.len() - 1
            }
        }
    }
    fn numfmt(&mut self, code: &str) -> u32 {
        if code.is_empty() {
            return 0;
        }
        let builtin = [("0", 1), ("0.00", 2), ("#,##0", 3), ("#,##0.00", 4), ("0%", 9), ("0.00%", 10), ("@", 49)];
        if let Some((_, i)) = builtin.iter().find(|(c, _)| *c == code) {
            return *i;
        }
        if let Some((_, i)) = self.numfmts.iter().find(|(c, _)| c == code) {
            return *i;
        }
        let id = 164 + self.numfmts.len() as u32;
        self.numfmts.push((code.to_string(), id));
        id
    }
    #[allow(clippy::too_many_arguments)] // 旧版の xf(color, bold, bg, fmt, border, align, wrap) と同じ並び
    fn xf(&mut self, color: &str, bold: bool, bg: &str, fmt: &str, border: u32, align: &str, wrap: bool) -> usize {
        let k = Xf { font: self.font(color, bold), fill: self.fill(bg), numfmt: self.numfmt(fmt), border, align: align.into(), wrap };
        match self.xfs.iter().position(|f| *f == k) {
            Some(i) => i,
            None => {
                self.xfs.push(k);
                self.xfs.len() - 1
            }
        }
    }
    fn xml(&self) -> String {
        let mut o = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">",
        );
        if !self.numfmts.is_empty() {
            o += &format!("<numFmts count=\"{}\">", self.numfmts.len());
            for (code, i) in &self.numfmts {
                o += &format!("<numFmt numFmtId=\"{i}\" formatCode=\"{}\"/>", x(code));
            }
            o += "</numFmts>";
        }
        o += &format!("<fonts count=\"{}\">", self.fonts.len());
        for (color, bold) in &self.fonts {
            o += "<font>";
            if *bold {
                o += "<b/>";
            }
            o += "<sz val=\"10\"/>";
            if !color.is_empty() {
                o += &format!("<color rgb=\"{color}\"/>");
            }
            o += "<name val=\"Meiryo UI\"/><family val=\"3\"/><charset val=\"128\"/></font>";
        }
        o += "</fonts>";
        o += &format!("<fills count=\"{}\">", self.fills.len());
        for f in &self.fills {
            o += &match f.as_str() {
                "" => "<fill><patternFill patternType=\"none\"/></fill>".to_string(),
                "gray125" => "<fill><patternFill patternType=\"gray125\"/></fill>".to_string(),
                c => {
                    format!("<fill><patternFill patternType=\"solid\"><fgColor rgb=\"{c}\"/><bgColor indexed=\"64\"/></patternFill></fill>")
                }
            };
        }
        o += "</fills>";
        let line = |t: &str| format!("<{t} style=\"thin\"><color rgb=\"FFE2E6EC\"/></{t}>");
        o += "<borders count=\"3\"><border><left/><right/><top/><bottom/><diagonal/></border><border>";
        for t in ["left", "right", "top", "bottom"] {
            o += &line(t);
        }
        o += "<diagonal/></border><border>";
        for t in ["left", "right", "top"] {
            o += &line(t);
        }
        o += "<bottom style=\"medium\"><color rgb=\"FF9AA5B1\"/></bottom><diagonal/></border></borders>";
        o += "<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>";
        o += &format!("<cellXfs count=\"{}\">", self.xfs.len());
        for f in &self.xfs {
            let a = if !f.align.is_empty() || f.wrap {
                format!(
                    "<alignment vertical=\"center\"{}{}/>",
                    if f.align.is_empty() { String::new() } else { format!(" horizontal=\"{}\"", f.align) },
                    if f.wrap { " wrapText=\"1\"" } else { "" }
                )
            } else {
                String::new()
            };
            o += &format!(
                "<xf numFmtId=\"{}\" fontId=\"{}\" fillId=\"{}\" borderId=\"{}\" xfId=\"0\"{}{}{}{}{}>{a}</xf>",
                f.numfmt,
                f.font,
                f.fill,
                f.border,
                if f.numfmt != 0 { " applyNumberFormat=\"1\"" } else { "" },
                if f.font != 0 { " applyFont=\"1\"" } else { "" },
                if f.fill != 0 { " applyFill=\"1\"" } else { "" },
                if f.border != 0 { " applyBorder=\"1\"" } else { "" },
                if a.is_empty() { "" } else { " applyAlignment=\"1\"" },
            );
        }
        o += "</cellXfs><cellStyles count=\"1\"><cellStyle name=\"標準\" xfId=\"0\" builtinId=\"0\"/></cellStyles></styleSheet>";
        o
    }
}

fn s(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(t) => t.clone(),
        v if !crate::pyfmt::truthy(v) => String::new(),
        v => crate::pyfmt::json_py_str(v),
    }
}

/// 数のセル（真偽は数ではない）。→ セルに書く字
fn number_text(v: &Value) -> Option<String> {
    let Value::Number(n) = v else { return None };
    if let Some(i) = n.as_i64() {
        return Some(i.to_string());
    }
    if let Some(u) = n.as_u64() {
        return Some(u.to_string());
    }
    n.as_f64().map(crate::pyfmt::py_float_repr)
}

/// XML の部品（名前, 中身）。作成日時は created。
pub fn parts(data: &Value, created: &str) -> Result<Vec<(&'static str, String)>, String> {
    let headers: Vec<String> = data["headers"].as_array().map(|a| a.iter().map(crate::pyfmt::json_py_str).collect()).unwrap_or_default();
    let ncol = headers.len();
    if ncol == 0 {
        return Err("列がありません".into());
    }
    let none = vec![];
    let rows = data["rows"].as_array().unwrap_or(&none);
    let widths = data["widths"].as_array().unwrap_or(&none);
    let aligns = data["aligns"].as_array().unwrap_or(&none);
    let name = sheet_name(&s(&data["sheet"]).chars().collect::<String>()).to_string();
    let name = if data["sheet"].is_null() || !crate::pyfmt::truthy(&data["sheet"]) { "Sheet1".to_string() } else { name };
    let mut st = Styles::new();
    let head_xf = st.xf("", true, "#EEF1F5", "", 2, "center", true);
    let last = col_letter(ncol - 1);
    let nrow = rows.len() + 1;

    let mut sd = String::from("<row r=\"1\" ht=\"20\" customHeight=\"1\">");
    for (ci, h) in headers.iter().enumerate() {
        sd += &format!("<c r=\"{}1\" s=\"{head_xf}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>", col_letter(ci), x(h));
    }
    sd += "</row>";
    let mut merges = vec![];
    for (i, row) in rows.iter().enumerate() {
        let ri = i + 2;
        let cells = row["cells"].as_array().unwrap_or(&none);
        let styles = &row["styles"];
        let (rbg, rcolor, rbold) = (s(&row["bg"]), s(&row["color"]), crate::pyfmt::truthy(&row["bold"]));
        if let Some(m) = row["merge"].as_array().filter(|m| m.len() == 2) {
            if let (Some(a), Some(b)) = (m[0].as_i64(), m[1].as_i64()) {
                if 0 <= a && a < b && (b as usize) < ncol {
                    merges.push(format!("{}{ri}:{}{ri}", col_letter(a as usize), col_letter(b as usize)));
                }
            }
        }
        sd += &format!("<row r=\"{ri}\">");
        for ci in 0..ncol {
            let c = cells.get(ci).filter(|c| c.is_object()).cloned().unwrap_or(Value::Object(Default::default()));
            let cs = styles.get(ci.to_string()).filter(|v| crate::pyfmt::truthy(v)).cloned().unwrap_or(Value::Object(Default::default()));
            let pick = |k: &str, row_v: &str| {
                let a = s(&cs[k]);
                if !a.is_empty() {
                    return a;
                }
                let b = s(&c[k]);
                if !b.is_empty() {
                    return b;
                }
                row_v.to_string()
            };
            let (bg, color) = (pick("bg", &rbg), pick("color", &rcolor));
            let bold = crate::pyfmt::truthy(&cs["bold"]) || crate::pyfmt::truthy(&c["bold"]) || rbold;
            let align = {
                let a = s(&c["align"]);
                let a = if a.is_empty() { aligns.get(ci).map(s).unwrap_or_default() } else { a };
                if matches!(a.as_str(), "left" | "center" | "right") {
                    a
                } else {
                    String::new()
                }
            };
            let r = format!("{}{ri}", col_letter(ci));
            if let Some(n) = number_text(&c["n"]) {
                let xf = st.xf(&color, bold, &bg, &s(&c["fmt"]), 1, &align, false);
                sd += &format!("<c r=\"{r}\" s=\"{xf}\"><v>{n}</v></c>");
            } else {
                let v = &c["v"];
                let blank = v.is_null() || v == "";
                let xf = st.xf(&color, bold, &bg, if blank { "" } else { "@" }, 1, &align, false);
                if blank {
                    sd += &format!("<c r=\"{r}\" s=\"{xf}\"/>");
                } else {
                    sd += &format!(
                        "<c r=\"{r}\" s=\"{xf}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
                        x(&crate::pyfmt::json_py_str(v))
                    );
                }
            }
        }
        sd += "</row>";
    }
    let mut cols = String::from("<cols>");
    for ci in 0..ncol {
        let w = match widths.get(ci) {
            None => 10.0,
            Some(v) => crate::pyfmt::py_float(v).filter(|f| !f.is_nan()).map(|f| f.clamp(3.0, 80.0)).unwrap_or(10.0),
        };
        cols += &format!("<col min=\"{}\" max=\"{}\" width=\"{w:.2}\" customWidth=\"1\"/>", ci + 1, ci + 1);
    }
    cols += "</cols>";
    let footer = s(&data["info"]);
    let mut sheet = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheetPr><pageSetUpPr fitToPage=\"1\"/></sheetPr>\
<dimension ref=\"A1:{last}{nrow}\"/><sheetViews><sheetView workbookViewId=\"0\" zoomScale=\"100\">\
<pane ySplit=\"1\" topLeftCell=\"A2\" activePane=\"bottomLeft\" state=\"frozen\"/>\
<selection pane=\"bottomLeft\" activeCell=\"A2\" sqref=\"A2\"/></sheetView></sheetViews><sheetFormatPr defaultRowHeight=\"15\"/>\
{cols}<sheetData>{sd}</sheetData><autoFilter ref=\"A1:{last}{nrow}\"/>"
    );
    if !merges.is_empty() {
        sheet += &format!(
            "<mergeCells count=\"{}\">{}</mergeCells>",
            merges.len(),
            merges.iter().map(|m| format!("<mergeCell ref=\"{m}\"/>")).collect::<String>()
        );
    }
    sheet += "<pageMargins left=\"0.4\" right=\"0.4\" top=\"0.5\" bottom=\"0.5\" header=\"0.3\" footer=\"0.3\"/>";
    sheet += "<pageSetup paperSize=\"9\" orientation=\"landscape\" fitToWidth=\"1\" fitToHeight=\"0\"/>";
    if !footer.is_empty() {
        sheet +=
            &format!("<headerFooter><oddFooter>&amp;L{}&amp;R&amp;P / &amp;N</oddFooter></headerFooter>", x(&footer.replace('&', "&&")));
    }
    sheet += "</worksheet>";
    let qname = format!("'{}'", name.replace('\'', "''"));
    let workbook = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><bookViews><workbookView/></bookViews>\
<sheets><sheet name=\"{}\" sheetId=\"1\" r:id=\"rId1\"/></sheets><definedNames>\
<definedName name=\"_xlnm._FilterDatabase\" localSheetId=\"0\" hidden=\"1\">{}!$A$1:${last}${nrow}</definedName>\
<definedName name=\"_xlnm.Print_Titles\" localSheetId=\"0\">{}!$1:$1</definedName></definedNames></workbook>",
        x(&name),
        x(&qname),
        x(&qname)
    );
    Ok(vec![
        ("[Content_Types].xml", "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>\
<Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>\
<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>\
<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>".into()),
        ("_rels/.rels", "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/>\
<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>".into()),
        ("docProps/core.xml", format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\
<dc:title>CM3 スケジュール {}</dc:title><dc:creator>CM3 Schedule Viewer</dc:creator><dcterms:created xsi:type=\"dcterms:W3CDTF\">{created}</dcterms:created></cp:coreProperties>", x(&name))),
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/>\
<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>".into()),
        ("xl/worksheets/sheet1.xml", sheet),
        ("xl/styles.xml", st.xml()),
    ])
}

/// .xlsx のバイト列。
pub fn build(data: &Value) -> Result<Vec<u8>, String> {
    let created = chrono::Local::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, text) in parts(data, &created)? {
            z.start_file(name, opt).map_err(|e| e.to_string())?;
            z.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        }
        z.finish().map_err(|e| e.to_string())?;
    }
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_and_names() {
        assert_eq!(
            (col_letter(0), col_letter(25), col_letter(26), col_letter(701), col_letter(702)),
            ("A".into(), "Z".into(), "AA".into(), "ZZ".into(), "AAA".into())
        );
        assert_eq!(sheet_name("L-3:[試]/?*"), "L-3__試____");
        assert_eq!(sheet_name("''"), "Sheet1");
    }
}
