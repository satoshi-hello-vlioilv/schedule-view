// アイコンの案を並べた一覧の画像（開発用。案を比べて採点するとき: CLAUDE.md）。
//
//   node program/tools/icon-sheet.mjs 出力フォルダ [案,案,…]
//
// program/app/app_icon.py で案ごとに描き、明るい地（エクスプローラー）・暗い地（タスクバー）・紺（起動画面）の上に、
// 16・24・32・48px をそのままの大きさと 4 倍（画素が見える）で、256px を 1 つ並べて撮る（sheet-icons.png）。

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { createRequire } from "node:module";
import { REPO } from "./ui-harness.mjs";

const require = createRequire(import.meta.url);
const { chromium } = require("playwright");
const OUT = path.resolve(process.argv[2] || "icon-sheet");
const py = process.env.SV_PYTHON || (process.platform === "win32" ? "python" : "python3");
const listed = spawnSync(py, ["-c", "import runpy;print(','.join(runpy.run_path('program/app/app_icon.py')['DESIGNS']))"], { cwd: REPO, encoding: "utf8" });
const designs = (process.argv[3] || listed.stdout.trim()).split(",");
fs.mkdirSync(OUT, { recursive: true });
for (const d of designs) {
  const r = spawnSync(py, [path.join(REPO, "program", "app", "app_icon.py"), path.join(OUT, d), d], { encoding: "utf8" });
  if (r.status !== 0) throw new Error(r.stderr);
}
const bgs = [["明るい地", "#f3f3f3"], ["タスクバー", "#202020"], ["起動画面", "#0b2a4a"]];
const img = (d, s, zoom) => `<img src="${pathToFileURL(path.join(OUT, d, `icon-${s}.png`)).href}" style="width:${s * zoom}px;height:${s * zoom}px;image-rendering:pixelated">`;
const rows = designs.map((d) => `<section><h2>${d}</h2><div class="row">${img(d, 256, 0.5)}${bgs.map(([n, c]) =>
  `<div class="bg" style="background:${c}"><small>${n}</small><div>${[16, 24, 32, 48].map((s) => img(d, s, 1)).join("")}</div><div>${[16, 24, 32].map((s) => img(d, s, 4)).join("")}</div></div>`).join("")}</div></section>`).join("");
const html = `<!doctype html><meta charset="utf-8"><style>body{margin:0;padding:16px;font:14px sans-serif;background:#fff}
section{margin-bottom:14px}h2{margin:0 0 6px;font-size:16px}.row{display:flex;gap:12px;align-items:center}
.bg{padding:10px;border-radius:8px;display:flex;flex-direction:column;gap:8px}.bg small{color:#888}.bg div{display:flex;gap:10px;align-items:end}</style>${rows}`;
const file = path.join(OUT, "sheet.html");
fs.writeFileSync(file, html);
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1500, height: 400 } });
await page.goto(pathToFileURL(file).href);
await page.screenshot({ path: path.join(OUT, "sheet-icons.png"), fullPage: true });
await browser.close();
console.log(path.join(OUT, "sheet-icons.png"));
