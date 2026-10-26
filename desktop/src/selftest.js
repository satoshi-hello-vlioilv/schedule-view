/* 自己診断（環境変数 SV_SELFTEST=結果のファイル で起動したときだけ、窓が画面に流し込む）。
   本物の WebView の中から、窓（Rust）と画面の組が正しく動くかを確かめて /__desktop/selftest へ送る。窓は結果を書いて終わる。
   調べること: 画面が開く・静的ファイルの使い回しの決まり・外へ出ない・API・台帳（裏で読み終える）・HOT・Excel・日本語の問い合わせ・
   40 本同時・安全な文脈・保存領域・保存（ダウンロード）・窓の係（版・更新・ショートカット・配布）・画面のエラーが無い・速さ。 */
(async () => {
  const res = [];
  const ok = (name, cond, info = "") => res.push({ name, ok: !!cond, info: String(info).slice(0, 300) });
  const t0 = performance.now();
  const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
  const get = async (url, opt) => { const r = await fetch(url, opt); return { r, text: await r.text() }; };
  const post = (body) => ({ method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  const parse = (t) => { try { return JSON.parse(t); } catch (_) { return null; } };
  try {
    // 1) 画面が開く（一覧が描かれる）
    const end = performance.now() + 60000;
    const shown = () => /取得成功|取得失敗/.test((document.getElementById("state") || {}).textContent || "");
    while (!shown() && performance.now() < end) await sleep(200);
    ok("画面が開き、一覧を取りに行く", shown(), ((performance.now() - t0) / 1000).toFixed(1) + " 秒 " + document.getElementById("state").textContent);
    ok("初めの値が画面に埋まる（設備・列の設定）", window.APP && APP.equipments.length > 0 && APP.view && APP.view.columns.length > 0,
       window.APP && APP.equipments.map((e) => e.code).join(","));
    ok("設備のタブを描く", document.querySelectorAll("#equipTabs .tab").length === APP.equipments.length + (APP.hot.equipments.length ? 1 : 0));

    // 2) 静的ファイルの使い回しの決まり・外へ出ない
    const v = await get(`/static/app.js?v=${APP.build}`);
    ok("版の印つきの部品は長く使い回す", v.r.ok && /immutable/.test(v.r.headers.get("Cache-Control") || ""), v.r.headers.get("Cache-Control"));
    const plain = await get("/static/app.js");
    ok("印の無い部品は毎回確かめる（no-cache）", plain.r.headers.get("Cache-Control") === "no-cache" && plain.text === v.text);
    const escape = await get("/static/%2e%2e/config.json");
    ok("画面の置き場の外へは出ない", escape.r.status === 404, escape.r.status);
    const nf = await get("/api/no-such-route");
    ok("無いところは 404", nf.r.status === 404, nf.r.status);

    // 3) 台帳は裏で読み終える。一覧に台帳の列が付く
    let s = null;
    for (let i = 0; i < 150; i++) {
      s = parse((await get("/api/cm3/schedule?setsubi=" + encodeURIComponent(APP.defaultSetsubi))).text);
      if (s && s.success && s.master && !s.master.loading) break;
      await sleep(200);
    }
    ok("一覧を取れる（CM3）", s && s.success && s.items.length > 0, s && (s.message || s.items.length + " 行"));
    ok("台帳を裏で読み終え、一覧に付く", s && s.master && !s.master.loading && s.masterHit > 0, s && JSON.stringify(s.master).slice(0, 160));
    const hot = parse((await get("/api/hot")).text);
    ok("HOT コイル作業順（3 設備を同時に取る）", hot && hot.success && hot.hot && hot.hot.errors.length === 0, hot && (hot.message || hot.count + " 件"));

    // 4) 日本語の問い合わせ文字・誤りの答え
    const bad = await get("/api/cm3/schedule?setsubi=" + encodeURIComponent("日本語の設備"));
    const bj = parse(bad.text) || {};
    ok("日本語の問い合わせ文字（400 と日本語の理由）", bad.r.status === 400 && bj.message === "未登録の設備です: 日本語の設備", bj.message);
    const vbad = await get("/api/view/settings", post({ columns: [] }));
    ok("保存しない誤りの答え（列の設定）", vbad.r.status === 400 && (parse(vbad.text) || {}).message === "列がありません");

    // 5) Excel（表示どおりの中身から作る）
    const x = await fetch("/api/export/xlsx", post({ sheet: "自己診断", headers: ["No", "ロット"], rows: [{ cells: [{ n: 1 }, { v: "L4061S0" }] }] }));
    const xb = new Uint8Array(await x.arrayBuffer());
    ok("Excel を作る（ZIP の頭 PK）", x.ok && xb[0] === 0x50 && xb[1] === 0x4b, `${x.status} ${xb.length} バイト`);

    // 6) 同時の問い合わせが混ざらない
    const paths = Array.from({ length: 40 }, (_, i) => (i % 2 ? "/api/ui/settings" : `/static/style.css?v=p${i}`));
    const all = await Promise.all(paths.map((p) => get(p)));
    ok("40 本同時でも、それぞれの答えが届く", all.every((y, i) => y.r.ok && (i % 2 ? (parse(y.text) || {}).success : /--c-|\{/.test(y.text))));

    // 7) 安全な文脈・保存領域
    ok("安全な文脈（クリップボードへ写せる）", window.isSecureContext && !!(navigator.clipboard && navigator.clipboard.writeText), `isSecureContext=${window.isSecureContext}`);
    localStorage.setItem("svSelftest", "1");
    ok("localStorage が使える（選んだ設備・図の表示を覚える）", localStorage.getItem("svSelftest") === "1");
    localStorage.removeItem("svSelftest");

    // 8) 保存（ダウンロード）: 画面の「CSV出力」で、BOM つきの CSV が日本語の名前どおりに届く
    document.getElementById("btnCsv").click();
    let saved = null;
    for (let i = 0; i < 100 && !saved; i++) {
      await sleep(200);
      saved = (await (await fetch("/__desktop/downloads")).json()).find((d) => d.state === "finished");
    }
    ok("保存: CSV が名前どおり・中身どおりに届く", saved && saved.success && saved.head.startsWith("efbbbf") && /cm3_.*\.csv$/.test(saved.path || ""),
       JSON.stringify(saved));

    // 9) 窓の係
    const info = parse((await get("/__desktop/info")).text) || {};
    ok("窓の情報（版・作業場所）", info.version && info.local_root, `${info.version} ${info.local_root}`);
    const upd = parse((await get("/__desktop/update")).text) || {};
    ok("更新の係が答える", typeof upd.state === "string", upd.state);
    const inst = parse((await get("/__desktop/install")).text) || {};
    ok("ショートカットの係が答える", inst.shortcut && Array.isArray(inst.shortcut.places), JSON.stringify(inst.shortcut || {}).slice(0, 120));
    const rel = parse((await get("/__desktop/release?cached=1")).text) || {};
    ok("配布の係が答える（置き場が無ければ理由）", "reachable" in rel, rel.reachable ? "届く" : rel.why);

    // 10) 速さ（2 回目からの一覧は覚えたスケジュール＋台帳）
    const w0 = performance.now();
    await get("/api/cm3/schedule?setsubi=" + encodeURIComponent(APP.defaultSetsubi));
    const warm = performance.now() - w0;
    ok("2 回目からの一覧は速い（200ms 未満）", warm < 200, warm.toFixed(0) + "ms");

    const errs = window.__svErrors || [];
    ok("画面のエラーが無い", errs.length === 0, errs.join(" / "));
  } catch (e) {
    ok("自己診断が最後まで動く", false, e && (e.stack || e.message || e));
  }
  const failed = res.filter((x) => !x.ok);
  // 通しの確かめ（desktop/bundle/verify.py）が読む: 動いている exe・program・版と、更新・ショートカットの係の状態
  const extra = {};
  for (const [k, u] of [["info", "/__desktop/info"], ["update", "/__desktop/update"], ["install", "/__desktop/install"]]) {
    try { extra[k] = await (await fetch(u)).json(); } catch (_) { extra[k] = null; }
  }
  await fetch("/__desktop/selftest", post({ ok: failed.length === 0, results: res, seconds: ((performance.now() - t0) / 1000).toFixed(1), ...extra }));
})();
