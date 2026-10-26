/* Column designer: order, visibility, names, width, formula columns, 読み替え, formats, rules. */
(function () {
  "use strict";

  const $ = (id) => document.getElementById(id);
  const V = window.CM3View;
  const X = window.CM3Expr;
  const App = () => window.CM3App;
  const esc = (s) => App().esc(s);

  const d = {
    settings: null, view: null, sample: [], masterAll: [], keyCols: [],
    memos: [], sel: null, mode: "col", chip: "all", mapSel: "",
    open: {}, dirty: false, timer: null, dragId: null, dragOk: false
  };

  const SRC_LABEL = { schedule: "スケジュール", master: "台帳", calc: "計算" };
  const FMT_LABEL = { none: "そのまま", number: "数値", date: "日付", text: "文字" };
  const ALIGN = [["auto", "自動"], ["left", "左"], ["center", "中央"], ["right", "右"]];

  const clone = (o) => JSON.parse(JSON.stringify(o));
  const byId = (id) => d.settings.columns.find((c) => c.id === id);
  const vcol = (id) => d.view.byId[id];

  // ------------------------------------------------------------ open / close
  async function open() {
    $("dlgCols").hidden = false;
    d.settings = clone(App().state.viewSettings);
    d.dirty = false;
    d.mode = "col";
    d.chip = "all";
    $("cdFilter").value = "";
    $("cdMsg").textContent = "実データを読み込み中...";
    d.sample = [];
    let setsubi = App().state.setsubi;
    if (setsubi.startsWith("__")) setsubi = (window.APP.hot.equipments[0] || window.APP.defaultSetsubi);
    try {
      const res = await fetch("/api/view/sample?setsubi=" + encodeURIComponent(setsubi), { cache: "no-store" });
      const j = await res.json();
      if (!j.success) throw new Error(j.message || "取得できませんでした");
      d.sample = j.items || [];
      d.masterAll = j.masterAll || [];
      d.keyCols = j.masterKeyCols || [];
      $("cdMsg").textContent = "";
    } catch (e) {
      const st = App().state.data;
      d.masterAll = (st && st.masterAll) || [];
      d.keyCols = (st && st.masterKeyCols) || [];
      $("cdMsg").textContent = "実データを取得できないため、見え方の表示は省略します（" + e.message + "）";
    }
    completeColumns();
    if (!d.sel || !byId(d.sel)) d.sel = d.settings.columns[0].id;
    rebuild(false);
    renderAll();
  }

  function close(force) {
    if (!force && d.dirty && !confirm("保存していない変更があります。破棄して閉じますか？")) return;
    clearTimeout(d.timer);
    App().setDraft(null);
    $("dlgCols").hidden = true;
  }

  /** every schedule field and every master column appears in the list */
  function completeColumns() {
    const cols = d.settings.columns;
    const ids = new Set(cols.map((c) => c.id));
    window.APP.scheduleColumns.forEach((f) => {
      if (!ids.has("s:" + f.key)) cols.push(newCol("s:" + f.key, "schedule", f.key, false));
    });
    d.masterAll.forEach((m) => {
      if (!ids.has("m:" + m)) cols.push(newCol("m:" + m, "master", m, false));
    });
    d.settings.maps = d.settings.maps || {};
    d.settings.rules = d.settings.rules || [];
  }
  function newCol(id, source, field, visible) {
    return { id: id, source: source, field: field, label: "", visible: visible, width: null, align: "auto",
      bold: false, format: { type: "none" }, map: "" };
  }

  // ------------------------------------------------------------ rebuild / preview
  function rebuild(live) {
    d.view = V.build(d.settings, d.masterAll);
    d.memos = d.sample.map(() => new Map());
    if (live !== false) {
      clearTimeout(d.timer);
      d.timer = setTimeout(() => App().setDraft(clone(d.settings)), 200);
    }
  }

  function changed(opt) {
    d.dirty = true;
    rebuild();
    if (!opt || !opt.noList) renderList();
    if (!opt || !opt.noRight) renderRight();
    renderCounts();
  }

  function rowsFor(c) {
    // master columns only have values on lot rows that matched the master
    const out = [];
    d.sample.forEach((it, i) => {
      if (c.source === "master" && !(it.rowKind === "lot" && it.mHit)) return;
      out.push(i);
    });
    return out;
  }

  function cellAt(c, i) { return V.cell(d.view, vcol(c.id), d.sample[i], d.memos[i]); }

  function previewText(c) {
    for (const i of rowsFor(c)) {
      const t = cellAt(c, i).text;
      if (t !== "") return t;
    }
    return null;
  }

  function stats(c) {
    const idx = rowsFor(c);
    let filled = 0, nums = 0, dates = 0;
    idx.forEach((i) => {
      const raw = V.rawValue(d.view, vcol(c.id), d.sample[i], d.memos[i], 0);
      if (X.blank(raw)) return;
      filled++;
      if (V.isNumLike(raw)) nums++;
      else if (X.parseDate(raw)) dates++;
    });
    let kind = "-";
    if (filled) kind = nums >= filled * 0.6 ? "数値が多い" : dates >= filled * 0.6 ? "日付が多い" : "文字が多い";
    return { total: idx.length, filled: filled, kind: kind };
  }

  function distinctRaw(c, limit) {
    const m = new Map();
    rowsFor(c).forEach((i) => {
      const raw = X.str(V.rawValue(d.view, vcol(c.id), d.sample[i], d.memos[i], 0));
      const k = V.normVal(raw);
      const e = m.get(k) || { raw: raw, n: 0, i: i };
      e.n++;
      m.set(k, e);
    });
    return Array.from(m.values()).sort((a, b) => b.n - a.n).slice(0, limit || 1000);
  }

  // ------------------------------------------------------------ left list
  function rulesOn(id) { return d.settings.rules.filter((r) => r.col === id || r.cellCol === id).length; }

  function badges(c) {
    let h = "";
    if (c.source === "calc") h += '<span class="badge b-calc">式</span>';
    if (c.map && d.settings.maps[c.map]) h += '<span class="badge b-map">読み替え ' + esc(c.map) + "</span>";
    if (c.format && c.format.type && c.format.type !== "none") h += '<span class="badge b-fmt">' + FMT_LABEL[c.format.type] + "</span>";
    const rn = rulesOn(c.id);
    if (rn) h += '<span class="badge b-rule">色分け ' + rn + "</span>";
    if (c.source === "master" && d.keyCols.includes(c.field)) h += '<span class="badge b-key">キー</span>';
    return h;
  }

  function listCols() {
    const q = $("cdFilter").value.trim().toLowerCase();
    return d.settings.columns.filter((c) => {
      if (d.chip === "schedule" && c.source !== "schedule") return false;
      if (d.chip === "master" && c.source !== "master") return false;
      if (d.chip === "calc" && c.source !== "calc") return false;
      if (d.chip === "on" && c.visible === false) return false;
      if (d.chip === "off" && c.visible !== false) return false;
      if (!q) return true;
      return (V.title(c) + " " + V.baseName(c) + " " + (c.field || "")).toLowerCase().includes(q);
    });
  }

  function renderChips() {
    const cs = d.settings.columns;
    const n = (f) => cs.filter(f).length;
    const chips = [
      ["all", "すべて", cs.length],
      ["schedule", "スケジュール", n((c) => c.source === "schedule")],
      ["master", "台帳", n((c) => c.source === "master")],
      ["calc", "計算", n((c) => c.source === "calc")],
      ["on", "表示中", n((c) => c.visible !== false)],
      ["off", "非表示", n((c) => c.visible === false)]
    ];
    $("cdChips").innerHTML = chips.map((c) => '<button type="button" class="chip' + (d.chip === c[0] ? " on" : "") +
      '" data-chip="' + c[0] + '">' + c[1] + ' <span class="n">' + c[2] + "</span></button>").join("");
  }

  function renderList() {
    const box = $("cdList");
    const top = box.scrollTop;
    const focus = saveFocus();
    const cols = listCols();
    box.innerHTML = cols.map((c) => {
      const pv = d.sample.length ? previewText(c) : "";
      const on = c.visible !== false;
      return '<div class="cd-row' + (c.id === d.sel ? " sel" : "") + (on ? "" : " off") + '" draggable="true" data-id="' + esc(c.id) + '">' +
        '<span><input type="checkbox" class="vis" ' + (on ? "checked" : "") + ' title="表示する"></span>' +
        '<span class="cd-name"><span class="handle" title="ドラッグで並べ替え">⠿</span><span class="dot ' + c.source + '" title="' +
        SRC_LABEL[c.source] + '"></span><span class="nm" title="' + esc(c.field || "") + '">' + esc(V.baseName(c)) + "</span>" + badges(c) + "</span>" +
        '<span><input class="lbl" type="text" data-fk="lbl:' + esc(c.id) + '" placeholder="' + esc(V.baseName(c)) + '" value="' +
        esc(c.source === "calc" ? "" : c.label || "") + '"' + (c.source === "calc" ? ' disabled title="計算列の名前は右の「この列の作り方」で変えます"' : "") + "></span>" +
        '<span class="cd-prev' + (pv === null ? " none" : "") + '" title="' + esc(pv || "") + '">' +
        (pv === null ? "（値のある行がありません）" : esc(pv)) + "</span>" +
        "<span>" + (c.source === "calc" ? '<button type="button" class="cd-del" title="この列を削除">×</button>' : "") + "</span></div>";
    }).join("") || '<div class="note" style="padding:16px">該当する列がありません</div>';
    box.scrollTop = top;
    restoreFocus(focus);
    const all = cols.length && cols.every((c) => c.visible !== false);
    $("cdAll").checked = !!all;
    $("cdAll").indeterminate = !all && cols.some((c) => c.visible !== false);
    renderChips();
  }

  function renderCounts() {
    const cs = d.settings.columns;
    $("cdCount").textContent = "表示 " + cs.filter((c) => c.visible !== false).length + " / " + cs.length + " 列";
    const maps = Object.keys(d.settings.maps);
    const used = cs.filter((c) => c.map && d.settings.maps[c.map]).length;
    $("cdMapInfo").textContent = maps.length + " 個（" + used + " 列で使用）";
    const rs = d.settings.rules;
    $("cdRuleInfo").textContent = rs.length + " 件" + (rs.length ? "（有効 " + rs.filter((r) => r.enabled !== false).length + "）" : "");
  }

  function renderAll() { renderList(); renderRight(); renderCounts(); }

  // keep focus/caret across re-render
  function saveFocus() {
    const a = document.activeElement;
    if (!a || !a.dataset || !a.dataset.fk) return null;
    return { fk: a.dataset.fk, s: a.selectionStart, e: a.selectionEnd };
  }
  function restoreFocus(f) {
    if (!f) return;
    const el = document.querySelector('[data-fk="' + CSS.escape(f.fk) + '"]');
    if (!el) return;
    el.focus();
    try { if (f.s != null) el.setSelectionRange(f.s, f.e); } catch (e) { /* select etc. */ }
  }

  // ------------------------------------------------------------ right: column
  function card(key, num, cls, title, sum, body, defOpen) {
    const isOpen = d.open[key] != null ? d.open[key] : defOpen;
    return '<div class="card2' + (isOpen ? "" : " closed") + '" data-card="' + key + '"><div class="hd" data-act="toggle" data-v="' + key + '">' +
      '<span class="num-ic ' + (cls || "") + '">' + num + "</span><h4>" + title + '</h4><span class="sum">' + esc(sum) + '</span><span class="chev">›</span></div>' +
      '<div class="bd">' + body + "</div></div>";
  }
  function seg(act, cur, opts) {
    return '<span class="seg">' + opts.map((o) => '<button type="button" data-act="' + act + '" data-v="' + o[0] + '" class="' +
      (String(cur) === String(o[0]) ? "on" : "") + '">' + o[1] + "</button>").join("") + "</span>";
  }
  function chk(fk, on, label) {
    return '<label class="row-gap"><input type="checkbox" data-fk="' + fk + '"' + (on ? " checked" : "") + "> " + label + "</label>";
  }

  function refNames() {
    const out = [];
    window.APP.scheduleColumns.forEach((f) => out.push({ name: f.label, g: "スケジュール" }));
    d.settings.columns.filter((c) => c.source === "calc").forEach((c) => out.push({ name: V.title(c), g: "計算列" }));
    d.masterAll.forEach((m) => out.push({ name: m, g: "台帳" }));
    return out;
  }

  function renderRight() {
    const focus = saveFocus();
    const box = $("cdRight");
    if (d.mode === "rules") box.innerHTML = rulesPane();
    else if (d.mode === "maps") box.innerHTML = mapsPane();
    else box.innerHTML = colPane();
    restoreFocus(focus);
  }

  function colPane() {
    const c = byId(d.sel);
    if (!c) return '<p class="note">左の一覧から列を選んでください。</p>';
    const st = d.sample.length ? stats(c) : null;
    const f = c.format || { type: "none" };
    const src = c.source === "schedule" ? "スケジュールの項目（" + c.field + "）"
      : c.source === "master" ? "台帳の列（" + c.field + "）" + (d.keyCols.includes(c.field) ? "　※突合せキー" : "")
        : "式で作る列（見出しを押すと、式の答えで並べ替えます）";

    let h = '<div class="card2 info-card"><h3><span class="dot ' + c.source + '"></span><span class="sp">' + esc(V.title(c)) + "</span>" +
      (c.source === "calc" ? '<button type="button" class="btn small" data-act="delcol" style="color:var(--err)">削除</button>' : "") + "</h3>" +
      '<div class="kv2"><span>元の列</span><b>' + esc(src) + "</b>" +
      (st ? "<span>値のある行</span><b>" + st.filled + " / " + st.total + "（空欄 " + (st.total - st.filled) + "）</b><span>値の種類</span><b>" + st.kind + "</b>" : "") +
      "</div></div>";

    // 1. 見せ方
    const look = '<div class="frm">' +
      '<span class="lab">表示名</span><input type="text" data-fk="label" value="' + esc(c.label || "") + '" placeholder="' + esc(V.baseName(c)) + '">' +
      '<span class="lab">表示</span>' + chk("visible", c.visible !== false, "一覧に表示する") +
      '<span class="lab">幅</span><span class="row-gap">' + seg("wmode", c.width ? "fix" : "auto", [["auto", "自動"], ["fix", "指定"]]) +
      (c.width ? '<input type="number" min="30" max="800" step="10" data-fk="width" value="' + c.width + '"> px' : "") + "</span>" +
      '<span class="lab">寄せ</span>' + seg("align", c.align || "auto", ALIGN) +
      '<span class="lab">太字</span>' + chk("bold", c.bold, "太字にする") +
      '<span class="lab">順番</span><span class="row-gap">' +
      ["top:先頭へ", "up:上へ", "down:下へ", "bottom:末尾へ"].map((x) => '<button type="button" class="btn small" data-act="move" data-v="' +
        x.split(":")[0] + '">' + x.split(":")[1] + "</button>").join("") + "</span></div>";
    h += card("look", "1", "", "見せ方", (c.width ? "幅 " + c.width + "px" : "幅 自動") + " / " +
      ALIGN.find((a) => a[0] === (c.align || "auto"))[1] + (c.bold ? " / 太字" : ""), look, c.source !== "calc");

    // 式
    if (c.source === "calc") {
      const vc = vcol(c.id);
      const cmp = vc && vc._cmp;
      let res = "";
      if (cmp && cmp.ok && d.sample.length) {
        const vals = [];
        for (let i = 0; i < d.sample.length && vals.length < 3; i++) {
          if (d.sample[i].rowKind !== "lot") continue;
          vals.push(X.str(V.rawValue(d.view, vc, d.sample[i], d.memos[i], 0)));
        }
        res = '<div class="row-gap" style="margin-top:6px"><span class="note" style="margin:0">先頭3件の結果</span>' +
          vals.map((v) => '<span class="code-chip">' + (v === "" ? "（空欄）" : esc(v)) + "</span>").join("") + "</div>";
      }
      const refs = refNames();
      const body = '<div class="frm"><span class="lab">列の名前</span><input type="text" data-fk="label2" value="' + esc(c.label || "") + '" placeholder="計算列"></div>' +
        '<textarea class="expr-box" data-fk="expr" spellcheck="false" placeholder="例）if([特急区分]=&quot;2&quot;,&quot;超特急&quot;,&quot;&quot;)">' + esc(c.expr || "") + "</textarea>" +
        (cmp ? (cmp.ok ? '<div class="expr-ok">使える式です' + (cmp.refs.length ? "（使っている列: " + esc(cmp.refs.join(", ")) + "）" : "") + "</div>"
          : '<div class="expr-ng">' + esc(cmp.error) + "</div>") : "") + res +
        '<div class="row-gap" style="margin-top:10px"><select data-fk="refpick">' +
        ["スケジュール", "計算列", "台帳"].map((g) => '<optgroup label="' + g + '">' +
          refs.filter((r) => r.g === g && !(g === "計算列" && r.name === V.title(c))).map((r) => '<option value="' + esc(r.name) + '">' +
            esc(r.name.normalize("NFKC")) + "</option>").join("") + "</optgroup>").join("") +
        '</select><button type="button" class="btn small" data-act="insref">列を式に入れる</button>' +
        '<button type="button" class="btn small" data-act="fnhelp">関数の一覧</button></div>' +
        (d.open.fnhelp ? '<div class="fn-help">' + X.help.map((f) => '<div data-act="insfn" data-v="' + f.name + '"><code>' +
          esc(f.syntax) + "</code><span>" + esc(f.desc) + "</span></div>").join("") + "</div>" : "") +
        '<p class="note">[列名] で列の値（読み替え前の元の値）を使います。文字は "" で囲みます。&amp; で文字をつなぎ、= &lt;&gt; &lt; &gt; で比べます。</p>';
      h += card("expr", "式", "f", "この列の作り方", "式で作る列", body, true);
    }

    // 2. 値の整え方
    let fb = '<div class="frm"><span class="lab">種類</span>' + seg("fmt", f.type || "none",
      [["none", "そのまま"], ["number", "数値"], ["date", "日付"], ["text", "文字"]]);
    if (f.type === "number") {
      fb += '<span class="lab">小数の桁</span><select data-fk="f.decimals"><option value="">そのまま</option>' +
        [0, 1, 2, 3, 4].map((n) => '<option value="' + n + '"' + (String(f.decimals) === String(n) ? " selected" : "") + ">" + n + " 桁</option>").join("") + "</select>" +
        '<span class="lab">区切り</span>' + chk("f.comma", f.comma, "3桁ごとにカンマ（1,234）") +
        '<span class="lab">0 のとき</span>' + chk("f.zeroBlank", f.zeroBlank, "空欄にする");
    } else if (f.type === "date") {
      fb += '<span class="lab">書式</span><select data-fk="f.dateFmt">' + V.DATE_FORMATS.map((x) => '<option value="' + x + '"' +
        ((f.dateFmt || "yyyy/MM/dd") === x ? " selected" : "") + ">" + x + "（" + X.fmtDate(new Date(2026, 9, 19, 8, 5), x) + "）</option>").join("") + "</select>";
    } else if (f.type === "text") {
      fb += '<span class="lab">空白</span>' + chk("f.trim", f.trim !== false, "前後の空白を除く") +
        '<span class="lab">半角カナ</span>' + chk("f.zen", f.zen, "全角にする（ｺｲﾙ → コイル）") +
        '<span class="lab">最大文字数</span><span class="row-gap"><input type="number" min="0" max="200" data-fk="f.maxLen" value="' + (f.maxLen || "") + '"> 文字（超えたら …）</span>';
    }
    fb += '<span class="lab">前に付ける</span><input type="text" data-fk="f.prefix" value="' + esc(f.prefix || "") + '" placeholder="例）￥">' +
      '<span class="lab">後に付ける</span><input type="text" data-fk="f.suffix" value="' + esc(f.suffix || "") + '" placeholder="例）%  kg  mm"></div>';
    h += card("fmt", "2", "", "値の整え方", FMT_LABEL[f.type || "none"], fb, false);

    // 3. 読み替え
    h += card("map", "3", "", "読み替え", c.map && d.settings.maps[c.map] ? c.map : "しない", mapBody(c), false);

    // 結果
    let rb = "";
    let changes = false;
    if (d.sample.length) {
      const dv = distinctRaw(c, 10);
      rb = '<table class="result-tbl"><tr><th>元の値</th><th></th><th>表示</th><th style="text-align:right">件数</th></tr>' +
        dv.map((e) => {
          const t = cellAt(c, e.i).text;
          if (t !== X.str(e.raw)) changes = true;
          return "<tr><td>" + (e.raw === "" ? '<span class="note">（空欄）</span>' : esc(e.raw)) + '</td><td class="ar">→</td><td>' +
            (t === "" ? '<span class="note">（空欄）</span>' : esc(t)) + '</td><td style="text-align:right">' + e.n + "</td></tr>";
        }).join("") + "</table>" + '<p class="note">実データ ' + rowsFor(c).length + " 行から、多い順に最大10種類</p>";
    } else rb = '<p class="note">実データがありません</p>';
    h += card("result", "✓", "", "結果", changes ? "変わります" : "変わりません", rb, true);
    return h;
  }

  function mapBody(c) {
    const maps = d.settings.maps;
    const names = Object.keys(maps);
    let h = '<div class="frm"><span class="lab">変換表</span><select data-fk="mapsel"><option value="">しない</option>' +
      names.map((n) => '<option value="' + esc(n) + '"' + (c.map === n ? " selected" : "") + ">" + esc(n) + "</option>").join("") +
      '<option value="__new">＋ 新しい変換表を作る</option></select></div>';
    if (c.map && maps[c.map]) h += mapEditor(c.map, c);
    else h += '<p class="note">値を別の表示に置き換えます（例：0 → 空欄、1 → 特、2 → 超特）。変換表は他の列でも使い回せます。</p>';
    return h;
  }

  function mapEditor(name, c) {
    const mp = d.settings.maps[name];
    const users = d.settings.columns.filter((x) => x.map === name).map((x) => V.title(x));
    const counts = new Map();
    if (c && d.sample.length) distinctRaw(c).forEach((e) => counts.set(V.normVal(e.raw), e.n));
    const ents = mp.entries || [];
    return '<div class="frm"><span class="lab">変換表の名前</span><input type="text" data-fk="mapname" data-map="' + esc(name) + '" value="' + esc(name) + '">' +
      '<span class="lab">使っている列</span><span class="note" style="margin:0">' + esc(users.join("、") || "なし") + "</span></div>" +
      '<table class="maptbl"><tr><th style="width:40%">元の値</th><th></th><th style="width:40%">表示</th><th>' + (c ? "件数" : "") + "</th><th></th></tr>" +
      ents.map((e, i) => '<tr><td><input type="text" data-fk="me:' + esc(name) + ":" + i + ':0" value="' + esc(e[0]) + '"></td><td class="note">→</td>' +
        '<td><input type="text" data-fk="me:' + esc(name) + ":" + i + ':1" value="' + esc(e[1]) + '" placeholder="（空欄）"></td>' +
        '<td class="cnt">' + (c ? (counts.get(V.normVal(e[0])) || 0) : "") + "</td>" +
        '<td><button type="button" class="cd-del" data-act="medel" data-map="' + esc(name) + '" data-v="' + i + '">×</button></td></tr>').join("") +
      "</table>" +
      '<div class="row-gap" style="margin-top:8px">' +
      (c ? '<button type="button" class="btn small" data-act="mecollect" data-map="' + esc(name) + '">実データから値を集める</button>' : "") +
      '<button type="button" class="btn small" data-act="meadd" data-map="' + esc(name) + '">行を追加</button></div>' +
      '<div class="frm"><span class="lab">表に無い値</span><span class="row-gap">' +
      seg("melse:" + name, mp.else || "keep", [["keep", "そのまま"], ["blank", "空欄"], ["text", "指定の文字"]]) +
      ((mp.else === "text") ? '<input type="text" data-fk="meelse:' + esc(name) + '" value="' + esc(mp.elseText || "") + '" style="width:120px">' : "") +
      "</span></div>" +
      '<p class="note">比べるときは全角／半角・前後の空白の違いを無視します。「実データから値を集める」で、まだ表に無い値を追加します（最初は元の値と同じ表示）。</p>';
  }

  // ------------------------------------------------------------ right: maps
  function mapsPane() {
    const maps = d.settings.maps;
    const names = Object.keys(maps);
    let h = '<div class="pane-title"><h3>変換表の一覧</h3><button type="button" class="btn small" data-act="back">列の設定に戻る</button></div>' +
      '<p class="note">変換表は、値を別の表示に置き換える表です。列ごとに「3. 読み替え」で選んで使います。</p>' +
      '<div class="row-gap" style="margin-bottom:10px"><button type="button" class="btn small" data-act="mapnew">＋ 新しい変換表</button></div>';
    if (!names.length) return h + '<p class="note">変換表はまだありません。</p>';
    names.forEach((n) => {
      const users = d.settings.columns.filter((x) => x.map === n);
      const isOpen = d.mapSel === n;
      h += '<div class="card2' + (isOpen ? "" : " closed") + '"><div class="hd" data-act="mapopen" data-v="' + esc(n) + '"><span class="num-ic">表</span><h4>' +
        esc(n) + '</h4><span class="sum">' + (maps[n].entries || []).length + " 行 / " + users.length + ' 列で使用</span><span class="chev">›</span></div>' +
        '<div class="bd">' + (isOpen ? mapEditor(n, users[0] || null) +
          '<div class="row-gap" style="margin-top:8px"><button type="button" class="btn small" data-act="mapdel" data-map="' + esc(n) + '"' +
          (users.length ? " disabled title=\"使っている列があるため削除できません\"" : "") + ' style="color:var(--err)">この変換表を削除</button></div>' : "") +
        "</div></div>";
    });
    return h;
  }

  // ------------------------------------------------------------ right: rules
  function colOptions(cur) {
    return d.settings.columns.map((c) => '<option value="' + esc(c.id) + '"' + (c.id === cur ? " selected" : "") + ">" +
      esc(V.title(c)) + (c.visible === false ? "（非表示）" : "") + "</option>").join("");
  }
  function rulesPane() {
    const rs = d.settings.rules;
    let h = '<div class="pane-title"><h3>色分けルール</h3><button type="button" class="btn small" data-act="back">列の設定に戻る</button></div>' +
      '<p class="note">条件に合う製造行（または指定した列のセル）に色を付けます。上から順に判定し、<b>下のルールほど優先</b>されます。比べる値は「読み替え」後の値です。</p>' +
      '<div class="row-gap" style="margin-bottom:10px"><button type="button" class="btn small" data-act="radd">＋ ルールを追加</button></div>';
    const lotIdx = d.sample.map((it, i) => (it.rowKind === "lot" ? i : -1)).filter((i) => i >= 0);
    rs.forEach((r, i) => {
      const vr = d.view.rules[i];
      let hit = "";
      let err = "";
      if (vr && vr.when === "expr" && vr._cmp && !vr._cmp.ok) err = vr._cmp.error;
      if (d.sample.length && !err) {
        const n = lotIdx.filter((k) => V.ruleMatch(d.view, vr, d.sample[k], d.memos[k])).length;
        hit = "該当 " + n + " / " + lotIdx.length + " 行";
      }
      const p = "r:" + i + ":";
      const cond = r.when === "expr"
        ? '<input type="text" class="rx" data-fk="' + p + 'expr" value="' + esc(r.expr || "") + '" placeholder="例）and([特急区分]&lt;&gt;&quot;0&quot;, [保留_区分]=&quot;0&quot;)">'
        : '<select data-fk="' + p + 'col">' + colOptions(r.col) + "</select>" +
          (["blank", "notblank"].includes(r.op) ? "" : '<input type="text" class="rv" data-fk="' + p + 'value" value="' + esc(r.value || "") + '" placeholder="値">') +
          '<select data-fk="' + p + 'op">' + V.OPS.map((o) => '<option value="' + o.v + '"' + ((r.op || "eq") === o.v ? " selected" : "") + ">" + o.label + "</option>").join("") + "</select>";
      const sample = '<span class="sample" style="' + V.styleText({ bg: r.bg, color: r.color, bold: r.bold }) + '">見本 123</span>';
      h += '<div class="rule-card' + (r.enabled === false ? " off" : "") + '">' +
        '<div class="r1"><label class="row-gap"><input type="checkbox" data-fk="' + p + 'enabled"' + (r.enabled !== false ? " checked" : "") + "> 有効</label>" +
        '<b style="margin-left:4px">ルール ' + (i + 1) + "</b>" + seg("rwhen:" + i, r.when || "simple", [["simple", "列の値で"], ["expr", "式で"]]) +
        '<span class="hit">' + esc(hit) + "</span></div>" +
        '<div class="r2"><span class="note" style="margin:0">条件</span>' + cond + "</div>" +
        (err ? '<div class="expr-ng">' + esc(err) + "</div>" : "") +
        '<div class="r3"><span class="note" style="margin:0">色を付ける所</span>' + seg("rtarget:" + i, r.target || "row", [["row", "行全体"], ["cell", "列だけ"]]) +
        (r.target === "cell" ? '<select data-fk="' + p + 'cellCol">' + colOptions(r.cellCol || r.col) + "</select>" : "") + "</div>" +
        '<div class="r3"><span class="colpick"><input type="checkbox" data-fk="' + p + 'bgOn"' + (r.bg ? " checked" : "") + '>背景<input type="color" data-fk="' + p + 'bg" value="' + (r.bg || "#fff1b8") + '"></span>' +
        '<span class="colpick"><input type="checkbox" data-fk="' + p + 'colorOn"' + (r.color ? " checked" : "") + '>文字<input type="color" data-fk="' + p + 'color" value="' + (r.color || "#b42318") + '"></span>' +
        '<label class="row-gap"><input type="checkbox" data-fk="' + p + 'bold"' + (r.bold ? " checked" : "") + ">太字</label>" + sample +
        '<span style="margin-left:auto" class="row-gap"><button type="button" class="btn small" data-act="rup" data-v="' + i + '">↑</button>' +
        '<button type="button" class="btn small" data-act="rdown" data-v="' + i + '">↓</button>' +
        '<button type="button" class="btn small" data-act="rdel" data-v="' + i + '" style="color:var(--err)">削除</button></span></div></div>';
    });
    if (!rs.length) h += '<p class="note">ルールはまだありません。例：特急区分 が 0 と違う → 行全体を黄色。</p>';
    return h;
  }

  // ------------------------------------------------------------ editing helpers
  function setFmt(c, key, val) {
    c.format = Object.assign({ type: "none" }, c.format || {});
    c.format[key] = val;
  }

  function uniqueMapName(base) {
    let n = base || "変換表", i = 2;
    while (d.settings.maps[n]) n = (base || "変換表") + " " + i++;
    return n;
  }

  function moveCol(id, where) {
    const cs = d.settings.columns;
    const i = cs.findIndex((c) => c.id === id);
    if (i < 0) return;
    const [c] = cs.splice(i, 1);
    let j = where === "top" ? 0 : where === "bottom" ? cs.length : where === "up" ? Math.max(0, i - 1) : Math.min(cs.length, i + 1);
    cs.splice(j, 0, c);
  }

  function insertAtCursor(text) {
    const ta = document.querySelector('[data-fk="expr"]');
    const c = byId(d.sel);
    if (!c) return;
    const s = c.expr || "";
    let a = s.length, b = s.length;
    if (ta) { a = ta.selectionStart; b = ta.selectionEnd; }
    c.expr = s.slice(0, a) + text + s.slice(b);
    changed();
    const ta2 = document.querySelector('[data-fk="expr"]');
    if (ta2) { ta2.focus(); ta2.setSelectionRange(a + text.length, a + text.length); }
  }

  // ------------------------------------------------------------ right: events
  function onRightInput(e) {
    const el = e.target;
    const fk = el.dataset.fk;
    if (!fk) return;
    const val = el.type === "checkbox" ? el.checked : el.value;
    const c = byId(d.sel);

    if (fk.startsWith("r:")) {                          // rules
      const [, i, key] = fk.split(":");
      const r = d.settings.rules[+i];
      if (key === "bgOn") r.bg = val ? (document.querySelector('[data-fk="r:' + i + ':bg"]').value) : "";
      else if (key === "colorOn") r.color = val ? (document.querySelector('[data-fk="r:' + i + ':color"]').value) : "";
      else r[key] = val;
      const typingR = e.type === "input";
      changed({ noRight: typingR });
      if (typingR) renderRightSoon();
      return;
    }
    if (fk.startsWith("me:")) {                         // map entry
      const parts = fk.split(":");
      const col = +parts.pop(), i = +parts.pop(), name = parts.slice(1).join(":");
      d.settings.maps[name].entries[i][col] = val;
      changed({ noRight: true });
      renderRightSoon();
      return;
    }
    if (fk.startsWith("meelse:")) {
      d.settings.maps[fk.slice(7)].elseText = val;
      changed({ noRight: true });
      renderRightSoon();
      return;
    }
    if (fk === "mapname") {
      if (e.type !== "change") return;
      const old = el.dataset.map;
      const nn = String(val).trim();
      if (!nn || nn === old) { el.value = old; return; }
      if (d.settings.maps[nn]) { alert("同じ名前の変換表があります"); el.value = old; return; }
      d.settings.maps[nn] = d.settings.maps[old];
      delete d.settings.maps[old];
      d.settings.columns.forEach((x) => { if (x.map === old) x.map = nn; });
      if (d.mapSel === old) d.mapSel = nn;
      changed();
      return;
    }
    if (fk === "refpick") return;
    if (!c) return;

    if (fk === "mapsel") {
      if (val === "__new") {
        const n = uniqueMapName(V.title(c));
        d.settings.maps[n] = { entries: [], else: "keep", elseText: "" };
        c.map = n;
        d.open.map = true;
      } else c.map = val;
      changed();
      return;
    }
    if (fk === "label" || fk === "label2") c.label = val;
    else if (fk === "visible") c.visible = val;
    else if (fk === "bold") c.bold = val;
    else if (fk === "width") c.width = Math.max(30, Math.min(800, parseInt(val, 10) || 0)) || null;
    else if (fk === "expr") c.expr = val;
    else if (fk.startsWith("f.")) {
      const k = fk.slice(2);
      let v = val;
      if (k === "maxLen") v = parseInt(val, 10) || 0;
      if (k === "decimals") v = val === "" ? "" : +val;
      setFmt(c, k, v);
    }
    const typing = e.type === "input";
    changed({ noRight: typing });
    if (typing) renderRightSoon();
  }

  function onColor(el) {
    const [, i, key] = el.dataset.fk.split(":");
    const r = d.settings.rules[+i];
    r[key] = el.value;                         // choosing a color turns it on
    const on = document.querySelector('[data-fk="r:' + i + ':' + key + 'On"]');
    if (on) on.checked = true;
    const cardEl = el.closest(".rule-card");
    const smp = cardEl && cardEl.querySelector(".sample");
    if (smp) smp.setAttribute("style", V.styleText({ bg: r.bg, color: r.color, bold: r.bold }));
    changed({ noRight: true });
  }

  let rightTimer = null;
  function renderRightSoon() {
    clearTimeout(rightTimer);
    rightTimer = setTimeout(renderRight, 250);
  }

  function onRightClick(e) {
    const b = e.target.closest("[data-act]");
    if (!b) return;
    const act = b.dataset.act, v = b.dataset.v;
    const c = byId(d.sel);

    if (act === "toggle") {
      const el = b.closest(".card2");
      const nowOpen = el.classList.contains("closed");
      d.open[v] = nowOpen;
      el.classList.toggle("closed", !nowOpen);
      return;
    }
    if (act === "back") { d.mode = "col"; renderRight(); return; }
    if (act === "fnhelp") { d.open.fnhelp = !d.open.fnhelp; renderRight(); return; }
    if (act === "insfn") { insertAtCursor(v + "("); return; }
    if (act === "insref") {
      const s = document.querySelector('[data-fk="refpick"]');
      if (s && s.value) insertAtCursor("[" + s.value + "]");
      return;
    }
    // rules
    if (act === "radd") {
      const first = d.settings.columns.find((x) => x.visible !== false && x.source === "master") || byId(d.sel) || d.settings.columns[0];
      d.settings.rules.push({ enabled: true, when: "simple", col: first.id, op: "eq", value: "", expr: "", target: "row", cellCol: "",
        bg: "#fff1b8", color: "", bold: false });
      changed();
      return;
    }
    if (act === "rdel") { d.settings.rules.splice(+v, 1); changed(); return; }
    if (act === "rup" || act === "rdown") {
      const i = +v, j = act === "rup" ? i - 1 : i + 1;
      const rs = d.settings.rules;
      if (j < 0 || j >= rs.length) return;
      [rs[i], rs[j]] = [rs[j], rs[i]];
      changed();
      return;
    }
    if (act.startsWith("rwhen:")) { d.settings.rules[+act.slice(6)].when = v; changed(); return; }
    if (act.startsWith("rtarget:")) { d.settings.rules[+act.slice(8)].target = v; changed(); return; }
    // maps
    if (act === "mapnew") {
      const n = uniqueMapName("変換表");
      d.settings.maps[n] = { entries: [], else: "keep", elseText: "" };
      d.mapSel = n;
      changed();
      return;
    }
    if (act === "mapopen") { d.mapSel = d.mapSel === v ? "" : v; renderRight(); return; }
    if (act === "mapdel") { delete d.settings.maps[b.dataset.map]; d.mapSel = ""; changed(); return; }
    if (act === "meadd") { d.settings.maps[b.dataset.map].entries.push(["", ""]); changed(); return; }
    if (act === "medel") { d.settings.maps[b.dataset.map].entries.splice(+v, 1); changed(); return; }
    if (act === "mecollect") {
      const mp = d.settings.maps[b.dataset.map];
      const col = d.mode === "maps" ? d.settings.columns.find((x) => x.map === b.dataset.map) : c;
      if (!col) return;
      const have = new Set(mp.entries.map((x) => V.normVal(x[0])));
      let added = 0;
      distinctRaw(col).forEach((e) => {
        const k = V.normVal(e.raw);
        if (k === "" || have.has(k)) return;
        mp.entries.push([X.str(e.raw), X.str(e.raw)]);
        have.add(k);
        added++;
      });
      changed();
      $("cdMsg").textContent = added ? added + " 種類の値を追加しました。「表示」を書き換えてください。" : "追加する値はありませんでした";
      return;
    }
    if (act.startsWith("melse:")) { d.settings.maps[act.slice(6)].else = v; changed(); return; }
    if (!c) return;
    // column
    if (act === "delcol") { deleteCol(c); return; }
    if (act === "wmode") { c.width = v === "fix" ? (c.width || 120) : null; changed(); return; }
    if (act === "align") { c.align = v; changed(); return; }
    if (act === "fmt") { c.format = Object.assign({}, c.format || {}, { type: v }); d.open.fmt = true; changed(); return; }
    if (act === "move") { moveCol(c.id, v); changed(); scrollToSel(); return; }
  }

  function deleteCol(c) {
    if (!confirm("「" + V.title(c) + "」を削除しますか？")) return;
    const i = d.settings.columns.indexOf(c);
    d.settings.columns.splice(i, 1);
    d.sel = (d.settings.columns[i] || d.settings.columns[i - 1] || d.settings.columns[0]).id;
    changed();
  }

  function scrollToSel() {
    const el = $("cdList").querySelector('.cd-row[data-id="' + CSS.escape(d.sel) + '"]');
    if (el) el.scrollIntoView({ block: "nearest" });
  }

  // ------------------------------------------------------------ left: events
  function onListClick(e) {
    const row = e.target.closest(".cd-row");
    if (!row) return;
    const c = byId(row.dataset.id);
    if (e.target.classList.contains("cd-del")) {
      d.sel = c.id;
      deleteCol(c);
      return;
    }
    if (e.target.classList.contains("vis") || e.target.classList.contains("lbl")) {
      if (d.sel !== c.id) { d.sel = c.id; d.mode = "col"; renderRight(); markSel(); }
      return;
    }
    d.sel = c.id;
    d.mode = "col";
    markSel();
    renderRight();
  }
  function markSel() {
    $("cdList").querySelectorAll(".cd-row").forEach((r) => r.classList.toggle("sel", r.dataset.id === d.sel));
  }
  function onListChange(e) {
    const row = e.target.closest(".cd-row");
    if (!row) return;
    const c = byId(row.dataset.id);
    if (e.target.classList.contains("vis")) {
      c.visible = e.target.checked;
      if (!d.settings.columns.some((x) => x.visible !== false)) { c.visible = true; e.target.checked = true; return; }
      changed();
    }
  }
  function onListInput(e) {
    if (!e.target.classList.contains("lbl")) return;
    const c = byId(e.target.closest(".cd-row").dataset.id);
    c.label = e.target.value;
    changed({ noList: true });
  }

  // drag & drop (only from the ⠿ handle)
  function onDragStart(e) {
    const row = e.target.closest && e.target.closest(".cd-row");
    if (!row || !d.dragOk) { e.preventDefault(); return; }
    d.dragId = row.dataset.id;
    row.classList.add("dragging");
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData("text/plain", d.dragId);
  }
  function onDragOver(e) {
    if (!d.dragId) return;
    const row = e.target.closest(".cd-row");
    if (!row) return;
    e.preventDefault();
    const r = row.getBoundingClientRect();
    const after = e.clientY > r.top + r.height / 2;
    $("cdList").querySelectorAll(".drop-before,.drop-after").forEach((x) => x.classList.remove("drop-before", "drop-after"));
    if (row.dataset.id !== d.dragId) row.classList.add(after ? "drop-after" : "drop-before");
  }
  function onDrop(e) {
    if (!d.dragId) return;
    e.preventDefault();
    const row = e.target.closest(".cd-row");
    const id = d.dragId;
    d.dragId = null;
    if (!row || row.dataset.id === id) { renderList(); return; }
    const after = row.classList.contains("drop-after");
    const cs = d.settings.columns;
    const [c] = cs.splice(cs.findIndex((x) => x.id === id), 1);
    let j = cs.findIndex((x) => x.id === row.dataset.id);
    cs.splice(after ? j + 1 : j, 0, c);
    d.sel = id;
    changed();
  }
  function onDragEnd() {
    d.dragId = null;
    d.dragOk = false;
    $("cdList").querySelectorAll(".dragging,.drop-before,.drop-after").forEach((x) => x.classList.remove("dragging", "drop-before", "drop-after"));
  }

  // ------------------------------------------------------------ footer
  function addCalc() {
    const cs = d.settings.columns;
    let n = 1;
    while (cs.some((c) => c.id === "c:" + n)) n++;
    let name = "新しい列", k = 2;
    while (cs.some((c) => c.source === "calc" && V.title(c) === name)) name = "新しい列" + k++;
    const c = newCol("c:" + n, "calc", "", true);
    c.label = name;
    c.expr = "[ロットNo]";
    const i = cs.findIndex((x) => x.id === d.sel);
    cs.splice(i >= 0 ? i + 1 : cs.length, 0, c);
    d.sel = c.id;
    d.mode = "col";
    d.open.expr = true;
    d.chip = d.chip === "off" ? "all" : d.chip;
    changed();
    scrollToSel();
    const ta = document.querySelector('[data-fk="expr"]');
    if (ta) { ta.focus(); ta.select(); }
  }

  function exportSettings() {
    const blob = new Blob([JSON.stringify(d.settings, null, 2)], { type: "application/json" });
    const a = document.createElement("a");
    const t = new Date();
    const p2 = (n) => String(n).padStart(2, "0");
    a.download = "cm3_view_settings_" + t.getFullYear() + p2(t.getMonth() + 1) + p2(t.getDate()) + ".json";
    a.href = URL.createObjectURL(blob);
    document.body.appendChild(a);
    a.click();
    setTimeout(() => { URL.revokeObjectURL(a.href); a.remove(); }, 1000);
  }

  function importSettings(file) {
    const fr = new FileReader();
    fr.onload = () => {
      try {
        const o = JSON.parse(fr.result);
        if (!o || !Array.isArray(o.columns) || !o.columns.length) throw new Error("列の設定が見つかりません");
        d.settings = o;
        completeColumns();
        d.sel = d.settings.columns[0].id;
        changed();
        $("cdMsg").textContent = "読み込みました（まだ保存していません）";
      } catch (e) {
        alert("読み込めませんでした: " + e.message);
      }
    };
    fr.readAsText(file, "utf-8");
  }

  async function resetDefault() {
    if (!confirm("列の設定を既定に戻します（保存するまで反映されません）。よろしいですか？")) return;
    const res = await fetch("/api/view/default", { cache: "no-store" });
    const j = await res.json();
    d.settings = j.settings;
    completeColumns();
    d.sel = d.settings.columns[0].id;
    changed();
    $("cdMsg").textContent = "既定の設定を表示しています（保存すると反映されます）";
  }

  async function save() {
    const bad = d.settings.columns.filter((c) => c.source === "calc" && vcol(c.id)._cmp && !vcol(c.id)._cmp.ok);
    if (bad.length && !confirm("式に誤りのある列があります: " + bad.map((c) => V.title(c)).join("、") + "\n（その列は空欄になります）\n保存しますか？")) return;
    const names = d.settings.columns.filter((c) => c.source === "calc").map((c) => V.normName(V.title(c)));
    if (new Set(names).size !== names.length) { alert("計算列の名前が重複しています。名前を変えてください。"); return; }
    const j = await App().postJson("/api/view/settings", d.settings);
    if (!j.success) { $("cdMsg").textContent = "保存できませんでした: " + j.message; return; }
    d.dirty = false;
    clearTimeout(d.timer);
    $("dlgCols").hidden = true;
    App().setSaved(j.settings);
  }

  // ------------------------------------------------------------ wiring
  $("btnCols").addEventListener("click", open);
  $("cdClose").addEventListener("click", () => close(false));
  $("cdCancel").addEventListener("click", () => close(false));
  $("cdSave").addEventListener("click", save);
  $("cdDefault").addEventListener("click", resetDefault);
  $("cdAddCalc").addEventListener("click", addCalc);
  $("cdAutoWidth").addEventListener("click", () => { d.settings.columns.forEach((c) => { c.width = null; }); changed(); });
  $("cdFilter").addEventListener("input", renderList);
  $("cdChips").addEventListener("click", (e) => {
    const b = e.target.closest("[data-chip]");
    if (!b) return;
    d.chip = b.dataset.chip;
    renderList();
  });
  $("cdAll").addEventListener("change", (e) => {
    const cols = listCols();
    cols.forEach((c) => { c.visible = e.target.checked; });
    if (!d.settings.columns.some((x) => x.visible !== false)) cols[0].visible = true;
    changed();
  });
  $("cdOpenRules").addEventListener("click", () => { d.mode = "rules"; renderRight(); });
  $("cdOpenMaps").addEventListener("click", () => { d.mode = "maps"; renderRight(); });

  const list = $("cdList");
  list.addEventListener("click", onListClick);
  list.addEventListener("change", onListChange);
  list.addEventListener("input", onListInput);
  list.addEventListener("mousedown", (e) => { d.dragOk = !!e.target.closest(".handle"); });
  list.addEventListener("dragstart", onDragStart);
  list.addEventListener("dragover", onDragOver);
  list.addEventListener("drop", onDrop);
  list.addEventListener("dragend", onDragEnd);

  const right = $("cdRight");
  // text / textarea / number / color: while typing ("input")
  // select / checkbox / renaming a conversion table: when committed ("change")
  const isTyping = (el) => el.tagName === "TEXTAREA" || el.type === "text" || el.type === "number" || el.type === "search";
  right.addEventListener("input", (e) => {
    const el = e.target;
    if (el.type === "color") { onColor(el); return; }
    if (isTyping(el) && el.dataset.fk !== "mapname") onRightInput(e);
  });
  right.addEventListener("change", (e) => {
    const el = e.target;
    if (el.type === "color") { renderRight(); return; }
    if (isTyping(el) && el.dataset.fk !== "mapname") return;
    onRightInput(e);
  });
  right.addEventListener("click", onRightClick);

  $("cdFileBtn").addEventListener("click", (e) => { e.stopPropagation(); $("cdFileMenu").hidden = !$("cdFileMenu").hidden; });
  document.addEventListener("click", () => { $("cdFileMenu").hidden = true; });
  $("cdFileMenu").addEventListener("click", (e) => {
    const b = e.target.closest("[data-act]");
    if (!b) return;
    if (b.dataset.act === "export") exportSettings();
    if (b.dataset.act === "import") $("cdImport").click();
  });
  $("cdImport").addEventListener("change", (e) => {
    if (e.target.files[0]) importSettings(e.target.files[0]);
    e.target.value = "";
  });
  $("dlgCols").addEventListener("click", (e) => { if (e.target === $("dlgCols")) close(false); });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !$("dlgCols").hidden) close(false);
  });
})();
