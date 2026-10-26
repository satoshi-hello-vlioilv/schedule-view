// 版・更新の知らせ・アプリの係（ショートカット・版の配布）の入口の案（2026-10。exe にしたので新しく要る UI）。
// 撮る: node program/tools/ui-variants.mjs <出力> program/tools/ui-proposals/2026-10-appmenu.mjs --view 1728x1152（1536x864・1280x800 も）
// 状態: line＝使っている間に新しい版 12.0.1 を取り込み終えた（開き直せば更新できる）・open＝アプリの係を開いた所
// 根拠（今の画面を撮って見えたこと）: 上の帯は 1728 で 1 段に収まり余りが無い。1536 で「CM3画面を開く」が 2 段目へ落ち、
// 1280 では「検索」の字が縦に折れ、自動更新のチェックが検索欄に重なる。下の状態の帯は右が空いている。

const MENU_ITEMS = `
  <div class="am-head"><b>版 12.0.0</b><span class="am-new">更新あり 12.0.1</span></div>
  <button type="button" class="am-primary">開き直して 12.0.1 にする</button>
  <button type="button">新しい版を確かめる<small>前に確かめた時刻 16:30（共有の置き場）</small></button>
  <button type="button">ショートカットを作る<small>デスクトップ ✕・スタート ✕</small></button>
  <hr><button type="button">版の配布…<small>置き場に版を置く・配る版を選ぶ</small></button>`;

const MENU_CSS = `
  .am-pop{position:absolute;z-index:20;width:330px;background:#fff;border:1px solid var(--line);border-radius:10px;
    box-shadow:0 10px 28px rgba(15,23,42,.18);padding:6px;font-size:13px}
  .am-pop .am-head{display:flex;align-items:center;justify-content:space-between;padding:8px 10px;color:var(--text)}
  .am-new{background:#e6f4ea;color:#1a7f37;border:1px solid #b7dfc2;border-radius:999px;padding:1px 9px;font-size:12px;font-weight:600}
  .am-pop button{display:block;width:100%;text-align:left;border:0;background:none;padding:8px 10px;font:inherit;border-radius:6px;cursor:pointer;color:var(--text)}
  .am-pop button small{display:block;color:var(--muted);font-size:12px;margin-top:1px}
  .am-pop button:hover{background:#f0f3f7}
  .am-pop .am-primary{background:#1a7f37;color:#fff;font-weight:600;text-align:center;margin:2px 0 6px}
  .am-pop .am-primary:hover{background:#166c2f}
  .am-pop hr{border:0;border-top:1px solid var(--line);margin:4px 6px}
  .am-dot{position:absolute;top:-3px;right:-3px;width:10px;height:10px;border-radius:50%;background:#1a7f37;border:2px solid #fff}`;

export const PROPOSALS = [
  {
    key: "A",
    name: "上の帯の右端に「アプリ ▾」（更新ありは緑の点）",
    css: MENU_CSS + `.am-btn{position:relative}`,
    ops: [
      ["insert", `<button type="button" class="btn am-btn" id="amBtn">アプリ ▾<span class="am-dot"></span></button>`, "beforeend", ".controls"],
    ],
    open: [
      ["script", `const b=document.getElementById('amBtn').getBoundingClientRect();document.body.insertAdjacentHTML('beforeend','<div class="am-pop" style="top:'+(b.bottom+6)+'px;left:'+Math.max(8,b.right-330)+'px">'+${JSON.stringify(MENU_ITEMS)}+'</div>')`],
    ],
  },
  {
    key: "B",
    name: "下の状態の帯の右に版の札（更新ありで緑・押すと上へ開く）",
    css: MENU_CSS + `.foot{display:flex;align-items:center;gap:12px}.foot .am-chip{margin-left:auto}
      .am-chip{display:inline-flex;align-items:center;gap:8px;border:1px solid var(--line);background:#fff;border-radius:999px;padding:2px 4px 2px 10px;font:inherit;font-size:12px;color:var(--muted);cursor:pointer}
      .am-chip b{color:var(--text);font-weight:600}
      .am-chip .am-new{font-size:12px}`,
    ops: [
      ["insert", `<button type="button" class="am-chip" id="amChip"><b>版 12.0.0</b><span class="am-new">更新あり 12.0.1 ▴</span></button>`, "beforeend", ".foot"],
    ],
    open: [
      ["script", `const b=document.getElementById('amChip').getBoundingClientRect();document.body.insertAdjacentHTML('beforeend','<div class="am-pop" style="bottom:'+(innerHeight-b.top+6)+'px;left:'+(b.right-330)+'px">'+${JSON.stringify(MENU_ITEMS)}+'</div>')`],
    ],
  },
  {
    key: "C",
    name: "題の下に版の札（更新ありで緑・押すと下へ開く）",
    css: MENU_CSS + `.title{display:flex;flex-direction:column;align-items:flex-start;gap:4px}
      .am-chip{display:inline-flex;align-items:center;gap:6px;border:1px solid #b7dfc2;background:#e6f4ea;color:#1a7f37;border-radius:999px;padding:1px 9px;font:inherit;font-size:12px;font-weight:600;cursor:pointer}`,
    ops: [
      ["insert", `<button type="button" class="am-chip" id="amChip">版 12.0.0 → 更新あり 12.0.1 ▾</button>`, "beforeend", ".title"],
    ],
    open: [
      ["script", `const b=document.getElementById('amChip').getBoundingClientRect();document.body.insertAdjacentHTML('beforeend','<div class="am-pop" style="top:'+(b.bottom+6)+'px;left:'+b.left+'px">'+${JSON.stringify(MENU_ITEMS)}+'</div>')`],
    ],
  },
  {
    key: "D",
    name: "知らせの帯（あるときだけ・一覧の上）＋上の帯の右端に「⋯」",
    css: MENU_CSS + `.am-bar{display:flex;align-items:center;gap:12px;margin:10px 20px 0;padding:8px 14px;border-radius:8px;background:#e6f4ea;color:#14532d;border:1px solid #b7dfc2}
      .am-bar b{font-weight:600}.am-bar .am-go{margin-left:auto;background:#1a7f37;border-color:#1a7f37;color:#fff}
      .am-more{width:36px;padding-left:0;padding-right:0;position:relative}`,
    ops: [
      ["insert", `<div class="am-bar"><b>新しい版 12.0.1 を取り込みました。</b><span>開き直すと入れ替わります（いまの版 12.0.0）。</span><button class="btn am-go" type="button">開き直して更新</button><button class="btn" type="button">あとで</button></div>`, "afterend", "#lineSummary"],
      ["insert", `<button type="button" class="btn am-more" id="amBtn" title="アプリ">⋯</button>`, "beforeend", ".controls"],
    ],
    open: [
      ["script", `const b=document.getElementById('amBtn').getBoundingClientRect();document.body.insertAdjacentHTML('beforeend','<div class="am-pop" style="top:'+(b.bottom+6)+'px;left:'+Math.max(8,b.right-330)+'px">'+${JSON.stringify(MENU_ITEMS)}+'</div>')`],
    ],
  },
  {
    key: "E",
    name: "下の状態の帯の右に、文字のリンクで並べる（開かずに全部見える）",
    css: `.foot{display:flex;align-items:center;gap:14px}.foot .am-links{margin-left:auto;display:flex;align-items:center;gap:14px}
      .am-links a{color:var(--c-accent);text-decoration:none;cursor:pointer}.am-links a:hover{text-decoration:underline}
      .am-links .am-go{color:#fff;background:#1a7f37;border-radius:6px;padding:2px 10px;font-weight:600;text-decoration:none}
      .am-links .sep{color:var(--line)}`,
    ops: [
      ["insert", `<span class="am-links"><span>版 12.0.0</span><a class="am-go">更新あり 12.0.1 · 開き直す</a><span class="sep">|</span><a>ショートカットを作る</a><span class="sep">|</span><a>版の配布…</a></span>`, "beforeend", ".foot"],
    ],
  },
  {
    key: "F",
    name: "設定を「設定 ▾」にまとめ（列・台帳・配色・アプリ）、上の帯を減らす",
    css: MENU_CSS + `.am-btn{position:relative}`,
    ops: [
      ["script", `['btnCols','btnMaster','btnUi'].forEach(id=>document.getElementById(id).style.display='none')`],
      ["insert", `<button type="button" class="btn am-btn" id="amBtn">設定 ▾<span class="am-dot"></span></button>`, "afterend", "#btnXlsx"],
    ],
    open: [
      ["script", `const b=document.getElementById('amBtn').getBoundingClientRect();document.body.insertAdjacentHTML('beforeend','<div class="am-pop" style="top:'+(b.bottom+6)+'px;left:'+Math.max(8,b.right-330)+'px"><button type="button">列の設定</button><button type="button">台帳設定</button><button type="button">配色</button><hr>'+${JSON.stringify(MENU_ITEMS)}+'</div>')`],
    ],
  },
];
