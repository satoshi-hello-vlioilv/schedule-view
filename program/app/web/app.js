(function () {
  "use strict";

  const $ = (id) => document.getElementById(id);
  const V = window.CM3View;
  const X = window.CM3Expr;
  const STORE_KEY = "cm3viewer.setsubi";
  const HOT_CODE = "__HOT__";
  const tabs = Array.from(document.querySelectorAll("#equipTabs .tab"));
  const codes = tabs.map((t) => t.dataset.code);
  const saved = localStorage.getItem(STORE_KEY);
  const urlSetsubi = new URLSearchParams(location.search).get("setsubi");

  const state = {
    setsubi: [urlSetsubi, saved, window.APP.defaultSetsubi, codes[0]].find((c) => c && codes.includes(c)),
    reqId: 0,
    data: null,
    viewSettings: window.APP.view,   // saved settings
    draft: null,                      // settings being edited in the designer (live preview)
    view: null,
    sortKey: "s:seq",
    sortDir: 1,
    nextFilter: new Set(),
    hotEq: new Set(),                 // HOT list: equipments hidden
    hotKama: new Set(),               // HOT list: 釜番 selected (empty = all)
    hotTl: localStorage.getItem("cm3viewer.hotTl") !== "0",
    mode: null,                       // "line" / "hot"
    timer: null,
    shown: []                         // rows currently shown (for CSV / Excel)
  };

  const isHot = () => state.setsubi === HOT_CODE;
  const hotInfo = () => (state.data && state.data.hot) || {};

  // equipment colors and 釜番 colors
  const EQ_COLORS = { "L-1": "#2563eb", "L-2": "#16a34a", "L-3": "#ea580c" };
  const EQ_FALLBACK = ["#7c3aed", "#0891b2", "#be123c", "#4d7c0f"];
  function eqColor(eq) {
    if (EQ_COLORS[eq]) return EQ_COLORS[eq];
    const list = (window.APP.hot.equipments || []);
    const i = Math.max(0, list.indexOf(eq));
    return EQ_FALLBACK[i % EQ_FALLBACK.length];
  }
  const KAMA_PALETTE = ["#ffd6a5", "#caffbf", "#9bf6ff", "#bdb2ff", "#ffc6ff", "#fdffb6", "#a0c4ff", "#ffadad",
    "#d0f4de", "#e4c1f9", "#fcf6bd", "#a9def9", "#f1c0e8", "#cfbaf0", "#b9fbc0", "#fbe0c3"];
  let kamaColorMap = new Map();
  function kamaKey(v) { return String(v == null ? "" : v).trim(); }
  function kamaColor(v) { const k = kamaKey(v); return k ? (kamaColorMap.get(k) || "#e5e7eb") : "#e5e7eb"; }

  function esc(s) {
    return String(s == null ? "" : s)
      .replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;").replace(/'/g, "&#39;");
  }
  function fill(tpl, obj) {
    return tpl.replace(/\{(\w+)\}/g, (_, k) => encodeURIComponent(obj[k] == null ? "" : obj[k]));
  }
  function show(id, text) {
    const m = $(id);
    if (text) { m.textContent = text; m.hidden = false; } else { m.hidden = true; m.textContent = ""; }
  }
  async function postJson(url, body) {
    const res = await fetch(url, {
      method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body || {})
    });
    return res.json();
  }

  function applyColors(colors) {
    const root = document.documentElement.style;
    Object.keys(colors).forEach((k) => root.setProperty("--c-" + k.replace(/_/g, "-"), colors[k]));
  }

  // ------------------------------------------------------------ view
  function rebuildView() {
    const masterAll = (state.data && state.data.masterAll) || [];
    state.view = V.build(state.draft || state.viewSettings, masterAll);
  }

  function colFor(field) {
    const c = state.view.byId["s:" + field];
    if (c) return Object.assign({}, c, { visible: true });
    const f = window.APP.scheduleColumns.find((x) => x.key === field);
    return V.extraCol(field, f ? f.label : field);
  }

  /** columns shown. HOT list: 順・設備・開始予定・完了予定 first, then the user's visible columns */
  function visibleCols() {
    const cols = state.view.cols.filter((c) => c.visible !== false);
    if (!isHot()) return cols;
    const lead = [V.extraCol("hotSeq", "順"), colFor("setsubi"), colFor("startYotei"), colFor("kanryoYotei")];
    const skip = new Set(["s:seq", "s:setsubi", "s:startYotei", "s:kanryoYotei"]);
    return lead.concat(cols.filter((c) => !skip.has(c.id)));
  }
  function colById(id) { return visibleCols().find((c) => c.id === id) || state.view.byId[id]; }
  function isKamaCol(c) {
    const k = hotInfo().kamaColumn;
    return !!k && c.source === "master" && c.field === k;
  }

  // ------------------------------------------------------------ equipment / HOT tab
  function selectEquipment(code) {
    const prevMode = state.mode;
    state.setsubi = code;
    state.mode = code === HOT_CODE ? "hot" : "line";
    localStorage.setItem(STORE_KEY, code);
    tabs.forEach((t) => {
      const on = t.dataset.code === code;
      t.classList.toggle("active", on);
      t.setAttribute("aria-selected", on ? "true" : "false");
      if (on && code !== HOT_CODE) {
        $("lnkCm3").href = fill(window.APP.cm3ViewUrlTemplate, { setsubi: code, termkbn: t.dataset.termkbn });
      }
    });
    const hot = isHot();
    $("lnkCm3").hidden = hot;
    $("lineSummary").hidden = hot;
    $("nextFilters").hidden = hot;
    $("hotPanel").hidden = !hot;
    if (state.mode !== prevMode) {
      state.sortKey = hot ? "s:kanryoYotei" : "s:seq";
      state.sortDir = 1;
    }
    document.title = hot ? window.APP.hot.title : "CM3 スケジュール " + code;
  }

  // ------------------------------------------------------------ load
  async function load(force) {
    const myId = ++state.reqId;
    $("btnRefresh").disabled = true;
    const setsubi = state.setsubi;
    const label = isHot() ? window.APP.hot.title : setsubi;
    $("state").textContent = label + " 取得中...";
    try {
      const url = isHot()
        ? "/api/hot" + (force ? "?force=1" : "")
        : "/api/cm3/schedule?setsubi=" + encodeURIComponent(setsubi) + (force ? "&force=1" : "");
      const res = await fetch(url, { cache: "no-store" });
      const json = await res.json();
      if (myId !== state.reqId) return;
      if (!json.success) throw new Error(json.message || "取得に失敗しました");
      state.data = json;
      const m = json.master || {};
      const errs = (json.hot && json.hot.errors) || [];
      show("message", errs.length ? "一部の設備を取得できませんでした: " + errs.join(" / ") : "");
      show("masterMsg", m.loading ? "台帳を読み込んでいます（読み終わると自動で表示します）" : (m.message ? "台帳: " + m.message : ""));
      // 台帳は exe が裏で読む。読み終わる前に開いた一覧は、少し待って読み直す（台帳の列が付く）
      if (m.loading) setTimeout(() => { if (myId === state.reqId) load(false); }, 1500);
      rebuildView();
      if (isHot()) buildHotPanel(); else buildNextFilters();
      renderAll();
      $("state").textContent = label + " 取得成功 " + json.fetchedAt +
        (m.keyLabel ? "　台帳キー: " + m.keyLabel : "");
    } catch (e) {
      if (myId !== state.reqId) return;
      const keep = state.data && state.data.setsubi === setsubi;
      if (!keep) { state.data = null; $("nextFilters").innerHTML = ""; rebuildView(); renderAll(); }
      show("message", label + " のデータを取得できませんでした: " + e.message +
        (keep ? "（前回取得したデータを表示しています）" : ""));
      $("state").textContent = label + " 取得失敗";
    } finally {
      if (myId === state.reqId) $("btnRefresh").disabled = false;
    }
  }

  // ------------------------------------------------------------ table
  function colAlign(c, text) {
    if (c.align && c.align !== "auto") return c.align;
    if (c.source === "schedule" && c.field === "setsubi") return "center";
    if (c._numeric || (c.format && c.format.type === "number")) return "right";
    return text != null && text !== "" && V.isNumLike(text) ? "right" : "left";
  }
  function cellStyle(c, extra) {
    let s = "";
    if (c.width) s += "width:" + c.width + "px;min-width:" + c.width + "px;max-width:" + c.width + "px;";
    if (c.bold) s += "font-weight:700;";
    return s + (extra || "");
  }

  function buildHead() {
    const cols = visibleCols();
    if (!cols.some((c) => c.id === state.sortKey)) { state.sortKey = cols.length ? cols[0].id : ""; state.sortDir = 1; }
    $("headRow").innerHTML = cols.map((c) => {
      const tip = c.source === "master" ? "台帳: " + c.field : c.source === "calc" ? "計算列: " + (c.expr || "") : "スケジュール";
      const al = colAlign(c, null);
      return '<th data-key="' + esc(c.id) + '" class="al-' + al + (c.width ? " fixw" : "") + '" style="' + cellStyle(c) +
        '" title="' + esc(tip) + '">' + esc(c._title) + "</th>";
    }).join("");
  }

  function buildNextFilters() {
    const box = $("nextFilters");
    const counts = {};
    state.data.items.forEach((it) => { if (!it.isStop) counts[it.jiKotei] = (counts[it.jiKotei] || 0) + 1; });
    [...state.nextFilter].forEach((k) => { if (!counts[k]) state.nextFilter.delete(k); });
    box.innerHTML = '<span class="field">次工程</span>' + Object.keys(counts).sort().map((k) =>
      '<button type="button" class="chip' + (state.nextFilter.has(k) ? " on" : "") + '" data-k="' + esc(k) + '">' +
      esc(k || "(空)") + '<span class="n">' + counts[k] + "</span></button>").join("");
  }

  /** compute all visible cells of all rows once */
  function computeRows() {
    const items = state.data ? state.data.items : [];
    const cols = visibleCols();
    const sortCol = colById(state.sortKey);
    return items.map((it) => {
      const memo = new Map();
      const cells = {};
      if (it.rowKind !== "stop") cols.forEach((c) => { cells[c.id] = V.cell(state.view, c, it, memo); });
      const rs = it.rowKind === "lot" ? V.applyRules(state.view, it, memo) : null;
      let sv = null;
      if (sortCol) {
        if (it.rowKind === "stop") {
          sv = sortCol.source === "schedule" && sortCol.field === "seq" ? it.seq
            : sortCol.source === "schedule" && sortCol.field === "kanryoYotei" ? (it.kanryoYoteiIso || "") : null;
        } else {
          sv = V.sortValue(state.view, sortCol, it, memo);
        }
      }
      return { it: it, cells: cells, rs: rs, sv: sv };
    });
  }

  function filtered(rows) {
    const q = $("search").value.trim().toLowerCase();
    if (isHot()) {
      if (state.hotEq.size) rows = rows.filter((r) => !state.hotEq.has(r.it.setsubi));
      if (state.hotKama.size) rows = rows.filter((r) => state.hotKama.has(kamaKey(r.it.kama)));
    } else if (state.nextFilter.size) {
      rows = rows.filter((r) => r.it.isStop || state.nextFilter.has(r.it.jiKotei));
    }
    if (q) {
      rows = rows.filter((r) => {
        const it = r.it;
        const parts = [it.stopText, it.kanryoYotei, it.kama || "", it.rowKind === "inspect" ? "点検" : "", it.rowKind === "stop" ? "設備停止" : ""];
        Object.keys(r.cells).forEach((k) => parts.push(r.cells[k].text));
        return parts.join(" ").toLowerCase().includes(q);
      });
    }
    const d = state.sortDir;
    rows.sort((a, b) => {
      const x = a.sv, y = b.sv;
      if (x === y) return (a.it.hotSeq || a.it.seq) - (b.it.hotSeq || b.it.seq);
      if (x == null) return 1;
      if (y == null) return -1;
      if (typeof x !== typeof y) return String(x) < String(y) ? -d : d;
      return x < y ? -d : d;
    });
    return rows;
  }

  function td(c, text, html, extraCls, extraStyle) {
    const al = colAlign(c, text);
    const t = c.width && text ? ' title="' + esc(text) + '"' : "";
    return '<td class="al-' + al + (extraCls ? " " + extraCls : "") +
      (c.width ? " fixw" : "") + '" style="' + cellStyle(c, extraStyle) + '"' + t + ">" + (html != null ? html : esc(text)) + "</td>";
  }

  function rowHtml(r, cols, brk) {
    const it = r.it;
    const tpl = window.APP.lotUrlTemplate;

    if (it.rowKind === "stop") {
      let lead = 0;
      while (lead < cols.length && cols[lead].source === "schedule" && ["seq", "kanryoYotei"].includes(cols[lead].field)) lead++;
      const shownLead = new Set(cols.slice(0, lead).map((c) => c.field));
      let h = cols.slice(0, lead).map((c) => {
        const v = c.field === "seq" ? it.seq : it.kanryoYotei;
        return td(c, String(v));
      }).join("");
      const pre = shownLead.has("kanryoYotei") ? "" : esc(it.kanryoYotei) + "　";
      const span = Math.max(1, cols.length - lead);
      h += '<td colspan="' + span + '">' + pre + '<span class="stop-tag">設備停止</span>' + esc(it.stopText) + "</td>";
      return '<tr class="stop' + brk + '">' + h + "</tr>";
    }

    const isLot = it.rowKind === "lot";
    let firstMaster = true;
    const h = cols.map((c) => {
      const cl = r.cells[c.id] || { text: "" };
      const cs = r.rs && r.rs.cells[c.id] ? V.styleText(r.rs.cells[c.id]) : "";
      if (c.source === "schedule" && c.field === "setsubi") {
        return td(c, cl.text, '<span class="eq-badge" style="background:' + eqColor(it.setsubi) + '">' + esc(cl.text) + "</span>", "", cs);
      }
      if (c.source === "master") {
        if (!isLot) return td(c, "", "", "", cs);
        if (it.mHit === false) {
          const html = firstMaster ? '<span class="nohit">台帳に該当なし</span>' : "";
          firstMaster = false;
          return td(c, "", html, "", cs);
        }
        firstMaster = false;
        if (isHot() && isKamaCol(c)) {
          return td(c, cl.text, '<span class="kama-dot" style="background:' + kamaColor(it.kama) + '"></span>' + esc(cl.text), "", cs);
        }
        return td(c, cl.text, null, "", cs);
      }
      if (c.source === "schedule" && c.field === "lotNo") {
        if (it.rowKind === "inspect") return td(c, cl.text, '<span class="inspect-tag">点検</span>' + esc(cl.text), "", cs);
        const link = tpl ? '<a href="' + esc(fill(tpl, it)) + '" target="_blank" rel="noopener">' + esc(cl.text) + "</a>" : esc(cl.text);
        return td(c, cl.text, link, "", cs);
      }
      if (c.source === "schedule" && c.field === "passNo" && (!c.format || c.format.type === "none") && !c.map) {
        if (!isLot) return td(c, "-", null, "", cs);
        const html = it.passTotal > 1 ? it.passNo + '<span class="pass">/' + it.passTotal + "</span>" : "1";
        return td(c, String(it.passNo), html, "", cs);
      }
      if (c.source === "schedule" && c.field === "jiKotei") return td(c, cl.text, null, "next", cs);
      return td(c, cl.text, null, "", cs);
    }).join("");

    const cls = it.rowKind === "inspect" ? "inspect" : esc(it.rowStyle || "") + (it.selfNext ? " self" : "");
    const rstyle = r.rs && r.rs.row ? ' style="' + V.styleText(r.rs.row) + '"' : "";
    return '<tr class="' + cls + brk + (rstyle ? " ruled" : "") + '"' + rstyle + ">" + h + "</tr>";
  }

  function render() {
    const cols = visibleCols();
    const rows = filtered(computeRows());
    state.shown = rows;
    const sc = colById(state.sortKey);
    const timeSort = sc && sc.source === "schedule" && ["seq", "kanryoYotei", "hotSeq", "startYotei"].includes(sc.field);
    let prevDate = null;
    $("rows").innerHTML = rows.map((r) => {
      const date = (r.it.kanryoYotei || "").slice(0, 5);
      const brk = timeSort && state.sortDir > 0 && prevDate && date !== prevDate ? " date-break" : "";
      prevDate = date;
      return rowHtml(r, cols, brk);
    }).join("");

    $("empty").hidden = rows.length > 0;
    $("empty").textContent = isHot() && state.data && !state.data.items.length
      ? "前工程が " + (hotInfo().prevValue || "HOT") + " のロットはありません" : "表示するデータがありません";

    if (isHot()) renderHotPanel(rows);
    else renderLineSummary(rows);

    document.querySelectorAll("#headRow th").forEach((th) => {
      th.classList.remove("sorted-asc", "sorted-desc");
      if (th.dataset.key === state.sortKey) th.classList.add(state.sortDir > 0 ? "sorted-asc" : "sorted-desc");
    });
  }

  function renderLineSummary(rows) {
    const d = state.data;
    const items = rows.map((r) => r.it);
    const prod = items.filter((i) => i.rowKind === "lot");
    const bySeq = items.slice().sort((a, b) => a.seq - b.seq);
    const ms = (d && d.master) || {};
    $("sumCount").textContent = d ? prod.length + " / " + d.count : "-";
    $("sumLots").textContent = d ? new Set(prod.map((i) => i.lotNo)).size : "-";
    $("sumInspect").textContent = d ? items.filter((i) => i.rowKind === "inspect").length : "-";
    $("sumStop").textContent = d ? items.filter((i) => i.rowKind === "stop").length : "-";
    $("sumHit").textContent = d ? (ms.keyLabel ? prod.filter((i) => i.mHit).length + " / " + prod.length : "未接続") : "-";
    $("sumRange").textContent = bySeq.length ? bySeq[0].kanryoYotei + " ～ " + bySeq[bySeq.length - 1].kanryoYotei : "-";
    $("sumUpdate").textContent = d ? (d.updateTime || "-") + " / " + d.fetchedAt + " / " + (ms.created || "-") : "-";
  }

  function renderAll() { buildHead(); render(); }

  // ------------------------------------------------------------ HOT panel
  function buildHotPanel() {
    // 釜番 colors in order of first appearance (by time)
    kamaColorMap = new Map();
    const items = state.data.items.slice().sort((a, b) => String(a.kanryoYoteiIso || "9").localeCompare(String(b.kanryoYoteiIso || "9")));
    items.forEach((it) => {
      const k = kamaKey(it.kama);
      if (k && !kamaColorMap.has(k)) kamaColorMap.set(k, KAMA_PALETTE[kamaColorMap.size % KAMA_PALETTE.length]);
    });
    const exist = new Set(state.data.items.map((it) => kamaKey(it.kama)));
    [...state.hotKama].forEach((k) => { if (!exist.has(k)) state.hotKama.delete(k); });
  }

  function timeRange(list) {
    const t = list.map((it) => it.kanryoYotei).filter((x) => x);
    return t.length ? t[0] + " ～ " + t[t.length - 1] : "-";
  }

  function renderHotPanel(rows) {
    const d = state.data;
    const h = hotInfo();
    if (!d) { $("hotCards").innerHTML = ""; $("hotTl").innerHTML = ""; $("hotKama").innerHTML = ""; return; }
    const all = d.items;
    const eqs = h.equipments || window.APP.hot.equipments;

    // cards
    let cards = '<div class="card"><div class="label">対象ロット（前工程 ' + esc(h.prevValue || "HOT") + '）</div><div class="value">' +
      rows.length + (rows.length !== all.length ? ' <span style="font-size:13px;color:var(--muted)">/ ' + all.length + "</span>" : "") + "</div>" +
      '<div class="sub">' + esc(timeRange(rows.map((r) => r.it))) + "</div></div>";
    eqs.forEach((eq) => {
      const list = all.filter((it) => it.setsubi === eq);
      cards += '<div class="card eq' + (state.hotEq.has(eq) ? " off" : "") + '" data-eq="' + esc(eq) + '" style="--eqc:' + eqColor(eq) +
        '" title="押すと表示／非表示を切り替えます"><div class="label"><span class="eq-badge" style="background:' + eqColor(eq) + '">' + esc(eq) +
        '</span></div><div class="value">' + list.length + ' <span style="font-size:13px;color:var(--muted)">本</span></div>' +
        '<div class="sub">' + esc(timeRange(list)) + "</div></div>";
    });
    const kamaN = new Set(all.map((it) => kamaKey(it.kama)).filter((k) => k)).size;
    const c = h.counts || {};
    const notes = [];
    if (!h.kamaColumn) notes.push("台帳に「" + (h.kamaConfigured || "設計_PIT炉番炉順") + "」列が無いため、釜番は空欄です");
    if (!h.resultColumn) notes.push("台帳に「実績_設備ｺｰｽ」列が無いため、前工程を判定できません");
    if (c.noMaster) notes.push("台帳に該当しないロット " + c.noMaster + " 件は前工程が不明のため除外");
    if (c.otherNext) notes.push("前工程HOTでも次の設備が別のロット " + c.otherNext + " 件は除外");
    cards += '<div class="card"><div class="label">釜番</div><div class="value">' + kamaN + "</div></div>" +
      '<div class="card info"><div class="label">抽出条件</div><div class="value">' +
      esc(eqs.join("・")) + " のスケジュールから、前工程（実績_設備ｺｰｽの最後）が " + esc(h.prevValue || "HOT") +
      " で、次の設備（残仕掛設備ｺｰｽの最初）がその設備のロットの1パス目。完了予定の順。" +
      (notes.length ? "<br>" + esc(notes.join("／")) : "") + "</div></div>";
    $("hotCards").innerHTML = cards;

    renderTimeline(all, eqs);

    // 釜番 chips (order of first appearance)
    const groups = new Map();
    all.slice().sort((a, b) => String(a.kanryoYoteiIso || "9").localeCompare(String(b.kanryoYoteiIso || "9"))).forEach((it) => {
      const k = kamaKey(it.kama);
      const g = groups.get(k) || { n: 0, eq: {}, list: [] };
      g.n++;
      g.eq[it.setsubi] = (g.eq[it.setsubi] || 0) + 1;
      g.list.push(it);
      groups.set(k, g);
    });
    $("hotKama").innerHTML = (groups.size ? '<button type="button" class="kchip' + (state.hotKama.size ? "" : " on") + '" data-k="__all">すべて</button>' : "") +
      Array.from(groups.entries()).map(([k, g]) => {
        const eqText = Object.keys(g.eq).map((e) => e + (g.eq[e] > 1 ? "×" + g.eq[e] : "")).join(" ");
        return '<button type="button" class="kchip' + (state.hotKama.has(k) ? " on" : "") + '" data-k="' + esc(k) + '" title="' +
          esc(timeRange(g.list)) + '"><span class="kama-dot" style="background:' + kamaColor(k) + '"></span>' +
          esc(k || "（釜番なし）") + '<span class="n">' + g.n + '</span><span class="eqs">' + esc(eqText) + "</span></button>";
      }).join("");
  }

  // CM3 times are plant local time (JST): use the wall-clock part, ignore the offset
  function parseIso(s) { return s ? new Date(String(s).slice(0, 19)).getTime() : NaN; }

  function renderTimeline(all, eqs) {
    const box = $("hotTl");
    box.hidden = !state.hotTl;
    $("hotTlToggle").textContent = state.hotTl ? "図を隠す" : "図を表示";
    if (!state.hotTl) return;
    const blocks = [];
    all.forEach((it) => {
      const end = parseIso(it.kanryoYoteiIso);
      if (isNaN(end)) return;
      let start = parseIso(it.startYoteiIso);
      if (isNaN(start) || start >= end) start = end - 10 * 60000;
      blocks.push({ it: it, s: start, e: end });
    });
    if (!blocks.length) { box.innerHTML = '<div class="note">表示できる予定がありません</div>'; return; }
    let t0 = Math.min.apply(null, blocks.map((b) => b.s));
    let t1 = Math.max.apply(null, blocks.map((b) => b.e));
    const pad = Math.max(10 * 60000, (t1 - t0) * 0.01);
    t0 -= pad; t1 += pad;
    const span = t1 - t0;
    const pct = (t) => ((t - t0) / span * 100).toFixed(3) + "%";
    const p2 = (n) => String(n).padStart(2, "0");

    // axis: every 1/2/3/6 hours depending on the span
    const hours = span / 3600000;
    const step = hours <= 12 ? 1 : hours <= 24 ? 2 : hours <= 48 ? 3 : 6;
    const first = new Date(t0); first.setMinutes(0, 0, 0); first.setHours(first.getHours() + 1);
    const ticks = [];
    for (let t = first.getTime(); t < t1; t += 3600000) {
      const dt = new Date(t);
      if (dt.getHours() % step) continue;
      ticks.push({ t: t, day: dt.getHours() === 0, label: dt.getHours() === 0 ? (dt.getMonth() + 1) + "/" + dt.getDate() : p2(dt.getHours()) + ":00" });
    }
    let h = '<div class="tl-axis">' + ticks.map((k) => '<span class="' + (k.day ? "day" : "") + '" style="left:' + pct(k.t) + '">' + k.label + "</span>").join("") + "</div>";
    const now = Date.now();
    eqs.forEach((eq, li) => {
      const grid = ticks.map((k) => '<div class="tl-grid' + (k.day ? " day" : "") + '" style="left:' + pct(k.t) + '"></div>').join("");
      const nowLine = now > t0 && now < t1 && li === 0 ? '<div class="tl-now" style="left:' + pct(now) + '"></div>' : "";
      const bl = blocks.filter((b) => b.it.setsubi === eq).map((b) => {
        const it = b.it;
        const k = kamaKey(it.kama);
        const dim = state.hotKama.size && !state.hotKama.has(k);
        const tip = eq + "　" + (it.startYotei || "?") + " ～ " + it.kanryoYotei + "\nロット " + it.lotNo + "\n釜番 " + (k || "なし") +
          "\n" + (it.zaishitsu || "") + " " + (it.inItaAtsu || "") + "→" + (it.outItaAtsu || "") + " × " + (it.reienHaba || "");
        return '<div class="tl-blk' + (dim ? " dim" : "") + '" data-k="' + esc(k) + '" title="' + esc(tip) + '" style="left:' + pct(b.s) +
          ";width:calc(" + ((b.e - b.s) / span * 100).toFixed(3) + "% - 1px);background:" + kamaColor(k) + '">' + esc(k || it.lotNo) + "</div>";
      }).join("");
      h += '<div class="tl-lane"><span class="ln"><span class="eq-badge" style="background:' + eqColor(eq) + '">' + esc(eq) +
        '</span></span><div class="tl-track">' + grid + nowLine + bl + "</div></div>";
    });
    box.innerHTML = h;
  }

  function toggleKama(k) {
    if (k === "__all") state.hotKama.clear();
    else if (state.hotKama.has(k)) state.hotKama.delete(k);
    else state.hotKama.add(k);
    render();
  }

  function resetTimer() {
    if (state.timer) clearInterval(state.timer);
    state.timer = null;
    if ($("autoRefresh").checked && window.APP.refreshSec > 0) {
      state.timer = setInterval(() => load(false), window.APP.refreshSec * 1000);
    }
  }

  // ------------------------------------------------------------ export (CSV / Excel) as shown
  const KIND = { lot: "製造", inspect: "点検", stop: "設備停止" };
  const RE_NUM = /^-?\d+(\.\d+)?$/;
  const RE_COMMA = /^-?\d{1,3}(,\d{3})+(\.\d+)?$/;
  const RE_PCT = /^-?\d+(\.\d+)?%$/;

  function decimalsOf(t) { const m = t.match(/\.(\d+)/); return m ? m[1].length : 0; }

  /** text shown on screen -> Excel cell (numbers stay numbers, with the same number of decimals) */
  function xCell(c, text) {
    if (text == null || text === "") return {};
    const t = String(text);
    const numCol = c._numeric || (c.format && c.format.type === "number");
    const dec = decimalsOf(t);
    const zeros = dec ? "." + "0".repeat(dec) : "";
    if (RE_PCT.test(t) && (numCol || c.source !== "schedule" || c.field === "reductionRate")) {
      return { n: parseFloat(t) / 100, fmt: "0" + zeros + "%" };
    }
    if (RE_COMMA.test(t) && (numCol || c.source !== "schedule")) {
      return { n: parseFloat(t.replace(/,/g, "")), fmt: "#,##0" + zeros };
    }
    // integers in master / formula columns (codes such as 5083, 0, 2) stay text unless formatted as numbers
    if (RE_NUM.test(t) && (numCol || (c.source !== "schedule" && dec > 0))) {
      return { n: parseFloat(t), fmt: dec ? "0" + zeros : "0" };
    }
    return { v: t };
  }

  function dispWidth(s) {
    let w = 0;
    for (const ch of String(s || "")) w += ch.charCodeAt(0) > 0xff && !/[\uff61-\uff9f]/.test(ch) ? 2 : 1;
    return w;
  }

  /** one model for CSV and Excel: [区分] + visible columns, in the order and filter shown */
  function exportModel() {
    const cols = visibleCols();
    const colors = window.APP.ui.colors;
    const headers = ["区分"].concat(cols.map((c) => c._title));
    const aligns = ["center"].concat(cols.map((c) => colAlign(c, null)));
    const maxW = headers.map((h) => dispWidth(h) + 3);
    const rows = state.shown.map((r) => {
      const it = r.it;
      const cells = [{ v: KIND[it.rowKind] }];
      const row = { kind: it.rowKind, stopText: it.stopText || "", cells: cells, styles: {} };

      if (it.rowKind === "stop") {
        let lead = 0;
        while (lead < cols.length && cols[lead].source === "schedule" && ["seq", "kanryoYotei"].includes(cols[lead].field)) lead++;
        cols.forEach((c, i) => {
          if (i < lead) cells.push(c.field === "seq" ? { n: it.seq, fmt: "0" } : { v: it.kanryoYotei });
          else if (i === lead) cells.push({ v: (cols.slice(0, lead).some((x) => x.field === "kanryoYotei") ? "" : it.kanryoYotei + "　") + "【設備停止】" + it.stopText });
          else cells.push({});
        });
        if (lead < cols.length - 1) row.merge = [lead + 1, cols.length];
        row.bg = colors.stop_bg; row.color = colors.stop_text; row.bold = true;
        row.texts = cells.map((x) => x.v != null ? x.v : x.n != null ? String(x.n) : "");
        return row;
      }

      let firstMaster = true;
      cols.forEach((c, i) => {
        const cl = r.cells[c.id] || { text: "" };
        let text = cl.text;
        let cell;
        if (c.source === "master" && it.rowKind !== "lot") { text = ""; cell = {}; }
        else if (c.source === "master" && it.mHit === false) {
          text = firstMaster ? "台帳に該当なし" : "";
          cell = text ? { v: text, color: "#9aa5b1" } : {};
          firstMaster = false;
        } else if (c.source === "schedule" && c.field === "lotNo" && it.rowKind === "inspect") {
          text = "【点検】" + text; cell = { v: text };
        } else if (c.source === "schedule" && c.field === "passNo" && (!c.format || c.format.type === "none") && !c.map) {
          if (it.rowKind !== "lot") { text = "-"; cell = { v: "-", align: "right" }; }
          else if (it.passTotal > 1) { text = it.passNo + "/" + it.passTotal; cell = { v: text, align: "right" }; }
          else { text = String(it.passNo); cell = { n: it.passNo, fmt: "0" }; }
        } else if (c.source === "schedule" && c.field === "setsubi") {
          cell = text ? { v: text, bg: eqColor(it.setsubi), color: "#ffffff", bold: true, align: "center" } : {};
        } else {
          if (c.source === "master") firstMaster = false;
          cell = xCell(c, text);
          if (isHot() && isKamaCol(c) && text) cell.bg = kamaColor(it.kama);
        }
        if (c.source === "schedule" && c.field === "jiKotei" && it.selfNext && it.rowKind === "lot") {
          cell.color = colors.self_next_text; cell.bold = true;
        }
        cells.push(cell);
        if (r.rs && r.rs.cells[c.id]) row.styles[String(i + 1)] = r.rs.cells[c.id];
        maxW[i + 1] = Math.max(maxW[i + 1], dispWidth(text) + 2);
      });

      if (it.rowKind === "inspect") { row.bg = colors.inspect_bg; row.color = colors.inspect_text; }
      else {
        const stripe = it.rowStyle === "cmschview-data-palegreen";
        row.bg = stripe ? colors.stripe_bg : (/^#?f{6}$/i.test(colors.normal_bg) ? "" : colors.normal_bg);
        if (r.rs && r.rs.row) {
          if (r.rs.row.bg) row.bg = r.rs.row.bg;
          if (r.rs.row.color) row.color = r.rs.row.color;
          if (r.rs.row.bold) row.bold = true;
        }
      }
      // keep the text exactly as shown for CSV
      row.texts = [cells[0].v];
      cols.forEach((c, i) => {
        const x = cells[i + 1];
        row.texts.push(x.v != null ? x.v : (x.n != null ? ((r.cells[c.id] || {}).text || String(x.n)) : ""));
      });
      return row;
    });
    const widths = cols.map((c, i) => c.width ? c.width / 7.5 : Math.min(50, maxW[i + 1]));
    return { cols: cols, headers: headers, aligns: aligns, widths: [6].concat(widths), rows: rows };
  }

  function fileKey() { return isHot() ? "HOT" : state.setsubi; }
  function stamp() {
    const t = new Date();
    const p2 = (n) => String(n).padStart(2, "0");
    return t.getFullYear() + p2(t.getMonth() + 1) + p2(t.getDate()) + "_" + p2(t.getHours()) + p2(t.getMinutes());
  }
  function download(blob, name) {
    const a = document.createElement("a");
    a.download = name;
    a.href = URL.createObjectURL(blob);
    document.body.appendChild(a);
    a.click();
    setTimeout(() => { URL.revokeObjectURL(a.href); a.remove(); }, 1000);
  }

  function exportCsv() {
    if (!state.data) return;
    const m = exportModel();
    const q = (s) => {
      s = String(s == null ? "" : s);
      return /[",\r\n]/.test(s) ? "\"" + s.replace(/"/g, "\"\"") + "\"" : s;
    };
    const lines = [m.headers.concat(["設備停止内容"]).map(q).join(",")];
    m.rows.forEach((r) => {
      let t = r.texts.slice();
      if (r.kind === "stop") t = t.map((x) => (String(x).startsWith("【設備停止】") ? "" : x));
      lines.push(t.concat([r.stopText]).map(q).join(","));
    });
    download(new Blob(["\uFEFF" + lines.join("\r\n")], { type: "text/csv" }), "cm3_" + fileKey() + "_" + stamp() + ".csv");
  }

  async function exportXlsx() {
    if (!state.data) return;
    const btn = $("btnXlsx");
    btn.disabled = true;
    $("state").textContent = "Excelを作成中...";
    try {
      const m = exportModel();
      const d = state.data;
      const title = isHot() ? window.APP.hot.title : state.setsubi;
      const body = {
        sheet: title,
        fileKey: fileKey(),
        headers: m.headers,
        widths: m.widths,
        aligns: m.aligns,
        info: "CM3 " + title + "　CM3更新 " + (d.updateTime || "-") + "　取得 " + d.fetchedAt,
        rows: m.rows.map((r) => ({ cells: r.cells, bg: r.bg || "", color: r.color || "", bold: !!r.bold, styles: r.styles, merge: r.merge || null }))
      };
      const res = await fetch("/api/export/xlsx", {
        method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body)
      });
      if (!res.ok) {
        let msg = res.status + "";
        try { msg = (await res.json()).message; } catch (e) { /* not json */ }
        throw new Error(msg);
      }
      download(await res.blob(), "cm3_" + fileKey() + "_" + stamp() + ".xlsx");
      $("state").textContent = "Excelを出力しました（" + m.rows.length + " 行）";
    } catch (e) {
      show("message", "Excelを出力できませんでした: " + e.message);
      $("state").textContent = "Excel出力失敗";
    } finally {
      btn.disabled = false;
    }
  }

  // ------------------------------------------------------------ master dialog
  const dlg = { info: null, keys: [] };
  const FIELDS = window.APP.scheduleFields;

  function fillMasterInfo(info) {
    $("mSource").textContent = info.source || "-";
    $("mLocal").textContent = info.local || "-";
    const st = info.status || {};
    $("mState").textContent = (st.state || "-") + (st.message ? "　" + st.message : "");
    $("mCreated").textContent = (info.meta && info.meta["作成日時"]) || "-";
    $("mCopied").textContent = (st.copiedAt || "-") + "（元ファイル更新 " + (st.sourceMtime || "-") + "）";
    $("mRows").textContent = info.rowCount + " 件　テーブル: " + (info.table || "-") +
      "　キー: " + (info.keyLabel || "未決定") +
      (info.duplicates ? "　（キー重複で除外 " + info.duplicates + " 件）" : "");
  }
  function currentTable() { return $("mTable").value || (dlg.info && dlg.info.autoTable) || ""; }

  function renderTableSelect(info) {
    const sel = (info.settings && info.settings.table) || "";
    $("mTable").innerHTML = '<option value="">自動（' + esc(info.autoTable || "-") + "）</option>" +
      Object.keys(info.tables || {}).map((t) => '<option value="' + esc(t) + '"' + (t === sel ? " selected" : "") + ">" +
        esc(t) + "</option>").join("");
  }

  function renderKeys() {
    const info = dlg.info;
    const cols = (info.tables && info.tables[currentTable()]) || [];
    const autoLabel = currentTable() === info.autoTable && info.autoKeyLabel
      ? "自動判定（" + info.autoKeyLabel + "）" : "自動判定";
    $("mKeys").innerHTML = dlg.keys.map((k, i) => {
      const sOpts = FIELDS.map((f) => '<option value="' + f.key + '"' + (f.key === k.schedule ? " selected" : "") + ">" +
        esc(f.label) + "</option>").join("");
      const mOpts = (i === 0 ? '<option value="">' + esc(autoLabel) + "</option>" : '<option value="">（選択してください）</option>') +
        cols.map((c) => '<option value="' + esc(c.name) + '"' + (c.name === k.master ? " selected" : "") + ">" +
          esc(c.label) + "</option>").join("");
      return "<tr><td>" + (i + 1) + '</td><td><select data-i="' + i + '" data-f="schedule">' + sOpts + "</select></td>" +
        '<td class="eq">＝</td><td><select data-i="' + i + '" data-f="master">' + mOpts + "</select></td>" +
        '<td><button type="button" class="btn small" data-del="' + i + '"' + (dlg.keys.length < 2 ? " disabled" : "") +
        ">削除</button></td></tr>";
    }).join("");
    $("mKeyResult").textContent = "";
  }

  function renderCandidates(info) {
    const lots = info.scheduleLots || 0;
    $("mCand").innerHTML = (info.candidates || []).map((c, i) => {
      const hit = c.scheduleHit == null ? "-" : c.scheduleHit + " / " + lots;
      return '<tr class="' + (i === 0 ? "best" : "") + '">' +
        '<td><button type="button" class="btn small" data-use="' + esc(c.table + "\u0000" + c.column) + '">使う</button></td>' +
        "<td>" + esc(c.table) + "</td>" +
        '<td title="' + esc(c.column) + '">' + esc(c.label) + "</td>" +
        "<td>" + (c.nameMatch ? '<span class="ok">一致</span>' : '<span class="ng">-</span>') + "</td>" +
        '<td class="num">' + c.patternRate + "%</td>" +
        '<td class="num">' + c.uniqueRate + "%</td>" +
        '<td class="num">' + (c.scheduleHit ? '<span class="ok">' + hit + "</span>" : hit) + "</td>" +
        "<td>" + esc((c.sample || []).join(", ")) + "</td></tr>";
    }).join("") || '<tr><td colspan="8" class="note">候補の列が見つかりません</td></tr>';
  }

  function keyBody() {
    return { table: $("mTable").value, keys: dlg.keys.map((k) => ({ schedule: k.schedule, master: k.master || "" })) };
  }

  async function openMaster() {
    $("dlg").hidden = false;
    dlg.info = null;
    $("mSave").disabled = true;
    ["mCand", "mKeys"].forEach((id) => { $(id).innerHTML = ""; });
    $("mMsg").textContent = "読込中...";
    try {
      const res = await fetch("/api/master", { cache: "no-store" });
      const info = await res.json();
      dlg.info = info;
      fillMasterInfo(info);
      renderTableSelect(info);
      dlg.keys = ((info.settings && info.settings.keys) || [{ schedule: "lotNo", master: "" }])
        .map((k) => ({ schedule: k.schedule, master: k.master || "" }));
      renderKeys();
      renderCandidates(info);
      $("mSave").disabled = false;
      $("mMsg").textContent = info.keyWarning || "";
    } catch (e) {
      $("mMsg").textContent = "台帳情報を取得できませんでした: " + e.message;
    }
  }

  async function reloadMaster() {
    $("mMsg").textContent = "再コピー・再読込中...";
    const info = await postJson("/api/master/reload");
    await openMaster();
    $("mMsg").textContent = "再読込しました（" + info.rowCount + " 件）";
    load(false);
  }

  async function testKeys() {
    $("mKeyResult").textContent = "確認中...";
    const r = await postJson("/api/master/test", keyBody());
    if (!r.success) { $("mKeyResult").innerHTML = '<span class="bad">' + esc(r.message) + "</span>"; return; }
    if (!r.total) { $("mKeyResult").textContent = "比較するスケジュールがありません（先に一覧を表示してください）"; return; }
    const rate = Math.round(r.hit / r.total * 100);
    $("mKeyResult").innerHTML = '<span class="' + (rate >= 80 ? "ok" : "bad") + '">一致 ' + r.hit + " / " + r.total +
      "（" + rate + "%）</span>　台帳のキー " + r.masterKeys + " 件" +
      (r.duplicates ? '　<span class="bad">キー重複 ' + r.duplicates + " 件（先頭の1件を使用）</span>" : "　重複なし") +
      (r.warning ? "　" + esc(r.warning) : "");
  }

  async function saveMaster() {
    if (!dlg.info) return;
    const json = await postJson("/api/master/settings", keyBody());
    if (!json.success) { $("mMsg").innerHTML = '<span class="bad">' + esc(json.message) + "</span>"; return; }
    $("dlg").hidden = true;
    load(false);
  }

  // ------------------------------------------------------------ color dialog
  const COLOR_DEFS = [
    { grp: "通常の行" },
    { key: "normal_bg", label: "背景" },
    { key: "stripe_bg", label: "背景（縞）" },
    { key: "self_next_text", label: "次工程が自設備の文字" },
    { key: "date_line", label: "日付の区切り線" },
    { grp: "点検の行（点検板など）" },
    { key: "inspect_bg", label: "背景" },
    { key: "inspect_text", label: "文字" },
    { key: "inspect_tag", label: "「点検」の印" },
    { grp: "設備停止の行" },
    { key: "stop_bg", label: "背景" },
    { key: "stop_text", label: "文字" },
    { key: "stop_tag", label: "「設備停止」の印" },
    { grp: "全体" },
    { key: "accent", label: "ボタン・リンク・選択中のタブ" }
  ];
  const ui = { colors: null };

  function renderColorInputs() {
    $("uiColors").innerHTML = COLOR_DEFS.map((d) => d.grp
      ? '<div class="grp">' + esc(d.grp) + "</div>"
      : "<label for=\"uc_" + d.key + "\">" + esc(d.label) + '</label><input type="color" id="uc_' + d.key +
        '" data-k="' + d.key + '" value="' + esc(ui.colors[d.key]) + '">').join("");
  }
  function openUi() {
    ui.colors = Object.assign({}, window.APP.ui.colors);
    renderColorInputs();
    $("uiKeywords").value = (window.APP.ui.inspect_keywords || []).join("\n");
    $("uiMsg").textContent = "";
    $("dlgUi").hidden = false;
  }
  function closeUi() {
    applyColors(window.APP.ui.colors);
    $("dlgUi").hidden = true;
  }
  async function saveUi() {
    const body = {
      colors: ui.colors,
      inspect_keywords: $("uiKeywords").value.split(/\r?\n/).map((s) => s.trim()).filter((s) => s)
    };
    const json = await postJson("/api/ui/settings", body);
    if (!json.success) { $("uiMsg").textContent = "保存できませんでした"; return; }
    window.APP.ui = json.settings;
    applyColors(json.settings.colors);
    $("dlgUi").hidden = true;
    load(true);
  }

  // ------------------------------------------------------------ events
  $("btnRefresh").addEventListener("click", () => load(true));
  $("equipTabs").addEventListener("click", (e) => {
    const t = e.target.closest(".tab");
    if (!t || t.dataset.code === state.setsubi) return;
    state.nextFilter.clear();
    state.data = null;
    $("rows").innerHTML = "";
    selectEquipment(t.dataset.code);
    rebuildView();
    buildHead();
    load(true);
  });
  $("search").addEventListener("input", render);
  $("autoRefresh").addEventListener("change", resetTimer);
  $("btnCsv").addEventListener("click", exportCsv);
  $("btnXlsx").addEventListener("click", exportXlsx);
  $("nextFilters").addEventListener("click", (e) => {
    const b = e.target.closest(".chip");
    if (!b) return;
    const k = b.dataset.k;
    if (state.nextFilter.has(k)) state.nextFilter.delete(k); else state.nextFilter.add(k);
    b.classList.toggle("on");
    render();
  });
  $("headRow").addEventListener("click", (e) => {
    const th = e.target.closest("th[data-key]");
    if (!th) return;
    const k = th.dataset.key;
    if (state.sortKey === k) state.sortDir = -state.sortDir; else { state.sortKey = k; state.sortDir = 1; }
    render();
  });

  // HOT panel
  $("hotCards").addEventListener("click", (e) => {
    const c = e.target.closest(".card.eq");
    if (!c) return;
    const eq = c.dataset.eq;
    if (state.hotEq.has(eq)) state.hotEq.delete(eq); else state.hotEq.add(eq);
    render();
  });
  $("hotKama").addEventListener("click", (e) => {
    const b = e.target.closest(".kchip");
    if (b) toggleKama(b.dataset.k);
  });
  $("hotTl").addEventListener("click", (e) => {
    const b = e.target.closest(".tl-blk");
    if (b) toggleKama(b.dataset.k);
  });
  $("hotTlToggle").addEventListener("click", () => {
    state.hotTl = !state.hotTl;
    localStorage.setItem("cm3viewer.hotTl", state.hotTl ? "1" : "0");
    render();
  });

  $("btnMaster").addEventListener("click", openMaster);
  $("mReload").addEventListener("click", reloadMaster);
  $("mSave").addEventListener("click", saveMaster);
  $("mKeyTest").addEventListener("click", testKeys);
  $("mKeyAdd").addEventListener("click", () => {
    const used = new Set(dlg.keys.map((k) => k.schedule));
    const next = FIELDS.find((f) => !used.has(f.key)) || FIELDS[0];
    dlg.keys.push({ schedule: next.key, master: "" });
    renderKeys();
  });
  $("mKeys").addEventListener("change", (e) => {
    const s = e.target;
    if (!s.dataset.f) return;
    dlg.keys[+s.dataset.i][s.dataset.f] = s.value;
    $("mKeyResult").textContent = "";
  });
  $("mKeys").addEventListener("click", (e) => {
    const b = e.target.closest("[data-del]");
    if (!b || dlg.keys.length < 2) return;
    dlg.keys.splice(+b.dataset.del, 1);
    renderKeys();
  });
  $("mTable").addEventListener("change", () => {
    dlg.keys = dlg.keys.map((k) => ({ schedule: k.schedule, master: "" }));
    renderKeys();
  });
  $("mCand").addEventListener("click", (e) => {
    const b = e.target.closest("[data-use]");
    if (!b) return;
    const [t, c] = b.dataset.use.split("\u0000");
    if (t !== currentTable()) { $("mTable").value = t; dlg.keys = [{ schedule: "lotNo", master: c }]; }
    else dlg.keys[0] = { schedule: "lotNo", master: c };
    renderKeys();
  });

  $("btnUi").addEventListener("click", openUi);
  $("uiSave").addEventListener("click", saveUi);
  $("uiReset").addEventListener("click", () => {
    ui.colors = Object.assign({}, window.APP.uiDefault.colors);
    renderColorInputs();
    applyColors(ui.colors);
    $("uiKeywords").value = window.APP.uiDefault.inspect_keywords.join("\n");
    $("uiMsg").textContent = "初期値を表示しています（保存すると反映されます）";
  });
  $("uiColors").addEventListener("input", (e) => {
    const k = e.target.dataset.k;
    if (!k) return;
    ui.colors[k] = e.target.value;
    applyColors(ui.colors);
  });

  document.querySelectorAll("#dlg .dlg-close").forEach((b) => b.addEventListener("click", () => { $("dlg").hidden = true; }));
  document.querySelectorAll("#dlgUi .dlg-close").forEach((b) => b.addEventListener("click", closeUi));
  $("dlg").addEventListener("click", (e) => { if (e.target === $("dlg")) $("dlg").hidden = true; });
  $("dlgUi").addEventListener("click", (e) => { if (e.target === $("dlgUi")) closeUi(); });
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    if (!$("dlgUi").hidden) closeUi();
    $("dlg").hidden = true;
  });

  // ------------------------------------------------------------ API for the column designer
  window.CM3App = {
    get state() { return state; },
    esc: esc,
    postJson: postJson,
    /** live preview while editing (null = back to saved settings) */
    setDraft(settings) {
      state.draft = settings;
      rebuildView();
      renderAll();
    },
    /** settings saved on the server */
    setSaved(settings) {
      state.viewSettings = settings;
      state.draft = null;
      window.APP.view = settings;
      rebuildView();
      renderAll();
      load(true);   // master columns needed may have changed
    }
  };

  applyColors(window.APP.ui.colors);
  selectEquipment(state.setsubi);
  rebuildView();
  buildHead();
  load(false);
  resetTimer();
  void X;
})();
