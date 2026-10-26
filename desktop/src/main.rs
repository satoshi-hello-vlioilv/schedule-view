//! CM3 スケジュールビューアの exe（Tauri の窓）。設計は program/docs/DESIGN.md。
//!
//! 役割: 窓・起動と終了・1 つだけ起動・画面（program/app/web）・アプリの中身（sv_core::app。CM3・台帳・列の設定・Excel）・
//! 配布と更新・ショートカット・外のリンク。画面の問い合わせ（`fetch("/api/...")`）は自前の仕組み（sv）で受け、ポートを開かない。
//! Python・Flask は使わない。
//!
//! 起動の流れ（LotScope と同じ決まり）:
//!   1. 共有・BOX・展開したフォルダの exe → この PC の作業場所の app へ写して、その exe を開き直す（起動画面に段を出す）
//!   2. app の exe（ショートカットの入口）→ 版ごとの写し（desktop\<印>\）へ渡す（動いている窓が app を掴まない）
//!   3. 版ごとの写し: 置き場に新しい版があれば取り込み、app と入れ替える（exe も変われば開き直す）→ 設定・台帳を読み、画面を開く

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod lnk;
mod proc;

use serde_json::{json, Value};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use sv_core::app::{App, Reply, Request};
use sv_core::brand::brand;
use sv_core::install;
use sv_core::locate;
use sv_core::update::{self, Paths};
use sv_core::updater::Updater;
use tauri::http::{Response, StatusCode};
use tauri::webview::{DownloadEvent, NewWindowResponse, PageLoadEvent};
use tauri::{AppHandle, Manager, RunEvent, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// 自前の仕組みの名前（画面は Windows で http://sv.localhost/、ほかは sv://localhost/）。
const SCHEME: &str = "sv";
const SELFTEST_JS: &str = include_str!("selftest.js");
const SELFTEST_LIMIT: Duration = Duration::from_secs(180);
/// 閉じる・開き直すとき、答えが画面へ届いてから閉じる。
const QUIT_DELAY: Duration = Duration::from_millis(150);
/// 起動の最初に置き場を確かめるとき、置き場に届くのを待つ長さ（過ぎたら待たずに開き、裏の確かめに任せる）。
const LAUNCH_CHECK_WAIT: Duration = Duration::from_secs(8);

/// 自己診断のときだけ、画面の読み込みより前に入れる見張り（画面のエラーを残す。selftest.js が読む）。
const ERROR_WATCH_JS: &str = "window.__svErrors=[];addEventListener('error',e=>__svErrors.push(String(e.message||e.type)));\
addEventListener('unhandledrejection',e=>__svErrors.push('promise: '+String(e.reason&&(e.reason.message||e.reason))));";

fn app_url(path: &str) -> Url {
    let base = if cfg!(windows) { "http://sv.localhost" } else { "sv://localhost" };
    Url::parse(&format!("{base}{path}")).expect("app url")
}

fn is_app_url(u: &Url) -> bool {
    u.scheme() == SCHEME || (matches!(u.scheme(), "http" | "https") && u.host_str() == Some("sv.localhost"))
}

/// 起動画面（desktop/splash。Tauri が tauri://localhost で出す）
fn is_splash(u: &Url) -> bool {
    u.scheme() == "tauri" || u.host_str() == Some("tauri.localhost")
}

/// 外のページ（ロット照会・CM3 の画面）はいつものブラウザで開く。開くのは http・https・mailto だけ（file: で exe を開かせない）
fn outside_ok(u: &Url) -> bool {
    matches!(u.scheme(), "http" | "https" | "mailto")
}

fn selftest_on() -> bool {
    std::env::var_os("SV_SELFTEST").is_some()
}

/// 窓の記録（この PC の作業場所の logs/desktop.log）。
fn log(line: &str) {
    let dir = locate::local_root().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("desktop.log")) {
        let _ = writeln!(f, "{} {line}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    }
}

/// 起動画面へ伝えること。画面が読み終わる前の分はためておき、読み終わったら流す。
#[derive(Default)]
struct Splash {
    loaded: AtomicBool,
    queue: Mutex<Vec<String>>,
}

impl Splash {
    fn say(&self, app: &AppHandle, js: String) {
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        if self.loaded.load(Ordering::SeqCst) {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.eval(&js);
            }
        } else {
            q.push(js);
        }
    }
    fn flush(&self, w: &WebviewWindow) {
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        self.loaded.store(true, Ordering::SeqCst);
        for js in q.drain(..) {
            let _ = w.eval(&js);
        }
    }
    fn step(&self, app: &AppHandle, id: &str, state: &str, detail: &str) {
        self.say(app, format!("splash.step(...{})", json!([id, state, detail])));
    }
    fn version(&self, app: &AppHandle, version: &str) {
        self.say(app, format!("splash.version(...{})", json!([version])));
    }
    fn fail(&self, app: &AppHandle, title: &str, detail: &str) {
        log(&format!("FAIL {title}: {}", detail.replace('\n', " / ")));
        self.say(app, format!("splash.fail(...{})", json!([title, detail])));
        selftest_finish(app, &json!({"ok": false, "error": format!("{title}: {detail}")}));
    }
}

/// 自己診断の結果を書いて終わる（SV_SELFTEST が無ければ何もしない）。
fn selftest_finish(app: &AppHandle, result: &Value) {
    if let Some(path) = std::env::var_os("SV_SELFTEST") {
        let _ = std::fs::write(&path, serde_json::to_vec_pretty(result).unwrap_or_default());
        app.exit(if result["ok"] == true { 0 } else { 1 });
    }
}

fn json_reply(v: &Value) -> Reply {
    Reply::json(200, v)
}

fn reply_of(r: Result<Value, (u16, String)>) -> Reply {
    match r {
        Ok(v) => json_reply(&v),
        Err((status, e)) => Reply::json(status, &json!({"error": e})),
    }
}

/// 動いている窓（画面からの問い合わせを受ける）。
struct Shell {
    app: Arc<App>,
    upd: Arc<Updater>,
    info: Value,
    handle: AppHandle,
}

impl Shell {
    /// 窓そのものが答える問い合わせ（/__desktop/…）。ほかはアプリの中身（App::handle）へ。
    fn handle(&self, req: &Request) -> Reply {
        let q = sv_core::app::parse_query(req.query);
        let body: Value = serde_json::from_slice(req.body).unwrap_or_default();
        let s = |k: &str| body[k].as_str().unwrap_or("").to_string();
        match (req.method, req.path) {
            ("GET", "/__desktop/info") => json_reply(&self.info),
            ("GET", "/__desktop/update") => json_reply(&self.upd.status()),
            ("POST", "/__desktop/update/check") => {
                self.upd.wake();
                json_reply(&json!({"ok": true}))
            }
            ("POST", "/__desktop/update/apply") => {
                if self.upd.status()["state"] != "ready" || !self.upd.installed {
                    return Reply::json(409, &json!({"error": "開き直して入れ替える版がありません。"}));
                }
                log(&format!("UPDATE 開き直して {} にします", self.upd.status()["ready"]));
                reply_of(relaunch(&self.handle, &self.upd.paths).map(|_| json!({"ok": true})).map_err(|e| (500, e)))
            }
            ("GET", "/__desktop/install") => {
                json_reply(&install::status(&self.upd.paths, &lnk::places(), &lnk::OsLinks, &names(), self.upd.installed))
            }
            ("POST", "/__desktop/shortcut") => {
                reply_of(install::make(&self.upd.paths, &lnk::places(), &lnk::OsLinks, &names()).map_err(|e| (400, e)))
            }
            ("POST", "/__desktop/shortcut/decline") => reply_of(install::decline(&self.upd.paths).map_err(|e| (500, e))),
            ("GET", "/__desktop/release") => {
                json_reply(&self.upd.release_view(q.iter().any(|(k, v)| k == "cached" && v == "1")))
            }
            ("GET", "/__desktop/release/progress") => json_reply(&self.upd.place_progress()),
            ("POST", "/__desktop/release/place") => {
                let name = q.iter().find(|(k, _)| k == "name").map(|(_, v)| v.clone()).unwrap_or_default();
                reply_of(self.upd.place(&name, req.body).map(|r| json!({"version": r["version"], "bytes": r["bytes"]})))
            }
            ("POST", "/__desktop/release/distribute") => reply_of(self.upd.distribute(&s("version"))),
            ("POST", "/__desktop/release/remove") => reply_of(self.upd.remove(&s("version"))),
            // 書き出したファイルの入ったフォルダを開き、そのファイルを選んだ状態にする
            ("POST", "/__desktop/reveal") => {
                use tauri_plugin_opener::OpenerExt;
                let path = s("path");
                if path.is_empty() || !std::path::Path::new(&path).exists() {
                    return Reply::json(404, &json!({"error": "そのファイルが見つかりません（移したか消したかもしれません）。"}));
                }
                reply_of(self.handle.opener().reveal_item_in_dir(&path).map(|_| json!({"ok": true})).map_err(|e| (500, e.to_string())))
            }
            ("GET", "/__desktop/downloads") => json_reply(&json!(downloads().lock().map(|v| v.clone()).unwrap_or_default())),
            ("POST", "/__desktop/selftest") => {
                selftest_finish(&self.handle, &body);
                json_reply(&json!({"received": true}))
            }
            _ => self.app.handle(req),
        }
    }
}

/// 答え → Tauri の答え。
fn to_response(r: Reply) -> Response<Vec<u8>> {
    let mut b = Response::builder().status(StatusCode::from_u16(r.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)).header("Content-Type", r.ctype);
    for (k, v) in r.headers {
        b = b.header(k, v);
    }
    b.body(r.body).unwrap_or_else(|_| Response::new(Vec::new()))
}

/// 入口（この PC の app の exe）で開き直して、この窓を閉じる。入口はこの窓が終わるのを待ってから、版ごとの写しへ渡す。
fn relaunch(app: &AppHandle, paths: &Paths) -> Result<(), String> {
    spawn_entry(paths)?;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(QUIT_DELAY);
        log("EXIT 開き直す（入口へ渡した）");
        app.exit(0);
    });
    Ok(())
}

/// app の exe（入口）を、この exe が終わるのを待つ印つきで起こす。
fn spawn_entry(paths: &Paths) -> Result<(), String> {
    let entry = paths.app().join(&brand().exe);
    std::process::Command::new(&entry)
        .args(["--wait-pid", &std::process::id().to_string()])
        .current_dir(paths.app())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("この PC のアプリを開けません（{}）: {e}", entry.display()))
}

/// ショートカットの名前とアイコン。
fn names() -> install::Names<'static> {
    static LEGACY: OnceLock<Vec<String>> = OnceLock::new();
    let b = brand();
    install::Names { name: &b.title, exe: &b.exe, description: &b.subtitle, legacy: LEGACY.get_or_init(Vec::new), icon: shortcut_icon().clone() }
}

/// exe に入れたアイコンの絵（build.rs が program/app/app_icon.py で描く）。
const ICON: &[u8] = include_bytes!("../icons/icon.ico");

/// ショートカットが指すアイコン: 作業場所の icon\<絵の指紋>.ico（絵が変わると場所も変わり、Windows が古い絵を出し続けない）。
fn shortcut_icon() -> &'static std::path::PathBuf {
    static I: OnceLock<std::path::PathBuf> = OnceLock::new();
    I.get_or_init(|| {
        let p = Paths::new(&locate::local_root());
        install::icon_file(&p, ICON).unwrap_or_else(|e| {
            log(&format!("SHORTCUT アイコンを書き出せません（exe の絵を使います）: {e}"));
            p.app().join(&brand().exe)
        })
    })
}

/// 保存（ダウンロード）の記録（自己診断のとき）。
fn downloads() -> &'static Mutex<Vec<Value>> {
    static D: OnceLock<Mutex<Vec<Value>>> = OnceLock::new();
    D.get_or_init(Mutex::default)
}

/// 保存の終わりを画面へ知らせる（sv:saved。画面が置いた場所と「フォルダを開く」を出す）。保存先は既定の「ダウンロード」。
fn on_download<R: tauri::Runtime>(wv: &tauri::Webview<R>, ev: DownloadEvent<'_>) -> bool {
    if let DownloadEvent::Finished { url, path, success } = &ev {
        let detail = json!({"path": path.as_ref().map(|p| p.display().to_string()), "ok": *success});
        let _ = wv.eval(format!("window.dispatchEvent(new CustomEvent('sv:saved', {{detail: {detail}}}))"));
        if selftest_on() {
            let body = path.as_ref().and_then(|p| std::fs::read(p).ok()).unwrap_or_default();
            let head: String = body.iter().take(4).map(|b| format!("{b:02x}")).collect();
            let row = json!({"state": "finished", "url": url.as_str(), "path": path.as_ref().map(|p| p.display().to_string()),
                             "success": success, "bytes": body.len(), "head": head});
            log(&format!("DOWNLOAD {row}"));
            if let Ok(mut v) = downloads().lock() {
                v.push(row);
            }
        }
    }
    true
}

/// 窓を作る（外のリンクはいつものブラウザで開く）。
fn window(app: &AppHandle, label: &str, url: WebviewUrl, splash: Arc<Splash>) -> tauri::Result<WebviewWindow> {
    let (handle, nav) = (app.clone(), app.clone());
    let b = brand();
    let builder = WebviewWindowBuilder::new(app, label, url)
        .title(format!("{} — {}", b.title, b.subtitle))
        .inner_size(1600.0, 1000.0)
        .min_inner_size(1100.0, 700.0)
        .maximized(true)
        // 起動画面と同じ紺（開いた瞬間の白い光りを出さない）
        .background_color(tauri::window::Color(0x0b, 0x2a, 0x4a, 0xff))
        .disable_drag_drop_handler()
        .on_navigation(move |u| {
            let inside = is_app_url(u) || is_splash(u) || matches!(u.scheme(), "about" | "blob" | "data");
            if !inside && outside_ok(u) {
                use tauri_plugin_opener::OpenerExt;
                let _ = nav.opener().open_url(u.as_str(), None::<&str>);
            }
            inside
        })
        .on_new_window(move |u, _features| {
            if outside_ok(&u) && !is_app_url(&u) {
                use tauri_plugin_opener::OpenerExt;
                let _ = handle.opener().open_url(u.as_str(), None::<&str>);
            }
            NewWindowResponse::Deny
        })
        .on_page_load(move |w, p| {
            if p.event() != PageLoadEvent::Finished {
                return;
            }
            if is_splash(p.url()) {
                splash.flush(&w);
            } else if is_app_url(p.url()) && p.url().path() == "/" && selftest_on() {
                let _ = w.eval(SELFTEST_JS);
            }
        })
        .on_download(|wv, ev| on_download(&wv, ev));
    let builder = if selftest_on() { builder.initialization_script(ERROR_WATCH_JS) } else { builder };
    builder.build()
}

/// 窓の中でする、この PC へ写す仕事。
#[derive(Debug)]
enum Job {
    /// 共有・BOX・展開したフォルダの配る形をこの PC の app へ写す
    Localize(std::path::PathBuf),
    /// 配る入口（置き場のアプリのフォルダ）から、配る版を取り込んで app に据える
    Entry(std::path::PathBuf),
}

fn job() -> &'static OnceLock<Job> {
    static J: OnceLock<Job> = OnceLock::new();
    &J
}

/// 写す・取り込む仕事。済めば入口を開いて窓を閉じ true。写せなければ（共有のまま開くなら）false。
fn run_job(app: &AppHandle, splash: &Splash, job: &Job) -> bool {
    let b = brand();
    let paths = Paths::new(&locate::local_root());
    let result = match job {
        Job::Localize(dist) => {
            let version = update::validate_dist(dist, &b.exe, &b.app_id, None).unwrap_or_default();
            if !update::should_localize(&paths, &version) {
                Ok(format!("この PC のアプリ（{}）で開きます", paths.app().display()))
            } else {
                splash.step(app, "program", "now", &format!("この PC へ写しています（{} → {}）…", dist.display(), paths.app().display()));
                update::localize(&paths, dist, &b.exe, &b.app_id).map(|v| {
                    log(&format!("INSTALL {} からこの PC へ写しました（版 {v}）", dist.display()));
                    format!("版 {v} をこの PC へ写しました")
                })
            }
        }
        Job::Entry(folder) => {
            splash.step(app, "program", "now", &format!("配る版を確かめています（{}）…", folder.display()));
            sv_core::distribute::install_from_entry(&paths, folder, &b.app_id, &|t| splash.step(app, "program", "now", t)).map(|v| {
                log(&format!("INSTALL 配る入口（{}）から版 {v} をこの PC へ据えました", folder.display()));
                format!("配る版 {v} をこの PC へ写しました")
            })
        }
    };
    match result {
        Ok(text) => {
            splash.step(app, "program", "ok", &text);
            splash.step(app, "open", "now", "この PC のアプリで開いています…");
            match spawn_entry(&paths) {
                Ok(()) => {
                    let app = app.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(QUIT_DELAY);
                        app.exit(0);
                    });
                }
                Err(e) => splash.fail(app, "この PC のアプリを開けません", &e),
            }
            true
        }
        Err(e) if matches!(job, Job::Localize(_)) => {
            log(&format!("INSTALL この PC へ写せないので、共有のまま開きます: {e}"));
            false
        }
        Err(e) => {
            splash.fail(app, "配る版を取り込めません", &e);
            true
        }
    }
}

/// 起動の最初（画面を開く前）: 控えた置き場に新しい版（配る版）があれば、その場で取り込む（このあと apply_staged が入れ替える）。
fn launch_update(app: &AppHandle, splash: &Splash, paths: &Paths, source: &str, notes: &mut Vec<String>) {
    let source = sv_core::updater::source_with_entry(paths, source);
    if source.trim().is_empty() {
        return;
    }
    let current = update::program_version(&paths.app().join("program")).unwrap_or_default();
    splash.step(app, "update", "now", "新しい版が配られていないか確かめています…");
    let progress = |t: &str| splash.step(app, "update", "now", &format!("{t}…"));
    match update::launch_check(paths, &source, &brand().app_id, &current, LAUNCH_CHECK_WAIT, &progress) {
        update::Launch::Staged(v) => log(&format!("UPDATE 起動の最初に版 {v} を取り込みました")),
        update::Launch::Nothing(why) => log(&format!("UPDATE 起動の最初の確かめ: {why}")),
        update::Launch::Slow => log(&format!("UPDATE 起動の最初の確かめ: 置き場に {} 秒で届かないので、裏で確かめます", LAUNCH_CHECK_WAIT.as_secs())),
        update::Launch::Failed(e) => {
            log(&format!("UPDATE 起動の最初の確かめ: {e}"));
            notes.push(format!("{e}（使っている間に、もう一度試します）"));
        }
    }
}

fn same_dir(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// 画面の版の印（静的ファイルの ?v=）: 版＋作ったコミット＋起動の時刻（開き直せば、古い JS・CSS を使い回さない）。
fn build_mark(version: &str) -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let commit: String = env!("SV_BUILD_COMMIT").chars().take(7).collect();
    format!("{version}-{commit}-{t:x}")
}

/// 中身を用意し、準備できたら主の窓を画面へ切り替える（裏の糸で。窓は先に出しておく）。
fn start(app: AppHandle, slot: Arc<OnceLock<Shell>>, splash: Arc<Splash>) {
    if let Some(j) = job().get() {
        if run_job(&app, &splash, j) {
            return;
        }
    }
    let program = match locate::program_dir() {
        Ok(p) => p,
        Err(e) => return splash.fail(&app, "アプリの中身が見つかりません", &e),
    };
    splash.step(&app, "program", "ok", &program.display().to_string());
    let paths = Paths::new(&locate::local_root());
    let installed = same_dir(&program, &paths.app().join("program"));
    let config = locate::config_file(&program);
    let source = sv_core::store::read_json(&config).and_then(|c| c["update"]["source"].as_str().map(str::to_string)).unwrap_or_default();
    let mut notes = vec![];
    if installed {
        launch_update(&app, &splash, &paths, &source, &mut notes);
        match update::apply_staged(&paths, &brand().exe) {
            Ok(Some(a)) => {
                log(&format!("UPDATE {} → {} に入れ替えました", a.from, a.to));
                splash.step(&app, "update", "ok", &format!("版 {} → {} に入れ替えました", a.from, a.to));
                if a.exe_changed {
                    splash.step(&app, "open", "now", "exe も新しくなったので、開き直しています…");
                    if relaunch(&app, &paths).is_ok() {
                        return;
                    }
                }
            }
            Ok(None) => splash.step(&app, "update", "ok", "いまの版で開きます"),
            Err(e) => {
                log(&format!("UPDATE 入れ替えられません: {e}"));
                notes.push(format!("新しい版に入れ替えられませんでした（次に開いたときにもう一度試します）: {e}"));
                splash.step(&app, "update", "warn", "入れ替えられませんでした（いまの版で開きます）");
            }
        }
    } else {
        splash.step(&app, "update", "ok", "この PC のアプリではないので、更新は確かめません");
    }
    let version = update::program_version(&program).unwrap_or_else(|| "?".into());
    splash.version(&app, &version);
    splash.step(&app, "data", "now", "設定と台帳を読んでいます…");
    let t0 = Instant::now();
    let checked = if program.join("app").join("web").join("index.html").is_file() {
        App::new(sv_core::app::Paths { program: program.clone(), local_root: locate::local_root(), config: config.clone() }, build_mark(&version))
    } else {
        Err(format!("画面（{}）がありません", program.join("app").join("web").join("index.html").display()))
    };
    let core = match checked {
        Ok(a) => a,
        Err(e) => {
            // 入れ替えた直後の版が開けなかった → 控え（直前の版）へ戻して開き直す
            if installed {
                if let Some(back) = update::revert(&paths) {
                    log(&format!("UPDATE 版 {} が開けないので {} に戻しました: {e}", back.from, back.to));
                    splash.step(&app, "data", "warn", &format!("版 {} が開けないので、前の版 {} に戻して開き直します", back.from, back.to));
                    if relaunch(&app, &paths).is_ok() {
                        return;
                    }
                }
            }
            splash.step(&app, "data", "bad", "読めません");
            return splash.fail(&app, "設定を読めません", &e);
        }
    };
    // 台帳は裏で写し・読み込む（画面は待たない。読み終われば画面が読み直す）
    core.start();
    if installed {
        update::confirm(&paths);
        std::thread::spawn(|| {
            let p = Paths::new(&locate::local_root());
            for f in install::maintain(&p, &lnk::places(), &lnk::pinned(), &lnk::OsLinks, &names()) {
                log(&format!("SHORTCUT {f}"));
            }
        });
        if let Ok(me) = std::env::current_exe() {
            std::thread::spawn(move || update::cleanup_copies(&Paths::new(&locate::local_root()), &me));
        }
    }
    log(&format!("READY version={version} program={} elapsed_ms={}", program.display(), t0.elapsed().as_millis()));
    splash.step(&app, "data", "ok", &format!("版 {version} ・ {:.1} 秒", t0.elapsed().as_secs_f64()));
    let info = json!({
        "shell": "tauri", "app_id": brand().app_id, "version": version, "shell_version": env!("CARGO_PKG_VERSION"),
        "commit": env!("SV_BUILD_COMMIT"), "program": program, "config": config, "local_root": locate::local_root(),
        "exe": std::env::current_exe().unwrap_or_default(), "url": app_url("/").as_str(), "installed": installed, "app": paths.app(),
        "settings": core.settings_dir(),
    });
    let upd = Updater::new(paths, installed, version, source, notes);
    upd.run(log);
    let _ = slot.set(Shell { app: core, upd, info, handle: app.clone() });
    splash.step(&app, "open", "now", "画面を開いています…");
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.navigate(app_url("/"));
    }
}

fn arg_u32(name: &str) -> Option<u32> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok())
}

/// 開発の作業ツリー（program の隣に .git）・引き継がない指定（--here・SV_NO_HANDOFF）。この PC へ写さず、そのまま動く。
fn dev_mode() -> bool {
    std::env::args().any(|a| a == "--here")
        || std::env::var_os("SV_NO_HANDOFF").is_some()
        || locate::program_dir().is_ok_and(|p| p.parent().is_some_and(|r| r.join(".git").exists()))
}

/// 起動のはじめ（窓を作る前）: 入口なら版ごとの写しへ渡す。共有などの配る形・配る入口は窓の中で写す（起動画面に段を出す）。
/// 渡したら true（この exe は終わる）。
fn hand_off() -> bool {
    if let Some(pid) = arg_u32("--wait-pid") {
        proc::wait_gone(pid, Duration::from_secs(30));
    }
    let Ok(me) = std::env::current_exe() else { return false };
    let b = brand();
    let paths = Paths::new(&locate::local_root());
    if !dev_mode() {
        if let Some(folder) = sv_core::distribute::entry_folder(&me, &b.app_id) {
            let _ = job().set(Job::Entry(folder));
            return false;
        }
    }
    let pid = std::process::id().to_string();
    match update::plan(&paths, &me, &b.exe, dev_mode()) {
        update::Step::Stay => false,
        update::Step::Handoff => match update::copy_for_run(&paths, &me) {
            Ok(copy) => {
                let program = paths.app().join("program").display().to_string();
                std::process::Command::new(&copy)
                    .args([locate::PROGRAM_ARG, &program, "--wait-pid", &pid])
                    .current_dir(copy.parent().unwrap_or(std::path::Path::new(".")))
                    .spawn()
                    .is_ok()
            }
            Err(e) => {
                log(&format!("START 版ごとの写しを作れないので、入口のまま開きます: {e}"));
                false
            }
        },
        update::Step::Localize(dist) => {
            let _ = job().set(Job::Localize(dist));
            false
        }
    }
}

fn main() {
    if hand_off() {
        return;
    }
    let slot: Arc<OnceLock<Shell>> = Arc::default();
    let splash: Arc<Splash> = Arc::default();
    let proto_slot = slot.clone();

    let app = tauri::Builder::default()
        // 2 つめを起こしたら、前の窓を前に出すだけ
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        // 画面からの問い合わせ。1 つずつ別の糸で答える（CM3 を待つ問い合わせが、ほかを待たせない）
        .register_asynchronous_uri_scheme_protocol(SCHEME, move |_ctx, req, responder| {
            let slot = proto_slot.clone();
            std::thread::spawn(move || {
                let reply = match slot.get() {
                    Some(sh) => {
                        let uri = req.uri();
                        let path = uri.path().to_string();
                        let query = uri.query().unwrap_or("").to_string();
                        sh.handle(&Request { method: req.method().as_str(), path: &path, query: &query, body: req.body() })
                    }
                    None => Reply::json(503, &json!({"success": false, "message": "起動中です。"})),
                };
                responder.respond(to_response(reply));
            });
        })
        .setup({
            let slot = slot.clone();
            let splash = splash.clone();
            move |app| {
                let handle = app.handle().clone();
                window(&handle, "main", WebviewUrl::App("index.html".into()), splash.clone())?;
                splash.say(&handle, format!("splash.brand(...{})", json!([brand().title, brand().subtitle])));
                if selftest_on() {
                    let h = handle.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(SELFTEST_LIMIT);
                        selftest_finish(&h, &json!({"ok": false, "error": format!("{} 秒で終わりませんでした", SELFTEST_LIMIT.as_secs())}));
                    });
                }
                std::thread::spawn(move || start(handle, slot, splash));
                Ok(())
            }
        })
        .build(tauri::generate_context!())
        .expect("起動できません");

    app.run(|_app, event| {
        if let RunEvent::Exit = event {
            log("EXIT 窓を閉じた");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_of_the_app_and_the_splash() {
        assert!(is_app_url(&app_url("/api/boot")), "自分の画面");
        assert!(is_app_url(&Url::parse("http://sv.localhost/x").unwrap()), "Windows（WebView2）の形");
        assert!(is_app_url(&Url::parse("sv://localhost/x").unwrap()), "ほかの OS の形");
        assert!(!is_app_url(&Url::parse("http://nlmfangyweb1a/lotdsp/").unwrap()), "外のページ");
        assert!(!is_app_url(&Url::parse("file://sv.localhost/x").unwrap()), "名前が同じでも file: はアプリの中ではない");
        assert!(outside_ok(&Url::parse("http://nlmfangyweb1a:9085/Cm3SchWeb/").unwrap()));
        assert!(!outside_ok(&Url::parse("file://///server/share/x.exe").unwrap()), "file: は外へ渡さない（exe を開かせない）");
        assert!(is_splash(&Url::parse("tauri://localhost/index.html").unwrap()));
    }
}
