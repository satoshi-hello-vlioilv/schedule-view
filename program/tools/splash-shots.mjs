// 起動画面（desktop/splash/index.html）を撮る（開発用）。進行中（2 段目）・済んだ所・止まった所（次にすることと理由）。
//
//   node program/tools/splash-shots.mjs 出力フォルダ [--view 1728x1152]
//
// icon.png は desktop を作ると build.rs が描く（無ければ program/app/app_icon.py で描く）。

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { createRequire } from "node:module";
import { REPO, parseView } from "./ui-harness.mjs";

const require = createRequire(import.meta.url);
const { chromium } = require("playwright");
const OUT = path.resolve(process.argv[2] || "splash-shots");
const i = process.argv.indexOf("--view");
const VIEW = parseView(i > 0 ? process.argv[i + 1] : "");
const splash = path.join(REPO, "desktop", "splash");
if (!fs.existsSync(path.join(splash, "icon.png"))) {
  const py = process.env.SV_PYTHON || (process.platform === "win32" ? "python" : "python3");
  spawnSync(py, ["-c", `import runpy,pathlib;m=runpy.run_path('program/app/app_icon.py');pathlib.Path('desktop/splash/icon.png').write_bytes(m['png'](256))`], { cwd: REPO });
}
fs.mkdirSync(OUT, { recursive: true });
const brand = JSON.parse(fs.readFileSync(path.join(REPO, "program", "app", "brand.json"), "utf8"));
const browser = await chromium.launch();
const states = {
  running: (p) => p.evaluate(([t, s]) => { splash.brand(t, s); splash.version("12.0.0"); splash.step("program", "ok", "C:\\Users\\user\\AppData\\Local\\ScheduleView\\app\\program"); splash.step("update", "now", "新しい版が配られていないか確かめています…"); }, [brand.title, brand.subtitle]),
  opening: (p) => p.evaluate(([t, s]) => { splash.brand(t, s); splash.version("12.0.1"); ["program", "update"].forEach((k) => splash.step(k, "ok", "")); splash.step("update", "ok", "版 12.0.0 → 12.0.1 に入れ替えました"); splash.step("data", "ok", "版 12.0.1 ・ 0.1 秒"); splash.step("open", "now", "画面を開いています…"); }, [brand.title, brand.subtitle]),
  fail: (p) => p.evaluate(([t, s]) => { splash.brand(t, s); splash.version("12.0.0"); splash.step("program", "ok", ""); splash.step("update", "ok", ""); splash.step("data", "now", ""); splash.fail("設定を読めません", "設定（C:\\Users\\user\\AppData\\Local\\ScheduleView\\app\\program\\app\\config.json）を読めません"); }, [brand.title, brand.subtitle]),
};
for (const [name, act] of Object.entries(states)) {
  const page = await browser.newPage({ viewport: VIEW, deviceScaleFactor: 1.25 });
  await page.goto(pathToFileURL(path.join(splash, "index.html")).href);
  await act(page);
  await page.waitForTimeout(600);
  await page.screenshot({ path: path.join(OUT, `splash-${name}.png`) });
  await page.close();
}
await browser.close();
console.log(OUT);
