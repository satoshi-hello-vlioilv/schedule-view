# -*- coding: utf-8 -*-
"""
Minimal .xlsx writer (standard library only: zipfile + XML text).

Input (built by the screen, "as shown"):
{
  "sheet":   "L-3",
  "headers": ["No", "完了予定", ...],
  "widths":  [6, 12, ...],                 # Excel column width (characters)
  "aligns":  ["right", "left", ...],
  "rows": [
    {"cells": [{"v": "L2033J0"} | {"n": 4.6, "fmt": "0.0"} | {} , ...],
     "bg": "#fff4c2", "color": "#6b4e00", "bold": false,      # row style
     "styles": {"3": {"bg": "...", "color": "...", "bold": true}},  # cell overrides (col index)
     "merge": [2, 13]}                                         # merge from col 2 to col 13 (0-based)
  ],
  "info": "CM3 L-3  取得 2026/10/19 14:37"   # optional, written in the sheet footer
}
A cell may also carry "bg", "color", "bold", "align".
"""
import io
import re
import zipfile
from datetime import datetime
from xml.sax.saxutils import escape

_BAD = re.compile("[\x00-\x08\x0b\x0c\x0e-\x1f\ufffe\uffff]")
_HEX = re.compile(r"^#?([0-9a-fA-F]{6})$")
_SHEET_BAD = re.compile(r"[\[\]:*?/\\]")


def _x(s):
    return escape(_BAD.sub("", str(s)), {'"': "&quot;"})


def _argb(c):
    m = _HEX.match(str(c or ""))
    return ("FF" + m.group(1).upper()) if m else None


def col_letter(i):
    s = ""
    i += 1
    while i:
        i, r = divmod(i - 1, 26)
        s = chr(65 + r) + s
    return s


def sheet_name(name):
    n = _SHEET_BAD.sub("_", str(name or "Sheet1")).strip("'")[:31]
    return n or "Sheet1"


class _Styles:
    """Registry of fonts / fills / number formats / cellXfs."""

    def __init__(self):
        self.fonts = [("", False)]                  # (color, bold)
        self.fills = [None, "gray125"]              # required defaults
        self.numfmts = {}                           # code -> id
        self.xfs = [(0, 0, 0, 0, "", False)]        # (font, fill, numfmt, border, align, wrap)
        self._xf_index = {self.xfs[0]: 0}

    def _font(self, color, bold):
        k = (_argb(color) or "", bool(bold))
        if k not in self.fonts:
            self.fonts.append(k)
        return self.fonts.index(k)

    def _fill(self, bg):
        k = _argb(bg)
        if not k:
            return 0
        if k not in self.fills:
            self.fills.append(k)
        return self.fills.index(k)

    def _numfmt(self, code):
        if not code:
            return 0
        builtin = {"0": 1, "0.00": 2, "#,##0": 3, "#,##0.00": 4, "0%": 9, "0.00%": 10, "@": 49}
        if code in builtin:
            return builtin[code]
        if code not in self.numfmts:
            self.numfmts[code] = 164 + len(self.numfmts)
        return self.numfmts[code]

    def xf(self, color="", bold=False, bg="", fmt="", border=1, align="", wrap=False):
        k = (self._font(color, bold), self._fill(bg), self._numfmt(fmt), border, align or "", bool(wrap))
        if k not in self._xf_index:
            self._xf_index[k] = len(self.xfs)
            self.xfs.append(k)
        return self._xf_index[k]

    def xml(self):
        out = ['<?xml version="1.0" encoding="UTF-8" standalone="yes"?>',
               '<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">']
        if self.numfmts:
            out.append('<numFmts count="%d">' % len(self.numfmts))
            for code, i in self.numfmts.items():
                out.append('<numFmt numFmtId="%d" formatCode="%s"/>' % (i, _x(code)))
            out.append("</numFmts>")
        out.append('<fonts count="%d">' % len(self.fonts))
        for color, bold in self.fonts:
            out.append("<font>" + ("<b/>" if bold else "") + '<sz val="10"/>' +
                       ('<color rgb="%s"/>' % color if color else "") +
                       '<name val="Meiryo UI"/><family val="3"/><charset val="128"/></font>')
        out.append("</fonts>")
        out.append('<fills count="%d">' % len(self.fills))
        for f in self.fills:
            if f is None:
                out.append('<fill><patternFill patternType="none"/></fill>')
            elif f == "gray125":
                out.append('<fill><patternFill patternType="gray125"/></fill>')
            else:
                out.append('<fill><patternFill patternType="solid"><fgColor rgb="%s"/><bgColor indexed="64"/></patternFill></fill>' % f)
        out.append("</fills>")
        line = '<%s style="thin"><color rgb="FFE2E6EC"/></%s>'
        out.append('<borders count="3">'
                   '<border><left/><right/><top/><bottom/><diagonal/></border>'
                   '<border>' + "".join(line % (t, t) for t in ("left", "right", "top", "bottom")) + '<diagonal/></border>'
                   '<border>' + "".join(line % (t, t) for t in ("left", "right", "top")) +
                   '<bottom style="medium"><color rgb="FF9AA5B1"/></bottom><diagonal/></border>'
                   "</borders>")
        out.append('<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>')
        out.append('<cellXfs count="%d">' % len(self.xfs))
        for font, fill, nf, border, align, wrap in self.xfs:
            a = ""
            if align or wrap:
                a = '<alignment vertical="center"' + (' horizontal="%s"' % align if align else "") + \
                    (' wrapText="1"' if wrap else "") + "/>"
            out.append('<xf numFmtId="%d" fontId="%d" fillId="%d" borderId="%d" xfId="0"%s%s%s%s%s>%s</xf>' % (
                nf, font, fill, border,
                ' applyNumberFormat="1"' if nf else "", ' applyFont="1"' if font else "",
                ' applyFill="1"' if fill else "", ' applyBorder="1"' if border else "",
                ' applyAlignment="1"' if a else "", a))
        out.append("</cellXfs>")
        out.append('<cellStyles count="1"><cellStyle name="標準" xfId="0" builtinId="0"/></cellStyles>')
        out.append("</styleSheet>")
        return "".join(out)


def build_xlsx(data):
    headers = [str(h) for h in data.get("headers") or []]
    ncol = len(headers)
    if not ncol:
        raise ValueError("列がありません")
    rows = data.get("rows") or []
    widths = list(data.get("widths") or [])
    aligns = list(data.get("aligns") or [])
    name = sheet_name(data.get("sheet"))
    st = _Styles()

    head_xf = st.xf(bold=True, bg="#EEF1F5", border=2, align="center", wrap=True)
    last = col_letter(ncol - 1)
    nrow = len(rows) + 1

    sd = ['<row r="1" ht="20" customHeight="1">']
    for ci, h in enumerate(headers):
        sd.append('<c r="%s1" s="%d" t="inlineStr"><is><t xml:space="preserve">%s</t></is></c>' % (col_letter(ci), head_xf, _x(h)))
    sd.append("</row>")

    merges = []
    for ri, row in enumerate(rows, start=2):
        cells = row.get("cells") or []
        styles = row.get("styles") or {}
        rbg, rcolor, rbold = row.get("bg") or "", row.get("color") or "", bool(row.get("bold"))
        merge = row.get("merge")
        if merge and len(merge) == 2 and 0 <= merge[0] < merge[1] < ncol:
            merges.append("%s%d:%s%d" % (col_letter(merge[0]), ri, col_letter(merge[1]), ri))
        sd.append('<row r="%d">' % ri)
        for ci in range(ncol):
            c = cells[ci] if ci < len(cells) and isinstance(cells[ci], dict) else {}
            cs = styles.get(str(ci)) or {}
            bg = cs.get("bg") or c.get("bg") or rbg
            color = cs.get("color") or c.get("color") or rcolor
            bold = bool(cs.get("bold") or c.get("bold") or rbold)
            align = c.get("align") or (aligns[ci] if ci < len(aligns) else "")
            align = align if align in ("left", "center", "right") else ""
            ref = "%s%d" % (col_letter(ci), ri)
            if isinstance(c.get("n"), (int, float)) and not isinstance(c.get("n"), bool):
                s = st.xf(color, bold, bg, c.get("fmt") or "", 1, align)
                sd.append('<c r="%s" s="%d"><v>%s</v></c>' % (ref, s, repr(float(c["n"])) if isinstance(c["n"], float) else c["n"]))
            else:
                v = c.get("v")
                s = st.xf(color, bold, bg, "@" if v not in (None, "") else "", 1, align)
                if v in (None, ""):
                    sd.append('<c r="%s" s="%d"/>' % (ref, s))
                else:
                    sd.append('<c r="%s" s="%d" t="inlineStr"><is><t xml:space="preserve">%s</t></is></c>' % (ref, s, _x(v)))
        sd.append("</row>")

    cols = ["<cols>"]
    for ci in range(ncol):
        w = widths[ci] if ci < len(widths) else 10
        try:
            w = max(3.0, min(80.0, float(w)))
        except (TypeError, ValueError):
            w = 10.0
        cols.append('<col min="%d" max="%d" width="%.2f" customWidth="1"/>' % (ci + 1, ci + 1, w))
    cols.append("</cols>")

    footer = data.get("info") or ""
    sheet = ['<?xml version="1.0" encoding="UTF-8" standalone="yes"?>',
             '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
             'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">',
             '<sheetPr><pageSetUpPr fitToPage="1"/></sheetPr>',
             '<dimension ref="A1:%s%d"/>' % (last, nrow),
             '<sheetViews><sheetView workbookViewId="0" zoomScale="100">'
             '<pane ySplit="1" topLeftCell="A2" activePane="bottomLeft" state="frozen"/>'
             '<selection pane="bottomLeft" activeCell="A2" sqref="A2"/></sheetView></sheetViews>',
             '<sheetFormatPr defaultRowHeight="15"/>',
             "".join(cols),
             "<sheetData>", "".join(sd), "</sheetData>",
             '<autoFilter ref="A1:%s%d"/>' % (last, nrow)]
    if merges:
        sheet.append('<mergeCells count="%d">%s</mergeCells>' % (len(merges), "".join('<mergeCell ref="%s"/>' % m for m in merges)))
    sheet.append('<pageMargins left="0.4" right="0.4" top="0.5" bottom="0.5" header="0.3" footer="0.3"/>')
    sheet.append('<pageSetup paperSize="9" orientation="landscape" fitToWidth="1" fitToHeight="0"/>')
    if footer:
        sheet.append('<headerFooter><oddFooter>&amp;L%s&amp;R&amp;P / &amp;N</oddFooter></headerFooter>' % _x(footer.replace("&", "&&")))
    sheet.append("</worksheet>")

    qname = "'%s'" % name.replace("'", "''")
    workbook = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
                '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
                'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
                '<bookViews><workbookView/></bookViews>'
                '<sheets><sheet name="%s" sheetId="1" r:id="rId1"/></sheets>'
                '<definedNames>'
                '<definedName name="_xlnm._FilterDatabase" localSheetId="0" hidden="1">%s!$A$1:$%s$%d</definedName>'
                '<definedName name="_xlnm.Print_Titles" localSheetId="0">%s!$1:$1</definedName>'
                '</definedNames></workbook>') % (_x(name), _x(qname), last, nrow, _x(qname))

    now = datetime.now().strftime("%Y-%m-%dT%H:%M:%SZ")
    files = {
        "[Content_Types].xml":
            '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
            '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
            '<Default Extension="xml" ContentType="application/xml"/>'
            '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>'
            '<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
            '<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>'
            '<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>'
            '</Types>',
        "_rels/.rels":
            '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>'
            '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>'
            '</Relationships>',
        "docProps/core.xml":
            '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" '
            'xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" '
            'xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">'
            '<dc:title>CM3 スケジュール %s</dc:title><dc:creator>CM3 Schedule Viewer</dc:creator>'
            '<dcterms:created xsi:type="dcterms:W3CDTF">%s</dcterms:created>'
            '</cp:coreProperties>' % (_x(name), now),
        "xl/workbook.xml": workbook,
        "xl/_rels/workbook.xml.rels":
            '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>'
            '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>'
            '</Relationships>',
        "xl/worksheets/sheet1.xml": "".join(sheet),
        "xl/styles.xml": st.xml(),
    }
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        for n, t in files.items():
            z.writestr(n, t.encode("utf-8"))
    return buf.getvalue()
