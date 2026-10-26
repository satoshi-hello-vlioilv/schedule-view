/*
 * Column pipeline used by the table and the column designer.
 *
 *   元の値 (schedule field / master column / formula)
 *     -> 読み替え (conversion table)
 *     -> 値の整え方 (number / date / text format, prefix, suffix)
 *     -> 表示
 *
 * Rules (conditional colors) compare the value AFTER 読み替え.
 */
(function () {
  "use strict";
  const X = window.CM3Expr;

  const OPS = [
    { v: "eq", label: "と同じ" },
    { v: "ne", label: "と違う" },
    { v: "contains", label: "を含む" },
    { v: "notcontains", label: "を含まない" },
    { v: "starts", label: "で始まる" },
    { v: "in", label: "のどれか（, 区切り）" },
    { v: "gt", label: "より大きい" },
    { v: "ge", label: "以上" },
    { v: "lt", label: "より小さい" },
    { v: "le", label: "以下" },
    { v: "blank", label: "が空欄" },
    { v: "notblank", label: "が空欄でない" }
  ];
  const DATE_FORMATS = ["yyyy/MM/dd", "yy/MM/dd", "MM/dd", "M/d", "MM/dd HH:mm", "yyyy/MM/dd HH:mm", "HH:mm"];
  const NUMERIC_FIELDS = new Set(["seq", "passNo", "inItaAtsu", "outItaAtsu", "reductionRate",
    "reienHaba", "seihinItaAtsu", "seihinItaHaba", "hotSeq"]);
  const ISO_SORT = { kanryoYotei: "kanryoYoteiIso", startYotei: "startYoteiIso" };

  function normName(s) {
    return String(s || "").normalize("NFKC").toUpperCase().replace(/[\s_\-.・]/g, "");
  }
  function normVal(s) {
    return X.str(s).normalize("NFKC").trim();
  }
  function isNumLike(v) {
    return typeof v === "number" || /^[+-]?(\d+\.?\d*|\.\d+)$/.test(X.str(v).trim());
  }

  function schedLabel(field) {
    const f = (window.APP.scheduleColumns || []).find((x) => x.key === field);
    return f ? f.label : field;
  }

  /** default name of a column (before the user's 表示名) */
  function baseName(c) {
    if (c.source === "schedule") return schedLabel(c.field);
    if (c.source === "master") return String(c.field || "").normalize("NFKC");
    return c.label || "計算列";
  }
  function title(c) { return (c.label && c.label.trim()) || baseName(c); }

  // ------------------------------------------------------------ build

  /**
   * build(settings, masterAll) -> view
   * view.cols: columns with compiled formula / conversion table
   * view.rules: compiled rules
   */
  function build(settings, masterAll) {
    const cols = (settings.columns || []).map((c) => Object.assign({}, c));
    const byId = {};
    cols.forEach((c) => { byId[c.id] = c; });
    const masterSet = new Set(masterAll || []);
    const masterByNorm = {};
    (masterAll || []).forEach((m) => { masterByNorm[normName(m)] = m; });

    function resolve(name) {
      const n = normName(name);
      for (const c of cols) {
        if (c.source === "calc" && normName(title(c)) === n) return { k: "c", id: c.id };
      }
      for (const f of window.APP.scheduleColumns) {
        if (normName(f.label) === n || normName(f.key) === n) return { k: "s", field: f.key };
      }
      for (const c of cols) {   // a schedule / master column renamed by the user
        if (c.source !== "calc" && c.label && normName(c.label) === n) {
          return c.source === "schedule" ? { k: "s", field: c.field } : { k: "m", col: c.field };
        }
      }
      if (masterSet.has(name)) return { k: "m", col: name };
      if (masterByNorm[n]) return { k: "m", col: masterByNorm[n] };
      return null;
    }

    const maps = settings.maps || {};
    cols.forEach((c) => {
      c._title = title(c);
      if (c.source === "calc") c._cmp = X.compile(c.expr || "", resolve);
      const mp = c.map && maps[c.map];
      if (mp) {
        const lk = new Map();
        (mp.entries || []).forEach((e) => { if (e && e[0] != null) lk.set(normVal(e[0]), X.str(e[1])); });
        c._map = { lk: lk, els: mp.else || "keep", elseText: mp.elseText || "" };
      }
      c._numeric = c.source === "schedule" && NUMERIC_FIELDS.has(c.field);
    });

    // formulas that refer to themselves (directly or through other formulas)
    const deps = {};
    cols.forEach((c) => {
      if (c.source !== "calc" || !c._cmp || !c._cmp.ok) return;
      deps[c.id] = c._cmp.refs.map(resolve).filter((r) => r && r.k === "c").map((r) => r.id);
    });
    const state = {};
    function visit(id) {
      if (state[id] === 1) return true;
      if (state[id] === 2) return false;
      state[id] = 1;
      const bad = (deps[id] || []).some(visit);
      state[id] = 2;
      return bad;
    }
    cols.forEach((c) => {
      Object.keys(state).forEach((k) => { delete state[k]; });
      if (deps[c.id] && visit(c.id)) c._cmp = { ok: false, error: "式が自分自身を参照しています（循環）", refs: c._cmp.refs };
    });

    const rules = (settings.rules || []).map((r) => {
      const o = Object.assign({}, r);
      if (r.when === "expr") o._cmp = X.compile(r.expr || "", resolve);
      return o;
    });

    return { settings: settings, cols: cols, byId: byId, rules: rules, resolve: resolve };
  }

  /** a schedule column that is not in the settings (used by special lists) */
  function extraCol(field, label) {
    return { id: "s:" + field, source: "schedule", field: field, label: label || "", visible: true,
      width: null, align: "auto", bold: false, format: { type: "none" }, map: "",
      _title: label || schedLabel(field), _numeric: NUMERIC_FIELDS.has(field) };
  }

  // ------------------------------------------------------------ values

  function rawValue(view, col, item, memo, depth) {
    if (col.source === "schedule") {
      const v = item[col.field];
      return v == null ? "" : v;
    }
    if (col.source === "master") {
      const m = item.m;
      return m && m[col.field] != null ? m[col.field] : "";
    }
    // formula
    if (memo.has(col.id)) return memo.get(col.id);
    if (!col._cmp || !col._cmp.ok || (depth || 0) > 20) return "";
    memo.set(col.id, "");          // guards against cycles
    let v = "";
    try {
      v = X.evaluate(col._cmp.ast, (r) => refValue(view, r, item, memo, (depth || 0) + 1));
    } catch (e) { v = ""; }
    memo.set(col.id, v);
    return v;
  }

  function refValue(view, r, item, memo, depth) {
    if (r.k === "s") { const v = item[r.field]; return v == null ? "" : v; }
    if (r.k === "m") { const m = item.m; return m && m[r.col] != null ? m[r.col] : ""; }
    const c = view.byId[r.id];
    return c ? rawValue(view, c, item, memo, depth) : "";
  }

  function mapValue(col, raw) {
    if (!col._map) return raw;
    const k = normVal(raw);
    if (col._map.lk.has(k)) return col._map.lk.get(k);
    if (col._map.els === "blank") return "";
    if (col._map.els === "text") return col._map.elseText;
    return raw;
  }

  function formatValue(v, f) {
    f = f || {};
    let s;
    const type = f.type || "none";
    if (type === "number") {
      const n = X.num(v);
      if (isNaN(n)) s = X.str(v);
      else if (n === 0 && f.zeroBlank) s = "";
      else {
        const d = f.decimals == null || f.decimals === "" ? null : Math.max(0, Math.min(6, +f.decimals));
        s = d == null ? X.str(n) : n.toFixed(d);
        if (f.comma) {
          const parts = s.split(".");
          parts[0] = parts[0].replace(/\B(?=(\d{3})+(?!\d))/g, ",");
          s = parts.join(".");
        }
      }
    } else if (type === "date") {
      const d = X.parseDate(v);
      s = d ? X.fmtDate(d, f.dateFmt || "yyyy/MM/dd") : X.str(v);
    } else {
      s = X.str(v);
      if (type === "text") {
        if (f.zen) s = s.normalize("NFKC");
        if (f.trim !== false) s = s.trim();
        if (f.maxLen > 0 && s.length > f.maxLen) s = s.slice(0, f.maxLen) + "…";
      }
    }
    if (s !== "" && (f.prefix || f.suffix)) s = (f.prefix || "") + s + (f.suffix || "");
    return s;
  }

  /** cell(view, col, item, memo) -> { raw, val (after 読み替え), text (shown) } */
  function cell(view, col, item, memo) {
    const raw = rawValue(view, col, item, memo, 0);
    const val = mapValue(col, raw);
    return { raw: raw, val: val, text: formatValue(val, col.format) };
  }

  function sortValue(view, col, item, memo) {
    if (col.source === "schedule" && ISO_SORT[col.field]) return item[ISO_SORT[col.field]] || null;
    const v = cell(view, col, item, memo).val;
    if (X.blank(v)) return null;
    const n = X.num(v);
    return isNaN(n) ? X.str(v) : n;
  }

  // ------------------------------------------------------------ rules

  function testOp(op, v, target) {
    const s = X.str(v), t = X.str(target);
    switch (op) {
      case "eq": return X.compare(v, target) === 0;
      case "ne": return X.compare(v, target) !== 0;
      case "contains": return t !== "" && s.includes(t);
      case "notcontains": return t === "" || !s.includes(t);
      case "starts": return t !== "" && s.startsWith(t);
      case "in": return t.split(/[,、，]/).map((x) => x.trim()).filter((x) => x !== "").some((x) => X.compare(v, x) === 0);
      case "gt": return !X.blank(v) && X.compare(v, target) > 0;
      case "ge": return !X.blank(v) && X.compare(v, target) >= 0;
      case "lt": return !X.blank(v) && X.compare(v, target) < 0;
      case "le": return !X.blank(v) && X.compare(v, target) <= 0;
      case "blank": return X.blank(v);
      case "notblank": return !X.blank(v);
    }
    return false;
  }

  function ruleMatch(view, r, item, memo) {
    if (r.enabled === false) return false;
    if (r.when === "expr") {
      if (!r._cmp || !r._cmp.ok) return false;
      try {
        return X.truthy(X.evaluate(r._cmp.ast, (ref) => refValue(view, ref, item, memo, 0)));
      } catch (e) { return false; }
    }
    const c = view.byId[r.col];
    if (!c) return false;
    return testOp(r.op || "eq", cell(view, c, item, memo).val, r.value);
  }

  /** returns { row: style, cells: {colId: style} } ; later rules win */
  function applyRules(view, item, memo) {
    const out = { row: null, cells: {} };
    view.rules.forEach((r) => {
      if (!ruleMatch(view, r, item, memo)) return;
      const st = { bg: r.bg || "", color: r.color || "", bold: !!r.bold };
      if (r.target === "cell") {
        const id = r.cellCol || r.col;
        out.cells[id] = Object.assign({}, out.cells[id] || {}, strip(st));
      } else {
        out.row = Object.assign({}, out.row || {}, strip(st));
      }
    });
    return out;
  }
  function strip(st) {
    const o = {};
    if (st.bg) o.bg = st.bg;
    if (st.color) o.color = st.color;
    if (st.bold) o.bold = true;
    return o;
  }
  function styleText(st) {
    if (!st) return "";
    return (st.bg ? "background:" + st.bg + ";" : "") + (st.color ? "color:" + st.color + ";" : "") +
      (st.bold ? "font-weight:700;" : "");
  }

  window.CM3View = {
    OPS: OPS,
    DATE_FORMATS: DATE_FORMATS,
    build: build,
    extraCol: extraCol,
    cell: cell,
    rawValue: rawValue,
    formatValue: formatValue,
    sortValue: sortValue,
    ruleMatch: ruleMatch,
    applyRules: applyRules,
    styleText: styleText,
    baseName: baseName,
    title: title,
    normName: normName,
    normVal: normVal,
    isNumLike: isNumLike
  };
})();
