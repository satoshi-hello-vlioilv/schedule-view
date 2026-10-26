/* exe（窓）の係: 版・更新の知らせ・アプリの係（新しい版を確かめる・ショートカット・版の配布）・保存の知らせ。
   窓が答える /__desktop/… を使う。開発用の受け口（ブラウザ）では答えが無いので、何も出さない。
   置き場と見せ方は program/tools/ui-proposals/2026-10-appmenu.json（案を撮って別のエージェントが採点して決めた）。
   言葉は 1 つ: 開き直すボタンはどこでも「開き直して更新」。 */
(function () {
  "use strict";

  const $ = (id) => document.getElementById(id);
  const REOPEN = "開き直して更新";
  const POLL_MS = 60 * 1000;
  const esc = (s) => String(s == null ? "" : s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
  const get = async (url) => { const r = await fetch(url, { cache: "no-store" }); if (!r.ok) throw new Error(r.status); return r.json(); };
  const post = async (url, body, raw) => {
    const r = await fetch(url, raw ? { method: "POST", body } : { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body || {}) });
    const j = await r.json().catch(() => ({}));
    if (!r.ok) throw new Error(j.error || j.message || r.status);
    return j;
  };
  const kb = (n) => n >= 1048576 ? (n / 1048576).toFixed(1) + " MB" : Math.max(1, Math.round(n / 1024)) + " KB";

  const st = { info: null, upd: null, install: null, asked: false };

  // ------------------------------------------------------------ 版の札と開き直すボタン
  function renderChip() {
    const slot = $("appSlot");
    if (!slot || !st.info) return;
    const u = st.upd || {};
    const ready = u.state === "ready" && u.ready;
    const dot = st.install && st.install.shortcut && st.install.shortcut.ask ? '<span class="app-dot" title="ショートカットがありません"></span>' : "";
    slot.hidden = false;
    slot.innerHTML =
      '<button type="button" class="app-chip' + (ready ? " has" : "") + '" id="appChip" aria-haspopup="menu" aria-expanded="false" title="版・更新・ショートカット・版の配布">' +
      "版 " + esc(st.info.version) + " ▾" + dot + "</button>" +
      (ready ? '<button type="button" class="app-go" id="appGo" title="新しい版 ' + esc(u.ready) + ' を取り込みました。開き直すと入れ替わります">' + REOPEN + "</button>" : "");
    $("appChip").onclick = (e) => { e.stopPropagation(); toggleMenu(); };
    if (ready) $("appGo").onclick = reopen;
  }

  async function reopen() {
    const b = $("appGo") || {};
    b.disabled = true;
    try {
      await post("/__desktop/update/apply");
      $("state").textContent = "新しい版で開き直しています…";
    } catch (e) {
      b.disabled = false;
      alert("開き直せませんでした: " + e.message);
    }
  }

  // ------------------------------------------------------------ 係のメニュー
  function closeMenu() {
    const m = $("appMenu");
    if (m) m.remove();
    const c = $("appChip");
    if (c) c.setAttribute("aria-expanded", "false");
  }

  function toggleMenu() {
    if ($("appMenu")) return closeMenu();
    const u = st.upd || {}, sc = (st.install && st.install.shortcut) || {};
    const ready = u.state === "ready" && u.ready;
    const where = (sc.places || []).map((p) => {
      const have = (sc.links || []).some((l) => l.startsWith(p));
      return (/Desktop|デスクトップ/i.test(p) ? "デスクトップ" : "スタート") + (have ? " ✓" : " ✕");
    }).join("・");
    const updLine = {
      latest: "最新です", ready: "新しい版 " + esc(u.ready) + " を取り込み済み", staging: "新しい版 " + esc(u.detail) + " を取り込んでいます…",
      checking: "確かめています…", error: "確かめられません: " + esc(u.problem || ""), off: esc(u.detail || "更新は確かめません"), idle: "",
    }[u.state] || "";
    const m = document.createElement("div");
    m.className = "app-menu";
    m.id = "appMenu";
    m.setAttribute("role", "menu");
    m.innerHTML =
      '<div class="app-head"><b>版 ' + esc(st.info.version) + "</b>" +
      (ready ? '<span class="app-new">' + updLine + "</span>" : '<span class="app-sub">' + updLine + "</span>") + "</div>" +
      (ready ? '<button type="button" class="app-primary" data-act="reopen">' + REOPEN + "</button>" : "") +
      '<button type="button" data-act="check"' + (st.info.installed ? "" : " disabled") + '>新しい版を確かめる<small>' +
      (st.info.installed ? "前に確かめた時刻 " + esc((u.checkedAt || "-").slice(11)) : "この PC のアプリで開いたときに確かめます") + "</small></button>" +
      '<button type="button" data-act="shortcut"' + (st.install && st.install.installed ? "" : " disabled") + ">" +
      ((sc.links || []).length ? "ショートカットを作り直す" : "ショートカットを作る") + "<small>" + (where || "この PC のアプリで開いたときに作れます") + "</small></button>" +
      '<hr><button type="button" data-act="release">版の配布…<small>置き場に版を置く・配る版を選ぶ（管理する人）</small></button>';
    document.body.appendChild(m);
    const b = $("appChip").getBoundingClientRect();
    m.style.top = b.bottom + 6 + "px";
    m.style.left = Math.max(8, Math.min(innerWidth - m.offsetWidth - 8, b.left)) + "px";
    $("appChip").setAttribute("aria-expanded", "true");
    m.onclick = (e) => {
      const act = e.target.closest("button") && e.target.closest("button").dataset.act;
      if (!act) return;
      closeMenu();
      if (act === "reopen") reopen();
      else if (act === "check") { post("/__desktop/update/check").then(() => setTimeout(refresh, 1500)); $("state").textContent = "新しい版を確かめています…"; }
      else if (act === "shortcut") askShortcut(true);
      else if (act === "release") openRelease();
    };
  }
  document.addEventListener("click", (e) => { if ($("appMenu") && !e.target.closest("#appMenu")) closeMenu(); });
  document.addEventListener("keydown", (e) => { if (e.key === "Escape") closeMenu(); });

  // ------------------------------------------------------------ 窓（ダイアログ）
  function dialog(id, title, body, foot, narrow) {
    let back = $(id);
    if (back) back.remove();
    back = document.createElement("div");
    back.className = "dlg-back";
    back.id = id;
    back.innerHTML = '<div class="dlg ' + (narrow ? "app-narrow" : "narrow") + '" role="dialog" aria-modal="true">' +
      '<div class="dlg-head"><h2>' + title + '</h2><button class="btn dlg-close" type="button">閉じる</button></div>' +
      '<div class="dlg-body">' + body + "</div>" + (foot ? '<div class="dlg-foot">' + foot + "</div>" : "") + "</div>";
    document.body.appendChild(back);
    back.querySelector(".dlg-close").onclick = () => back.remove();
    return back;
  }

  // ------------------------------------------------------------ ショートカット（無ければ起動のたびに 1 度尋ねる・今後たずねないを覚える）
  function askShortcut(always) {
    const sc = (st.install && st.install.shortcut) || {};
    if (!always && !sc.ask) return;
    const places = (sc.places || []).map((p) => "<li>" + esc(p) + "</li>").join("");
    const d = dialog("appShortcut", "ショートカットを作りますか",
      '<p>この PC のアプリ（<code>' + esc(st.install.appDir) + "</code>）を開くショートカットを作ります。次からは共有フォルダに関係なく、ショートカットから開けます。" +
      "作業フォルダもこの PC のアプリにします。</p><p class=\"note\">作る場所</p><ul class=\"app-list\">" + places + "</ul>",
      '<button type="button" class="btn" data-a="never">今後たずねない</button><span class="note"></span>' +
      '<button type="button" class="btn" data-a="later">今回は作らない</button><button type="button" class="btn primary" data-a="make">作る</button>', true);
    const make = d.querySelector('[data-a="make"]');
    make.focus();
    d.querySelector('[data-a="later"]').onclick = () => d.remove();
    d.querySelector('[data-a="never"]').onclick = async () => { await post("/__desktop/shortcut/decline").catch(() => {}); d.remove(); refresh(); };
    make.onclick = async () => {
      try {
        const r = await post("/__desktop/shortcut");
        d.querySelector(".dlg-body").innerHTML = "<p><b>ショートカットを作りました。</b></p><ul class=\"app-list\">" +
          (r.made || []).map((p) => "<li>" + esc(p) + "</li>").join("") + "</ul>" +
          ((r.problems || []).length ? '<p class="note">作れなかった場所: ' + r.problems.map(esc).join(" / ") + "</p>" : "");
        d.querySelector(".dlg-foot").innerHTML = '<span class="note"></span><button type="button" class="btn primary">閉じる</button>';
        d.querySelector(".dlg-foot .btn").onclick = () => d.remove();
        refresh();
      } catch (e) {
        d.querySelector(".dlg-foot .note").textContent = "作れませんでした: " + e.message;
      }
    };
  }

  // ------------------------------------------------------------ 版の配布
  async function openRelease() {
    const d = dialog("appRelease", "版の配布", '<p class="note">更新の置き場を見ています…</p>', "");
    const draw = (v) => {
      const dist = v.distributed && v.distributed.version, prev = v.distributed && v.distributed.previous, cur = st.info.version;
      if (!v.reachable) {
        d.querySelector(".dlg-body").innerHTML = '<p class="app-warn">更新の置き場に届きません: ' + esc(v.why) + "</p>" +
          '<p class="note">置き場は <code>program\\app\\config.json</code> の <code>update.source</code> に書きます（共有フォルダ。「;」で候補を並べられます）。</p>';
        return;
      }
      const rows = (v.versions || []).map((r) => {
        const tags = (r.version === dist ? '<span class="app-tag on">配っている</span>' : "") + (r.version === prev ? '<span class="app-tag">前に配った</span>' : "") +
          (r.version === cur ? '<span class="app-tag">この PC</span>' : "");
        return "<tr><td><b>" + esc(r.version) + "</b> " + tags + "</td><td>" + esc((r.placedAt || "").replace("T", " ")) + "</td><td>" + esc(r.placedBy || "") +
          "</td><td class=\"num\">" + kb(r.bytes || 0) + '</td><td class="act">' +
          (r.version === dist ? "" : '<button type="button" class="btn small app-dist" data-dist="' + esc(r.version) + '">この版を配る</button>') +
          (r.version === dist ? "" : '<button type="button" class="btn small" data-rm="' + esc(r.version) + '">消す</button>') + "</td></tr>";
      }).join("");
      d.querySelector(".dlg-body").innerHTML =
        '<ol class="app-flow"><li><b>1. 置く</b><span>Releases から取った ZIP を選ぶと、中身を確かめて置き場に版のフォルダを作ります（まだ配りません）。</span>' +
        '<label class="btn">ZIP から版を置く…<input type="file" id="appZip" accept=".zip" hidden></label><span class="note" id="appPlace"></span></li>' +
        "<li><b>2. 配る</b><span>試してから「この版を配る」。各 PC は起動したとき・30 分ごとに確かめ、開き直すとその版になります。前の版を選び直せば戻せます。</span></li>" +
        "<li><b>3. 新しい PC</b><span>配る入口のアドレスを渡すだけで入ります。</span>" +
        (v.entry && v.entry.path ? '<code id="appEntry">' + esc(v.entry.path) + '</code><button type="button" class="btn small" id="appCopy">コピー</button>' : '<span class="note">（配る版を決めるとできます）</span>') + "</li></ol>" +
        '<table class="app-versions"><thead><tr><th>版</th><th>置いた日時</th><th>置いた人</th><th>大きさ</th><th></th></tr></thead><tbody>' +
        (rows || '<tr><td colspan="5" class="note">まだ版がありません。</td></tr>') + "</tbody></table>" +
        '<p class="note">置き場: ' + esc(v.found) + (v.adjusted ? "（この PC での見え方に合わせて探しました）" : "") + "　置けるのは、置き場の共有フォルダに書ける人です。</p>" +
        ((v.problems || []).length ? '<p class="app-warn">読めない物: ' + v.problems.map(esc).join(" / ") + "</p>" : "");
      const body = d.querySelector(".dlg-body");
      if ($("appCopy")) $("appCopy").onclick = () => navigator.clipboard.writeText($("appEntry").textContent).then(() => { $("appCopy").textContent = "コピーしました"; });
      body.querySelectorAll("[data-dist]").forEach((b) => b.onclick = async () => {
        if (!confirm("版 " + b.dataset.dist + " を配りますか？ 各 PC は開き直したときにこの版になります。")) return;
        try { const r = await post("/__desktop/release/distribute", { version: b.dataset.dist }); if ((r.notes || []).length) alert(r.notes.join("\n")); load(false); refresh(); } catch (e) { alert("配れませんでした: " + e.message); }
      });
      body.querySelectorAll("[data-rm]").forEach((b) => b.onclick = async () => {
        if (!confirm("版 " + b.dataset.rm + " を置き場から消しますか？")) return;
        try { await post("/__desktop/release/remove", { version: b.dataset.rm }); load(false); } catch (e) { alert("消せませんでした: " + e.message); }
      });
      $("appZip").onchange = async (e) => {
        const f = e.target.files[0];
        if (!f) return;
        const note = $("appPlace");
        note.textContent = "読み込んでいます…";
        const t0 = performance.now();
        const timer = setInterval(async () => {
          const p = await get("/__desktop/release/progress").catch(() => ({}));
          if (p.state === "running") note.textContent = ({ check: "中身を確かめています", copy: "置き場へ写しています", finish: "仕上げています" }[p.stage] || "") +
            (p.total ? "（" + kb(p.done) + " / " + kb(p.total) + "）" : "") + "… " + ((performance.now() - t0) / 1000).toFixed(0) + " 秒";
        }, 500);
        try {
          const r = await post("/__desktop/release/place?name=" + encodeURIComponent(f.name), await f.arrayBuffer(), true);
          note.textContent = "版 " + r.version + " を置きました。試してから「この版を配る」を押してください。";
          load(false);
        } catch (err) {
          note.textContent = "置けませんでした: " + err.message;
        } finally {
          clearInterval(timer);
        }
      };
    };
    const load = async (cached) => {
      try { draw(await get("/__desktop/release" + (cached ? "?cached=1" : ""))); } catch (e) { d.querySelector(".dlg-body").innerHTML = '<p class="app-warn">' + esc(e.message) + "</p>"; }
    };
    await load(true);
    load(false);
  }

  // ------------------------------------------------------------ 保存の知らせ（書き出したファイルの置き場とフォルダを開く）
  window.addEventListener("sv:saved", (e) => {
    const d = e.detail || {};
    if (!d.ok || !d.path) return;
    const s = $("state");
    s.innerHTML = "保存しました: " + esc(d.path) + ' <a href="#" id="appReveal">フォルダを開く</a>';
    $("appReveal").onclick = (ev) => { ev.preventDefault(); post("/__desktop/reveal", { path: d.path }).catch((err) => alert(err.message)); };
  });

  // ------------------------------------------------------------ 読み直し
  async function refresh() {
    try {
      [st.upd, st.install] = await Promise.all([get("/__desktop/update"), get("/__desktop/install")]);
    } catch (e) { return; }
    renderChip();
    if (!st.asked) { st.asked = true; askShortcut(false); }
  }

  (async () => {
    try { st.info = await get("/__desktop/info"); } catch (e) { return; }   // 窓の外（開発用の受け口）
    await refresh();
    setInterval(refresh, POLL_MS);
  })();
})();
