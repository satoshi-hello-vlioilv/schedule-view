// 版・更新の知らせとアプリの係の入口: 2 回目の案（1 回目は 2026-10-appmenu.json。1 位 C 78.0 点で合格点 80 に届かず）。
// 撮る: node program/tools/ui-variants.mjs <出力> program/tools/ui-proposals/2026-10-appmenu-2.mjs --view 1728x1152（1536x864・1280x800 も）
// 状態: line＝新しい版 12.0.1 を取り込み終えた・open＝アプリの係を開いた・plain＝更新が無いとき（いつもの見え方）
// 1 回目の採点の指摘: ① 版の札が係の入口と分かる形に（▾）② 開き直しを 1 手に ③ 開き直しの言葉と置き場を 1 つに
//                    ④ 更新が無いときの見え方も撮る ⑤ 上の帯の右端にボタンを足さない ⑥ 管理者だけの「版の配布…」を常に見える所に出さない
// 自分の確かめ: M は初め帯を重ねて置き、要約のカードの見出しを隠した → 上の帯の下に差し込む形に直した
// 言葉は全案で 1 つ: 開き直しのボタンは「開き直して更新」（どの置き場でも同じ字・同じ緑）

const REOPEN = "開き直して更新";
const ITEMS = (withReopen) => `
  <div class="am-head"><b>版 12.0.0</b>${withReopen ? '<span class="am-new">新しい版 12.0.1 を取り込み済み</span>' : '<span class="am-ok">最新</span>'}</div>
  ${withReopen ? `<button type="button" class="am-primary">${REOPEN}</button>` : ""}
  <button type="button">新しい版を確かめる<small>前に確かめた時刻 16:30（共有の置き場）</small></button>
  <button type="button">ショートカットを作る<small>デスクトップ ✕・スタート ✕</small></button>
  <hr><button type="button">版の配布…<small>置き場に版を置く・配る版を選ぶ（管理する人）</small></button>`;

const CSS = `
  .am-pop{position:absolute;z-index:20;width:330px;background:#fff;border:1px solid var(--line);border-radius:10px;
    box-shadow:0 10px 28px rgba(15,23,42,.18);padding:6px;font-size:13px}
  .am-pop .am-head{display:flex;align-items:center;justify-content:space-between;gap:8px;padding:8px 10px;color:var(--text)}
  .am-new{background:#e6f4ea;color:#1a7f37;border:1px solid #b7dfc2;border-radius:999px;padding:1px 9px;font-size:12px;font-weight:600}
  .am-ok{color:var(--muted);font-size:12px}
  .am-pop button{display:block;width:100%;text-align:left;border:0;background:none;padding:8px 10px;font:inherit;border-radius:6px;cursor:pointer;color:var(--text)}
  .am-pop button small{display:block;color:var(--muted);font-size:12px;margin-top:1px}
  .am-pop button:hover{background:#f0f3f7}
  .am-pop .am-primary,.am-go{background:#1a7f37;color:#fff;font-weight:600;border:1px solid #1a7f37}
  .am-pop .am-primary{text-align:center;margin:2px 0 6px}
  .am-go{border-radius:6px;padding:3px 10px;font:inherit;font-size:12px;cursor:pointer;white-space:nowrap}
  .am-pop hr{border:0;border-top:1px solid var(--line);margin:4px 6px}
  .am-chip{display:inline-flex;align-items:center;gap:6px;border:1px solid var(--line);background:#fff;color:var(--text);border-radius:999px;
    padding:1px 10px;font:inherit;font-size:12px;cursor:pointer;white-space:nowrap}
  .am-chip.has{border-color:#b7dfc2;background:#e6f4ea;color:#1a7f37;font-weight:600}`;

const pop = (anchor, where, withReopen) => ["script", `const b=document.querySelector('${anchor}').getBoundingClientRect();
  const pos=${where === "up" ? "'bottom:'+(innerHeight-b.top+6)+'px;left:'+Math.max(8,Math.min(innerWidth-338,b.left))+'px'" : "'top:'+(b.bottom+6)+'px;left:'+Math.max(8,Math.min(innerWidth-338,b.left))+'px'"};
  document.body.insertAdjacentHTML('beforeend','<div class="am-pop" style="'+pos+'">'+${JSON.stringify(ITEMS(withReopen))}+'</div>')`];

const titleCss = `.title{display:flex;flex-direction:column;align-items:flex-start;gap:5px}.am-row{display:flex;align-items:center;gap:6px}`;

export const PROPOSALS = [
  {
    key: "C2",
    name: "（1 回目 C を直した）題の下に「版 ▾」の札＋更新ありのときだけ緑の「開き直して更新」",
    css: CSS + titleCss,
    ops: [["insert", `<div class="am-row"><button type="button" class="am-chip has" id="amChip">版 12.0.0 ▾</button><button type="button" class="am-go">${REOPEN}</button></div>`, "beforeend", ".title"]],
    plain: [["insert", `<div class="am-row"><button type="button" class="am-chip" id="amChip">版 12.0.0 ▾</button></div>`, "beforeend", ".title"]],
    open: [pop("#amChip", "down", true)],
  },
  {
    key: "H",
    name: "（1 回目 B を直した）下の状態の帯の右に「版 ▾」の札＋更新ありのときだけ緑の「開き直して更新」",
    css: CSS + `.foot{display:flex;align-items:center;gap:10px}.foot .am-row{margin-left:auto;display:flex;align-items:center;gap:8px}`,
    ops: [["insert", `<span class="am-row"><span style="color:#1a7f37;font-weight:600">新しい版 12.0.1 を取り込みました</span><button type="button" class="am-go">${REOPEN}</button><button type="button" class="am-chip" id="amChip">版 12.0.0 ▴</button></span>`, "beforeend", ".foot"]],
    plain: [["insert", `<span class="am-row"><button type="button" class="am-chip" id="amChip">版 12.0.0 ▴</button></span>`, "beforeend", ".foot"]],
    open: [pop("#amChip", "up", true)],
  },
  {
    key: "I",
    name: "（新）右下に知らせの札（更新ありのときだけ・閉じられる）＋題の下に「版 ▾」",
    css: CSS + titleCss + `.am-toast{position:fixed;right:24px;bottom:44px;z-index:15;display:flex;align-items:center;gap:12px;background:#fff;border:1px solid #b7dfc2;
      border-left:4px solid #1a7f37;border-radius:10px;box-shadow:0 10px 28px rgba(15,23,42,.18);padding:10px 12px 10px 14px;font-size:13px}
      .am-toast b{color:#14532d}.am-toast .x{border:0;background:none;color:var(--muted);font-size:16px;cursor:pointer}`,
    ops: [
      ["insert", `<div class="am-row"><button type="button" class="am-chip has" id="amChip">版 12.0.0 ▾</button></div>`, "beforeend", ".title"],
      ["insert", `<div class="am-toast"><div><b>新しい版 12.0.1 を取り込みました</b><div style="color:var(--muted);font-size:12px">開き直すと入れ替わります</div></div><button type="button" class="am-go">${REOPEN}</button><button type="button" class="x" title="あとで">×</button></div>`, "beforeend", "body"],
    ],
    plain: [["insert", `<div class="am-row"><button type="button" class="am-chip" id="amChip">版 12.0.0 ▾</button></div>`, "beforeend", ".title"]],
    open: [pop("#amChip", "down", true)],
  },
  {
    key: "J",
    name: "（新）要約のカードの並びの端に「版」のカード（更新ありで緑の地と「開き直して更新」・押すと係）",
    css: CSS + `.summary{grid-template-columns:repeat(5,minmax(0,1fr)) 1.4fr 2fr 1.3fr}
      .am-card{display:flex;flex-direction:column;gap:4px;cursor:pointer}.am-card.has{background:#e6f4ea;border-color:#b7dfc2}
      .am-card .value{display:flex;align-items:center;gap:8px;flex-wrap:wrap}.am-card.has .label{color:#1a7f37}`,
    ops: [["insert", `<div class="card am-card has" id="amChip"><div class="label">版 12.0.0 ▾</div><div class="value small"><b style="color:#14532d">12.0.1 を取り込み済み</b><button type="button" class="am-go">${REOPEN}</button></div></div>`, "beforeend", "#lineSummary"]],
    plain: [["insert", `<div class="card am-card" id="amChip"><div class="label">版 ▾</div><div class="value small">12.0.0（最新）</div></div>`, "beforeend", "#lineSummary"]],
    open: [pop("#amChip", "down", true)],
  },
  {
    key: "L",
    name: "（新）題の右に版をつなげた 1 つの名乗り「CM3 スケジュール 12.0.0 ▾」＋更新ありの緑の印とボタン",
    css: CSS + `.title{display:flex;flex-direction:column;align-items:flex-start;gap:4px}.title h1{display:flex;align-items:baseline;gap:6px;cursor:pointer}
      .am-ver{font-size:12px;font-weight:500;color:var(--muted)}.am-ver.has{color:#1a7f37;font-weight:600}
      .am-dotline{display:flex;align-items:center;gap:6px;font-size:12px;color:#1a7f37;font-weight:600}`,
    ops: [
      ["script", `document.querySelector('.title h1').insertAdjacentHTML('beforeend','<span class="am-ver has" id="amChip">12.0.0 ▾</span>')`],
      ["insert", `<div class="am-dotline">● 12.0.1 あり <button type="button" class="am-go">${REOPEN}</button></div>`, "beforeend", ".title"],
    ],
    plain: [["script", `document.querySelector('.title h1').insertAdjacentHTML('beforeend','<span class="am-ver" id="amChip">12.0.0 ▾</span>')`]],
    open: [pop("#amChip", "down", true)],
  },
  {
    key: "M",
    name: "（新）上の帯のすぐ下に細い帯（更新ありのときだけ・高さ 26px）＋題の下に「版 ▾」",
    css: CSS + titleCss + `.am-strip{display:flex;justify-content:center;align-items:center;gap:12px;height:26px;box-sizing:border-box;
      background:#e6f4ea;border-bottom:1px solid #b7dfc2;padding:0 12px;font-size:12px;color:#14532d}.am-strip .am-go{padding:1px 10px}`,
    ops: [
      ["insert", `<div class="am-row"><button type="button" class="am-chip has" id="amChip">版 12.0.0 ▾</button></div>`, "beforeend", ".title"],
      ["insert", `<div class="am-strip"><b>新しい版 12.0.1 を取り込みました。</b><span>開き直すと入れ替わります</span><button type="button" class="am-go">${REOPEN}</button></div>`, "afterend", ".topbar"],
    ],
    plain: [["insert", `<div class="am-row"><button type="button" class="am-chip" id="amChip">版 12.0.0 ▾</button></div>`, "beforeend", ".title"]],
    open: [pop("#amChip", "down", true)],
  },
];
