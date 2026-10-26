// 画面の撮影と測定の土台（ui-check.mjs・ui-variants.mjs が使う。開発用）。
//
// 画面は開発用の HTTP（desktop/examples/serve.rs）で出し、本物と同じ Chromium で撮る（WebView2 も Chromium）。
//   - 受け口の URL を --url で渡すか（旧版の Flask などほかの物を撮るとき）、
//   - 渡さなければ、作り物のデータ（fixture.py）を作り、desktop/target/release/examples/serve を起こす
//     （先に `cd desktop && cargo build --release --example serve`）
// 大きさの既定は利用者の PC（2160×1440・125% → CSS で 1728×1152・倍率 1.25）。

import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { chromium } = require("playwright");
const HERE = path.dirname(fileURLToPath(import.meta.url));
export const REPO = path.resolve(HERE, "..", "..");

/** "1536x864" → {width, height}。既定は 1728×1152 */
export function parseView(text) {
  const m = /^(\d+)x(\d+)$/.exec(text || "");
  return m ? { width: +m[1], height: +m[2] } : { width: 1728, height: 1152 };
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** 空いているポート（乱数で選ぶと、ほかの撮影と重なって失敗することがあった） */
function freePort() {
  return new Promise((ok, ng) => {
    const srv = net.createServer();
    srv.once("error", ng);
    srv.listen(0, "127.0.0.1", () => { const { port } = srv.address(); srv.close(() => ok(port)); });
  });
}

async function waitUp(url, ms = 30000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    try {
      const r = await fetch(url);
      if (r.ok) return;
    } catch { /* まだ */ }
    await sleep(200);
  }
  throw new Error(`${url} が答えません`);
}

/** 受け口を用意し、ブラウザを開く。→ { base, newPage(theme, view), close() } */
export async function launch({ url } = {}) {
  let child = null;
  let tmp = null;
  let base = url;
  if (!base) {
    const exe = path.join(REPO, "desktop", "target", "release", "examples", process.platform === "win32" ? "serve.exe" : "serve");
    if (!fs.existsSync(exe)) throw new Error(`${exe} がありません。先に cd desktop && cargo build --release --example serve`);
    tmp = fs.mkdtempSync(path.join(os.tmpdir(), "sv-ui-"));
    const py = process.env.SV_PYTHON || (process.platform === "win32" ? "python" : "python3");
    const r = spawnSync(py, [path.join(REPO, "program", "tools", "fixture.py"), path.join(tmp, "data")], { encoding: "utf8" });
    if (r.status !== 0) throw new Error(`fixture.py: ${r.stderr}`);
    const port = await freePort();
    child = spawn(exe, [String(port), "", path.join(tmp, "work")], { env: { SV_FAKE_DESKTOP: "ready", ...process.env, SV_FIXTURE: path.join(tmp, "data") }, stdio: "ignore" });
    base = `http://127.0.0.1:${port}`;
  }
  base = base.replace(/\/$/, "");
  await waitUp(base + "/");
  const browser = await chromium.launch();
  return {
    base,
    async newPage(theme = "light", view = parseView()) {
      const ctx = await browser.newContext({ viewport: view, deviceScaleFactor: 1.25, colorScheme: theme, locale: "ja-JP", timezoneId: "Asia/Tokyo" });
      const page = await ctx.newPage();
      page.on("pageerror", (e) => console.error("画面のエラー:", e.message));
      return page;
    },
    async close() {
      await browser.close();
      if (child) child.kill();
      if (tmp) fs.rmSync(tmp, { recursive: true, force: true });
    },
  };
}

/** 一覧が描き終わるまで待つ */
async function settled(page) {
  await page.waitForFunction(() => {
    const s = document.getElementById("state");
    return document.querySelectorAll("#rows tr").length > 0 && s && /取得成功|取得失敗/.test(s.textContent);
  }, null, { timeout: 30000 });
  await sleep(300);
}

/**
 * 状態をたどりながら撮る。shot(名前) を状態ごとに呼ぶ。
 * line: 設備の一覧（L-2）・hot: HOT コイル作業順・cols: 列の設定・master: 台帳設定・ui: 配色
 * app: 版の札の係を開いた所・release: 版の配布の窓・shortcut: ショートカットの問い（窓の係は開発用の受け口の作り物の答え。SV_FAKE_DESKTOP）
 */
export async function walk(page, base, shot, only = null) {
  const want = (n) => !only || only.includes(n);
  await page.goto(base + "/?setsubi=L-2");
  await settled(page);
  if (want("line")) await shot("line");
  if (want("hot")) {
    await page.click('#equipTabs .tab[data-code="__HOT__"]');
    await settled(page);
    await shot("hot");
    await page.click('#equipTabs .tab[data-code="L-2"]');
    await settled(page);
  }
  const dialog = async (name, button, box, close) => {
    if (!want(name)) return;
    await page.click(button);
    await page.waitForSelector(`${box}:not([hidden])`);
    await sleep(800);
    await shot(name);
    await page.click(close);
    await sleep(200);
  };
  await dialog("cols", "#btnCols", "#dlgCols", "#cdCancel");
  await dialog("master", "#btnMaster", "#dlg", "#dlg .dlg-close");
  await dialog("ui", "#btnUi", "#dlgUi", "#dlgUi .dlg-close");
  const desk = async (name, open, close) => {
    if (!want(name) || !(await page.evaluate(() => !!window.SVDesktop && !!document.getElementById("appChip")))) return;
    await page.evaluate(open);
    await sleep(700);
    await shot(name);
    await page.evaluate(close);
    await sleep(200);
  };
  await desk("app", () => SVDesktop.toggleMenu(), () => document.getElementById("appMenu") && SVDesktop.toggleMenu());
  await desk("release", () => SVDesktop.openRelease(), () => document.getElementById("appRelease").remove());
  await desk("shortcut", () => SVDesktop.askShortcut(), () => document.getElementById("appShortcut").remove());
}

/** 迷いやすさに関わる量（page.evaluate に渡す） */
export function measure() {
  const vis = (el) => {
    const r = el.getBoundingClientRect();
    const st = getComputedStyle(el);
    return r.width > 0 && r.height > 0 && st.visibility !== "hidden" && st.display !== "none";
  };
  const all = [...document.querySelectorAll("body *")].filter(vis);
  const cut = all.filter((el) => ["BUTTON", "A", "LABEL", "SPAN"].includes(el.tagName) && el.scrollWidth > el.clientWidth + 1 && getComputedStyle(el).overflow === "hidden").length;
  const small = all.filter((el) => el.childNodes.length && [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim()) && parseFloat(getComputedStyle(el).fontSize) < 11).length;
  return {
    hScroll: document.documentElement.scrollWidth > window.innerWidth,
    primaries: all.filter((el) => el.matches(".btn.primary")).length,
    cut,
    smallText: small,
    buttons: all.filter((el) => el.matches("button, .btn")).length,
  };
}
