/*
 * Small formula language for calculated columns and rules (no eval).
 *
 *   [列名]            column reference
 *   "文字"  123  1.5  literals (use "" inside a string for a quote)
 *   + - * /           arithmetic (numbers)
 *   &                 join text
 *   = <> != < <= > >= comparison
 *   func(a, b, ...)   functions (see FUNCS)
 */
(function () {
  "use strict";

  // full-width punctuation typed by Japanese IME -> ASCII (outside strings only)
  const FW = {
    "［": "[", "］": "]", "（": "(", "）": ")", "，": ",", "、": ",", "＆": "&", "＋": "+", "－": "-",
    "＊": "*", "／": "/", "＝": "=", "＜": "<", "＞": ">", "！": "!", "“": "\"", "”": "\"", "＂": "\"", "　": " "
  };

  function tokenize(src) {
    const out = [];
    let i = 0;
    const s = String(src || "");
    while (i < s.length) {
      let c = FW[s[i]] || s[i];
      if (/\s/.test(c)) { i++; continue; }
      if (c === "\"") {
        let j = i + 1, v = "";
        for (;;) {
          if (j >= s.length) throw new Error("文字の \" が閉じていません");
          const d = FW[s[j]] === "\"" ? "\"" : s[j];
          if (d === "\"") {
            const n = FW[s[j + 1]] === "\"" ? "\"" : s[j + 1];
            if (n === "\"") { v += "\""; j += 2; continue; }
            break;
          }
          v += s[j]; j++;
        }
        out.push({ t: "str", v: v });
        i = j + 1;
        continue;
      }
      if (c === "[") {
        let j = i + 1, v = "";
        while (j < s.length && (FW[s[j]] || s[j]) !== "]") { v += s[j]; j++; }
        if (j >= s.length) throw new Error("列名の [ が閉じていません");
        out.push({ t: "ref", v: v.trim() });
        i = j + 1;
        continue;
      }
      if (/[0-9.]/.test(c) || /[０-９]/.test(s[i])) {
        let j = i, v = "";
        while (j < s.length && /[0-9.０-９．]/.test(s[j])) { v += s[j].normalize("NFKC"); j++; }
        if (!/^\d*\.?\d+$|^\d+\.$/.test(v)) throw new Error("数値が不正です: " + v);
        out.push({ t: "num", v: parseFloat(v) });
        i = j;
        continue;
      }
      const two = c + (FW[s[i + 1]] || s[i + 1] || "");
      if (["<=", ">=", "<>", "!="].includes(two)) { out.push({ t: "op", v: two === "!=" ? "<>" : two }); i += 2; continue; }
      if ("+-*/&=<>(),".includes(c)) { out.push({ t: "op", v: c }); i++; continue; }
      if (/[A-Za-z_]/.test(c)) {
        let j = i, v = "";
        while (j < s.length && /[A-Za-z_0-9]/.test(s[j])) { v += s[j]; j++; }
        out.push({ t: "id", v: v.toLowerCase() });
        i = j;
        continue;
      }
      throw new Error("使えない文字です: " + s[i]);
    }
    return out;
  }

  function parse(tokens, resolve, refs) {
    let p = 0;
    const peek = () => tokens[p];
    const isOp = (v) => peek() && peek().t === "op" && peek().v === v;
    const expect = (v) => {
      if (!isOp(v)) throw new Error("「" + v + "」が必要です");
      p++;
    };

    function cmp() {
      let a = cat();
      while (peek() && peek().t === "op" && ["=", "<>", "<", "<=", ">", ">="].includes(peek().v)) {
        const op = tokens[p++].v;
        a = { k: "bin", op: op, a: a, b: cat() };
      }
      return a;
    }
    function cat() {
      let a = add();
      while (isOp("&")) { p++; a = { k: "bin", op: "&", a: a, b: add() }; }
      return a;
    }
    function add() {
      let a = mul();
      while (isOp("+") || isOp("-")) { const op = tokens[p++].v; a = { k: "bin", op: op, a: a, b: mul() }; }
      return a;
    }
    function mul() {
      let a = unary();
      while (isOp("*") || isOp("/")) { const op = tokens[p++].v; a = { k: "bin", op: op, a: a, b: unary() }; }
      return a;
    }
    function unary() {
      if (isOp("-")) { p++; return { k: "neg", a: unary() }; }
      if (isOp("+")) { p++; return unary(); }
      return primary();
    }
    function primary() {
      const t = peek();
      if (!t) throw new Error("式が途中で終わっています");
      p++;
      if (t.t === "num" || t.t === "str") return { k: "lit", v: t.v };
      if (t.t === "ref") {
        const r = resolve(t.v);
        if (!r) throw new Error("列が見つかりません: [" + t.v + "]");
        refs.push(t.v);
        return { k: "ref", name: t.v, r: r };
      }
      if (t.t === "id") {
        if (t.v === "true") return { k: "lit", v: true };
        if (t.v === "false") return { k: "lit", v: false };
        if (!FUNCS[t.v]) throw new Error("関数がありません: " + t.v);
        expect("(");
        const args = [];
        if (!isOp(")")) {
          args.push(cmp());
          while (isOp(",")) { p++; args.push(cmp()); }
        }
        expect(")");
        const f = FUNCS[t.v];
        if (args.length < f.min || (f.max != null && args.length > f.max)) {
          throw new Error(t.v + " の引数の数が違います（" + f.syntax + "）");
        }
        return { k: "call", f: t.v, args: args };
      }
      if (t.t === "op" && t.v === "(") {
        const e = cmp();
        expect(")");
        return e;
      }
      throw new Error("ここに「" + t.v + "」は書けません");
    }

    const ast = cmp();
    if (p < tokens.length) throw new Error("余分な記述があります: " + (tokens[p].v));
    return ast;
  }

  // ------------------------------------------------------------ values
  function str(v) {
    if (v == null) return "";
    if (v === true) return "TRUE";
    if (v === false) return "FALSE";
    if (typeof v === "number") {
      if (!isFinite(v)) return "";
      return String(Number(v.toPrecision(12)));
    }
    return String(v);
  }
  function num(v) {
    if (typeof v === "number") return v;
    if (v === true) return 1;
    if (v === false) return 0;
    const s = str(v).trim().replace(/,/g, "");
    if (s === "") return NaN;
    return /^[+-]?(\d+\.?\d*|\.\d+)$/.test(s) ? parseFloat(s) : NaN;
  }
  function truthy(v) {
    if (v === true) return true;
    if (v === false || v == null) return false;
    if (typeof v === "number") return v !== 0;
    const s = str(v).trim().toUpperCase();
    return s !== "" && s !== "0" && s !== "FALSE";
  }
  function blank(v) { return str(v).trim() === ""; }
  function compare(a, b) {
    const x = num(a), y = num(b);
    if (!isNaN(x) && !isNaN(y)) return x < y ? -1 : x > y ? 1 : 0;
    const s = str(a), t = str(b);
    return s < t ? -1 : s > t ? 1 : 0;
  }

  function parseDate(v) {
    if (v instanceof Date) return v;
    const s = str(v).trim();
    let m = s.match(/^(\d{4})[\/\-.](\d{1,2})[\/\-.](\d{1,2})(?:[ T](\d{1,2}):(\d{2})(?::(\d{2}))?)?/);
    if (m) return new Date(+m[1], +m[2] - 1, +m[3], +(m[4] || 0), +(m[5] || 0), +(m[6] || 0));
    m = s.match(/^(\d{4})(\d{2})(\d{2})$/);
    if (m) return new Date(+m[1], +m[2] - 1, +m[3]);
    m = s.match(/^(\d{1,2})\/(\d{1,2})(?:\s+(\d{1,2}):(\d{2}))?$/);
    if (m) return new Date(new Date().getFullYear(), +m[1] - 1, +m[2], +(m[3] || 0), +(m[4] || 0));
    return null;
  }
  function fmtDate(d, fmt) {
    const p2 = (n) => String(n).padStart(2, "0");
    return fmt.replace(/yyyy|yy|MM|dd|HH|mm|M|d/g, (t) => ({
      yyyy: d.getFullYear(), yy: p2(d.getFullYear() % 100), MM: p2(d.getMonth() + 1), dd: p2(d.getDate()),
      HH: p2(d.getHours()), mm: p2(d.getMinutes()), M: d.getMonth() + 1, d: d.getDate()
    })[t]);
  }
  function splitParts(t, sep) {
    const s = str(t);
    return sep === " " ? s.trim().split(/\s+/).filter((x) => x !== "") : s.split(sep);
  }

  // ------------------------------------------------------------ functions
  const FUNCS = {
    "if": { min: 2, max: 3, lazy: true, syntax: "if(条件, 真のとき, 偽のとき)", desc: "条件で値を切り替える",
      fn: (a, ev) => truthy(ev(a[0])) ? ev(a[1]) : (a.length > 2 ? ev(a[2]) : "") },
    "and": { min: 1, max: null, syntax: "and(条件1, 条件2, …)", desc: "すべて成り立つ", fn: (a) => a.every(truthy) },
    "or": { min: 1, max: null, syntax: "or(条件1, 条件2, …)", desc: "どれかが成り立つ", fn: (a) => a.some(truthy) },
    "not": { min: 1, max: 1, syntax: "not(条件)", desc: "成り立たない", fn: (a) => !truthy(a[0]) },
    "in": { min: 2, max: null, syntax: "in(値, 候補1, 候補2, …)", desc: "候補のどれかと同じ",
      fn: (a) => a.slice(1).some((x) => compare(a[0], x) === 0) },
    "isblank": { min: 1, max: 1, syntax: "isblank(値)", desc: "空欄か", fn: (a) => blank(a[0]) },
    "coalesce": { min: 1, max: null, syntax: "coalesce(値1, 値2, …)", desc: "最初の空欄でない値",
      fn: (a) => { const v = a.find((x) => !blank(x)); return v == null ? "" : v; } },

    "split": { min: 2, max: 3, syntax: "split(文字, 区切り, 番号)", desc: "区切って番号目を取り出す（-1で最後）",
      fn: (a) => {
        const parts = splitParts(a[0], str(a[1]));
        let n = a.length > 2 ? Math.trunc(num(a[2])) : 1;
        if (isNaN(n) || n === 0) n = 1;
        const v = n > 0 ? parts[n - 1] : parts[parts.length + n];
        return v == null ? "" : v;
      } },
    "count": { min: 1, max: 2, syntax: "count(文字, 区切り)", desc: "区切った個数（区切り省略時は空白）",
      fn: (a) => splitParts(a[0], a.length > 1 ? str(a[1]) : " ").filter((x) => x !== "").length },
    "left": { min: 2, max: 2, syntax: "left(文字, 文字数)", desc: "左から取り出す", fn: (a) => str(a[0]).slice(0, Math.max(0, num(a[1]) || 0)) },
    "right": { min: 2, max: 2, syntax: "right(文字, 文字数)", desc: "右から取り出す",
      fn: (a) => { const n = Math.max(0, num(a[1]) || 0); return n ? str(a[0]).slice(-n) : ""; } },
    "mid": { min: 2, max: 3, syntax: "mid(文字, 開始, 文字数)", desc: "途中から取り出す（1文字目が1）",
      fn: (a) => { const s = str(a[0]), st = Math.max(1, num(a[1]) || 1) - 1; return a.length > 2 ? s.substr(st, Math.max(0, num(a[2]) || 0)) : s.slice(st); } },
    "len": { min: 1, max: 1, syntax: "len(文字)", desc: "文字数", fn: (a) => str(a[0]).length },
    "trim": { min: 1, max: 1, syntax: "trim(文字)", desc: "前後の空白を除く", fn: (a) => str(a[0]).trim() },
    "upper": { min: 1, max: 1, syntax: "upper(文字)", desc: "大文字にする", fn: (a) => str(a[0]).toUpperCase() },
    "lower": { min: 1, max: 1, syntax: "lower(文字)", desc: "小文字にする", fn: (a) => str(a[0]).toLowerCase() },
    "zen": { min: 1, max: 1, syntax: "zen(文字)", desc: "半角カナを全角に（英数字は半角に）", fn: (a) => str(a[0]).normalize("NFKC") },
    "replace": { min: 3, max: 3, syntax: "replace(文字, 探す, 置き換え)", desc: "すべて置き換える",
      fn: (a) => { const f = str(a[1]); return f ? str(a[0]).split(f).join(str(a[2])) : str(a[0]); } },
    "contains": { min: 2, max: 2, syntax: "contains(文字, 探す)", desc: "含むか", fn: (a) => str(a[0]).includes(str(a[1])) },
    "startswith": { min: 2, max: 2, syntax: "startswith(文字, 探す)", desc: "で始まるか", fn: (a) => str(a[0]).startsWith(str(a[1])) },
    "endswith": { min: 2, max: 2, syntax: "endswith(文字, 探す)", desc: "で終わるか", fn: (a) => str(a[0]).endsWith(str(a[1])) },
    "concat": { min: 1, max: null, syntax: "concat(値1, 値2, …)", desc: "つなげる（& と同じ）", fn: (a) => a.map(str).join("") },

    "num": { min: 1, max: 1, syntax: "num(値)", desc: "数値にする（できなければ空欄）", fn: (a) => { const n = num(a[0]); return isNaN(n) ? "" : n; } },
    "round": { min: 1, max: 2, syntax: "round(数値, 桁)", desc: "四捨五入",
      fn: (a) => { const n = num(a[0]); if (isNaN(n)) return ""; const d = a.length > 1 ? num(a[1]) || 0 : 0; const f = Math.pow(10, d); return Math.round(n * f) / f; } },
    "abs": { min: 1, max: 1, syntax: "abs(数値)", desc: "絶対値", fn: (a) => { const n = num(a[0]); return isNaN(n) ? "" : Math.abs(n); } },
    "min": { min: 1, max: null, syntax: "min(数値1, 数値2, …)", desc: "最小（空欄は無視）",
      fn: (a) => { const v = a.map(num).filter((x) => !isNaN(x)); return v.length ? Math.min.apply(null, v) : ""; } },
    "max": { min: 1, max: null, syntax: "max(数値1, 数値2, …)", desc: "最大（空欄は無視）",
      fn: (a) => { const v = a.map(num).filter((x) => !isNaN(x)); return v.length ? Math.max.apply(null, v) : ""; } },
    "text": { min: 2, max: 2, syntax: "text(数値, 小数桁)", desc: "小数桁をそろえた文字にする",
      fn: (a) => { const n = num(a[0]); return isNaN(n) ? str(a[0]) : n.toFixed(Math.max(0, Math.min(10, num(a[1]) || 0))); } },

    "today": { min: 0, max: 0, syntax: "today()", desc: "今日の日付", fn: () => fmtDate(new Date(), "yyyy/MM/dd") },
    "days": { min: 2, max: 2, syntax: "days(開始日, 終了日)", desc: "日数の差（終了日−開始日）",
      fn: (a) => { const x = parseDate(a[0]), y = parseDate(a[1]); if (!x || !y) return "";
        const u = (d) => Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()); return Math.round((u(y) - u(x)) / 86400000); } },
    "datefmt": { min: 2, max: 2, syntax: "datefmt(日付, \"MM/dd\")", desc: "日付の書式を変える",
      fn: (a) => { const d = parseDate(a[0]); return d ? fmtDate(d, str(a[1])) : ""; } }
  };

  function evaluate(node, ref) {
    const ev = (n) => evaluate(n, ref);
    switch (node.k) {
      case "lit": return node.v;
      case "ref": return ref(node.r);
      case "neg": { const n = num(ev(node.a)); return isNaN(n) ? "" : -n; }
      case "call": {
        const f = FUNCS[node.f];
        if (f.lazy) return f.fn(node.args, ev);
        return f.fn(node.args.map(ev));
      }
      case "bin": {
        const a = ev(node.a), b = ev(node.b);
        switch (node.op) {
          case "&": return str(a) + str(b);
          case "+": case "-": case "*": case "/": {
            const x = num(a), y = num(b);
            if (isNaN(x) || isNaN(y)) return "";
            if (node.op === "+") return x + y;
            if (node.op === "-") return x - y;
            if (node.op === "*") return x * y;
            return y === 0 ? "" : x / y;
          }
          case "=": return compare(a, b) === 0;
          case "<>": return compare(a, b) !== 0;
          case "<": return compare(a, b) < 0;
          case "<=": return compare(a, b) <= 0;
          case ">": return compare(a, b) > 0;
          case ">=": return compare(a, b) >= 0;
        }
      }
    }
    return "";
  }

  /**
   * compile(src, resolve) -> { ok, ast, refs, error }
   * resolve(name) returns a reference object or null.
   */
  function compile(src, resolve) {
    const refs = [];
    try {
      if (!String(src || "").trim()) return { ok: false, error: "式が空です", refs: refs };
      const ast = parse(tokenize(src), resolve, refs);
      return { ok: true, ast: ast, refs: Array.from(new Set(refs)) };
    } catch (e) {
      return { ok: false, error: e.message, refs: refs };
    }
  }

  window.CM3Expr = {
    compile: compile,
    evaluate: evaluate,
    str: str,
    num: num,
    truthy: truthy,
    blank: blank,
    compare: compare,
    parseDate: parseDate,
    fmtDate: fmtDate,
    help: Object.keys(FUNCS).map((k) => ({ name: k, syntax: FUNCS[k].syntax, desc: FUNCS[k].desc }))
  };
})();
