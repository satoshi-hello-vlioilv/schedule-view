// 画面の撮影と測定（開発用。直す前と後を同じ物差しで比べる: CLAUDE.md「改良するときは…」の 1 と 7）。
//
//   node program/tools/ui-check.mjs 出力フォルダ [--view 1728x1152] [--url http://127.0.0.1:5000] [--only line,hot]
//
// 状態ごと（line: 設備の一覧・hot: HOT コイル作業順・cols: 列の設定・master: 台帳設定・ui: 配色）に撮り（状態.png）、
// 迷いやすさに関わる量を測る（metrics.json。測る量は ui-harness.mjs の measure）。--url を省くと開発用の受け口を作り物のデータで起こす。

import fs from "node:fs";
import path from "node:path";
import { launch, measure, parseView, walk } from "./ui-harness.mjs";

const [outArg, ...rest] = process.argv.slice(2);
if (!outArg) {
  console.error("使い方: node program/tools/ui-check.mjs 出力フォルダ [--view 1728x1152] [--url URL] [--only line,hot]");
  process.exit(2);
}
const option = (name) => {
  const i = rest.indexOf(`--${name}`);
  return i >= 0 ? rest[i + 1] : undefined;
};
const OUT = path.resolve(outArg);
const VIEW = parseView(option("view"));
const ONLY = option("only")?.split(",");

const app = await launch({ url: option("url") });
const report = {};
try {
  fs.mkdirSync(OUT, { recursive: true });
  const page = await app.newPage("light", VIEW);
  await walk(page, app.base, async (name) => {
    await page.screenshot({ path: path.join(OUT, `${name}.png`) });
    report[name] = await page.evaluate(measure);
    console.log(name, JSON.stringify(report[name]));
  }, ONLY);
} finally {
  await app.close();
}
fs.writeFileSync(path.join(OUT, "metrics.json"), JSON.stringify(report, null, 1));
