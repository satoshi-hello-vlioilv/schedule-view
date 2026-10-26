// 改良案を画像で比べる（開発用。見た目を変えるときは、案を必ずこれで画像にして見比べてから選ぶ: CLAUDE.md）。
//
//   node program/tools/ui-variants.mjs 出力フォルダ 案の定義.mjs [--states line,open] [--view 1728x1152] [--only A,B] [--url URL]
//
// 案の定義（ES モジュール）は PROPOSALS = [{ key, name, css, ops, open, plain }] を書き出す（例: ui-proposals/2026-10-appmenu.mjs）。
// 今の画面に案ごとの CSS と DOM の組み替え（ops）を当てて撮る（line-案.png）。open があれば、続けて当てて「open」の状態も撮る
// （メニューを開いた所など）。plain があれば、ops の代わりに当てた別の状態（知らせが無いときなど）を「plain」として撮る。状態ごとに全案を並べた一覧（sheet-状態.png）も作る。一覧で全体を、1 枚ずつの画像で細部を見て採点する（ui_score.py）。
//
// ops（上から順に当てる）:
//   ["move", 動かすもの, "before"|"after"|"prepend"|"append", 基準]
//   ["class", 対象, クラス名]                                        … クラスを置き換える
//   ["insert", HTML, "beforebegin"|"afterbegin"|"beforeend"|"afterend", 基準]
//   ["attr", 対象, 属性の名前, 値]                                     … 値が null なら外す
//   ["script", "JavaScript の文"]                                      … 頁の中で動かす

import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { launch, measure, parseView, walk } from "./ui-harness.mjs";

const [outArg, defsArg, ...rest] = process.argv.slice(2);
if (!outArg || !defsArg) {
  console.error("使い方: node program/tools/ui-variants.mjs 出力フォルダ 案の定義.mjs [--states line,open] [--view 1728x1152] [--only A,B] [--url URL]");
  process.exit(2);
}
const option = (name, fallback) => {
  const i = rest.indexOf(`--${name}`);
  return i >= 0 ? rest[i + 1] : fallback;
};
const OUT = path.resolve(outArg);
const STATES = option("states", "line,open,plain").split(",");
const VIEW = parseView(option("view", ""));
const ONLY = option("only", "").split(",").filter(Boolean);
const { PROPOSALS } = await import(pathToFileURL(path.resolve(defsArg)).href);
const TAKE = PROPOSALS.filter((p) => !ONLY.length || ONLY.includes(p.key));

/** 案を頁に当てる（page.evaluate に渡す） */
function applyOps(ops) {
  const pick = (selector) => {
    const el = document.querySelector(selector);
    if (!el) throw new Error(`見つからない: ${selector}`);
    return el;
  };
  const place = (node, where, target) => ({ before: () => target.before(node), after: () => target.after(node), prepend: () => target.prepend(node), append: () => target.append(node) })[where]();
  for (const [kind, a, b, c] of ops) {
    if (kind === "move") place(pick(a), b, pick(c));
    else if (kind === "class") pick(a).className = b;
    else if (kind === "insert") pick(c).insertAdjacentHTML(b, a);
    else if (kind === "attr") {
      if (c === null) pick(a).removeAttribute(b);
      else pick(a).setAttribute(b, c);
    } else if (kind === "script") new Function(a)();
    else throw new Error(`知らない操作: ${kind}`);
  }
}

/** 状態ごとに全案を並べた一覧の画像 */
async function contactSheet(app, state, metrics) {
  const shot = (key) => `${state}-${key}.png`;
  const shown = PROPOSALS.filter(({ key }) => fs.existsSync(path.join(OUT, shot(key))));
  if (!shown.length) return;
  const cols = Math.min(3, shown.length);
  const tiles = shown.map(({ key, name }) => {
    const m = metrics[`${state}-${key}`];
    const facts = m ? `主ボタン ${m.primaries} · 横スクロール ${m.hScroll ? "あり" : "なし"} · 切れ ${m.cut} · 11px 未満 ${m.smallText}` : "";
    return `<figure><figcaption><b>${key}</b> ${name}<small>${facts}</small></figcaption><img src="${pathToFileURL(path.join(OUT, shot(key))).href}"></figure>`;
  }).join("");
  const html = `<!doctype html><meta charset="utf-8"><style>
    body{margin:0;padding:16px;background:#e9edf2;font:14px "Noto Sans CJK JP",sans-serif;display:grid;grid-template-columns:repeat(${cols},1fr);gap:16px}
    figure{margin:0;background:#fff;border:1px solid #c9d2dc;border-radius:8px;overflow:hidden}
    figcaption{padding:8px 10px;border-bottom:1px solid #e3e8ee}figcaption small{display:block;color:#5b6773;font-size:12px}
    img{display:block;width:100%}</style>${tiles}`;
  const page = await app.newPage("light", { width: 1800, height: 1000 });
  const file = path.join(OUT, `sheet-${state}.html`);
  fs.writeFileSync(file, html);
  await page.goto(pathToFileURL(file).href);
  await page.waitForTimeout(500);
  await page.screenshot({ path: path.join(OUT, `sheet-${state}.png`), fullPage: true });
  await page.context().close();
  fs.unlinkSync(file);
}

const app = await launch({ url: option("url") });
const metrics = fs.existsSync(path.join(OUT, "metrics.json")) ? JSON.parse(fs.readFileSync(path.join(OUT, "metrics.json"), "utf8")) : {};
try {
  fs.mkdirSync(OUT, { recursive: true });
  for (const p of TAKE) {
    const page = await app.newPage("light", VIEW);
    await walk(page, app.base, async () => {}, ["line"]);
    if (p.css) await page.addStyleTag({ content: p.css });
    await page.evaluate(applyOps, p.ops || []);
    await page.waitForTimeout(300);
    if (STATES.includes("line")) {
      await page.screenshot({ path: path.join(OUT, `line-${p.key}.png`) });
      metrics[`line-${p.key}`] = await page.evaluate(measure);
    }
    if (STATES.includes("open") && p.open) {
      await page.evaluate(applyOps, p.open);
      await page.waitForTimeout(300);
      await page.screenshot({ path: path.join(OUT, `open-${p.key}.png`) });
      metrics[`open-${p.key}`] = await page.evaluate(measure);
    }
    console.log(p.key, JSON.stringify(metrics[`line-${p.key}`] || {}));
    await page.context().close();
    if (STATES.includes("plain") && p.plain) {
      const plain = await app.newPage("light", VIEW);
      await walk(plain, app.base, async () => {}, ["line"]);
      if (p.css) await plain.addStyleTag({ content: p.css });
      await plain.evaluate(applyOps, p.plain);
      await plain.waitForTimeout(300);
      await plain.screenshot({ path: path.join(OUT, `plain-${p.key}.png`) });
      metrics[`plain-${p.key}`] = await plain.evaluate(measure);
      await plain.context().close();
    }
  }
  for (const s of STATES) await contactSheet(app, s, metrics);
} finally {
  await app.close();
}
fs.writeFileSync(path.join(OUT, "metrics.json"), JSON.stringify(metrics, null, 1));
