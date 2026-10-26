//! 更新の係（裏の糸と画面への答え）。取り込み・入れ替えの中身は update、置き場の探し方は release、置き場の形は distribute。
//!
//! - 置き場は現場の設定（config.json の `update.source`。「;」で候補を並べられる・%環境変数% も使える）と、配る入口から入れた PC が
//!   控えた入口の置き場（後ろの候補）
//! - 確かめる: 起動の直後・30 分ごと・画面の「確かめる」。配る版が決まっていればその版（前の版へ戻すときも）、無ければいちばん新しい版を
//!   使っている間に取り込む（決め方は update::pick）。入れ替えは次の起動の最初（または画面の「更新して開き直す」）
//! - 置き場を控える（update::remember_source）。次の起動の最初に、窓を出す前にも確かめる（閉じている間に配った版を 1 回の起動で入れる）
//! - 配布の画面（release_view）は、置き場をたどった結果を覚える。cached なら覚えた結果をすぐ返す（共有をたどるのは遅いことがある）
//! - 版を置く・配る・消すのは、置き場に書ける人（共有フォルダの権限で決まる。書けなければ理由を返す）

use crate::brand::brand;
use crate::distribute::{self, Who};
use crate::release;
use crate::update::{self, Paths};
use serde_json::{json, Map, Value};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// 確かめる間隔。
const EVERY: Duration = Duration::from_secs(30 * 60);

pub struct Updater {
    pub paths: Paths,
    /// この PC のアプリ（app）で動いているか。開発の作業ツリー・共有から動くときは取り込まない
    pub installed: bool,
    /// いま動いている版
    pub current: String,
    /// 置き場の設定（config.json の update.source）
    source: String,
    state: Mutex<Map<String, Value>>,
    wake: (Mutex<bool>, Condvar),
    /// 版を置いている最中の進み具合（1 台で同時に置けるのは 1 本）
    placing: Mutex<Option<Value>>,
    /// 配布の画面の答えの覚え（置き場の設定, 答え, たどった時刻）
    view_cache: Mutex<Option<(String, Value, String)>>,
}

/// 置き場の設定に、配る入口から入れた PC が控えた置き場を後ろの候補として足す。
pub fn source_with_entry(paths: &Paths, source: &str) -> String {
    match distribute::remembered_entry(paths) {
        Some(dir) if !source.contains(&*dir.to_string_lossy()) => {
            if source.trim().is_empty() {
                dir.display().to_string()
            } else {
                format!("{source};{}", dir.display())
            }
        }
        _ => source.to_string(),
    }
}

/// だれが（Windows のログイン名と PC 名）。
pub fn who() -> Who {
    let var = |a: &str, b: &str| std::env::var(a).or_else(|_| std::env::var(b)).unwrap_or_default();
    Who { login: var("USERNAME", "USER"), pc: var("COMPUTERNAME", "HOSTNAME") }
}

fn now_iso() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

impl Updater {
    pub fn new(paths: Paths, installed: bool, current: String, source: String, notes: Vec<String>) -> Arc<Updater> {
        let mut st = Map::new();
        st.insert("state".into(), json!(if installed { "idle" } else { "off" }));
        st.insert("current".into(), json!(current));
        st.insert("notes".into(), json!(notes));
        if !installed {
            st.insert("detail".into(), json!("この PC のアプリ（作業場所の app）で動いていないので、更新は取り込みません"));
        }
        Arc::new(Updater {
            paths,
            installed,
            current,
            source,
            state: Mutex::new(st),
            wake: (Mutex::new(false), Condvar::new()),
            placing: Mutex::new(None),
            view_cache: Mutex::new(None),
        })
    }

    /// 置き場（設定＋入口の控え）。
    pub fn source(&self) -> String {
        source_with_entry(&self.paths, &self.source)
    }

    /// 画面へ（GET /__desktop/update）。
    pub fn status(&self) -> Value {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner()).clone();
        s.insert("failed".into(), json!(update::failed_versions(&self.paths)));
        s.insert("installed".into(), json!(self.installed));
        Value::Object(s)
    }

    fn set(&self, pairs: Value) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        for k in ["ready", "direction", "detail", "problem"] {
            s.remove(k);
        }
        if let Value::Object(m) = pairs {
            s.extend(m);
        }
    }

    /// すぐ確かめる（画面の「確かめる」）。
    pub fn wake(&self) {
        *self.wake.0.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.wake.1.notify_all();
    }

    /// 裏の糸: 確かめて、間隔を空けて繰り返す。
    pub fn run(self: &Arc<Self>, log: fn(&str)) {
        if !self.installed {
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || loop {
            me.check(log);
            let (m, cv) = (&me.wake.0, &me.wake.1);
            let g = m.lock().unwrap_or_else(|e| e.into_inner());
            let (mut g, _) = cv.wait_timeout_while(g, EVERY, |w| !*w).unwrap_or_else(|e| e.into_inner());
            *g = false;
        });
    }

    /// 1 回確かめる（取り込みまで）。
    pub fn check(&self, log: fn(&str)) {
        let b = brand();
        self.set(json!({"state": "checking"}));
        let source = self.source();
        {
            let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
            s.insert("source".into(), json!(source));
            s.insert("checkedAt".into(), json!(chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()));
        }
        update::remember_source(&self.paths, &source);
        if source.trim().is_empty() {
            let _ = std::fs::remove_file(self.paths.want());
            return self.set(json!({"state": "off", "detail": "更新の置き場が決まっていません（config.json の update.source）"}));
        }
        let (found, adjusted) = match release::locate_source(&source) {
            Ok(f) => f,
            Err(e) => return self.set(json!({"state": "error", "problem": e})),
        };
        let scan = release::scan(&found, &b.app_id);
        self.remember_view(&source, distribute::view_of(&scan, &found, adjusted, &source, &b.name, Value::Null));
        {
            let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
            s.insert("found".into(), json!(found.display().to_string()));
            s.insert("adjusted".into(), json!(adjusted));
            s.insert("distributed".into(), scan.distribute.as_ref().map(|d| d.raw.clone()).unwrap_or(Value::Null));
        }
        match &scan.distribute {
            Some(d) => {
                let _ = update::write_json(&self.paths.want(), &d.raw);
            }
            None => {
                let _ = std::fs::remove_file(self.paths.want());
            }
        }
        let (target, direction) = match update::pick(&scan, &update::failed_versions(&self.paths), &self.current) {
            update::Pick::Take(t, d) => (t, d),
            update::Pick::Stay(why) => {
                // いまの版が最新なら、取り込みかけの別の版は捨てる（配る版が戻された等）
                if why.is_none() && update::staged(&self.paths).is_some() {
                    let _ = std::fs::remove_dir_all(self.paths.stage());
                    let _ = std::fs::remove_file(self.paths.stage_record());
                }
                return self.set(match why {
                    Some(w) => json!({"state": "latest", "detail": w}),
                    None => json!({"state": "latest"}),
                });
            }
        };
        if update::staged(&self.paths).is_some_and(|s| s["version"] == json!(target.version)) {
            return self.set(json!({"state": "ready", "ready": target.version, "direction": direction}));
        }
        self.set(json!({"state": "staging", "detail": target.version}));
        match update::stage_release(&self.paths, target, &b.app_id, &|_| {}) {
            Ok(_) => {
                log(&format!("UPDATE 版 {} を取り込みました（{}）", target.version, target.zip_path().display()));
                self.set(json!({"state": "ready", "ready": target.version, "direction": direction}));
            }
            Err(e) => {
                log(&format!("UPDATE 版 {} を取り込めません: {e}", target.version));
                self.set(json!({"state": "error", "problem": format!("版 {} を取り込めません: {e}", target.version)}));
            }
        }
    }

    fn me(&self) -> Value {
        let w = who();
        json!({"login": w.login, "pc": w.pc, "current": self.current, "installed": self.installed})
    }

    /// 配布の画面（GET /__desktop/release）。cached なら覚えた答え（置き場が同じとき）。
    pub fn release_view(&self, cached: bool) -> Value {
        let b = brand();
        let source = self.source();
        let stamp = |mut v: Value, at: Option<&str>| {
            v["me"] = self.me();
            v["cached"] = json!(at.is_some());
            v["checkedAt"] = json!(at.map(str::to_string).unwrap_or_else(now_iso));
            v
        };
        if cached {
            if let Some((src, v, at)) = self.view_cache.lock().unwrap_or_else(|e| e.into_inner()).clone() {
                if src == source {
                    return stamp(v, Some(&at));
                }
            }
        }
        let v = match release::locate_source(&source) {
            Ok((found, adjusted)) => distribute::view(&found, adjusted, &source, &b.app_id, &b.name, Value::Null),
            Err(e) => json!({"reachable": false, "why": e, "folder": source, "found": null, "versions": []}),
        };
        self.remember_view(&source, v.clone());
        stamp(v, None)
    }

    fn remember_view(&self, source: &str, v: Value) {
        *self.view_cache.lock().unwrap_or_else(|e| e.into_inner()) = Some((source.to_string(), v, now_iso()));
    }

    /// 置いている最中の進み具合（GET /__desktop/release/progress）。
    pub fn place_progress(&self) -> Value {
        self.placing.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(|| json!({"state": "idle"}))
    }

    /// 画面から選んだ ZIP（読み終えたバイト列）を置き場に置く（POST /__desktop/release/place）。→ release.json か (状態, 理由)
    pub fn place(&self, file_name: &str, body: &[u8]) -> Result<Value, (u16, String)> {
        let b = brand();
        let (found, _) = release::locate_source(&self.source()).map_err(|e| (400, e))?;
        {
            let mut g = self.placing.lock().unwrap_or_else(|e| e.into_inner());
            if g.as_ref().is_some_and(|v| v["state"] == "running") {
                return Err((409, "ほかの版を置いている最中です。終わってからもう一度選んでください。".into()));
            }
            *g = Some(json!({"state": "running", "stage": "check", "done": 0, "total": 0, "elapsed": 0.0}));
        }
        let t0 = std::time::Instant::now();
        let dl = self.paths.work().join(format!("place-{}.zip", std::process::id()));
        let result = (|| {
            std::fs::create_dir_all(self.paths.work()).map_err(|e| e.to_string())?;
            std::fs::write(&dl, body).map_err(|e| format!("受け取った ZIP を書けません: {e}"))?;
            let progress = |stage: &str, done: u64, total: u64| {
                *self.placing.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some(json!({"state": "running", "stage": stage, "done": done, "total": total, "elapsed": t0.elapsed().as_secs_f64()}));
            };
            distribute::place(&self.paths, &found, &dl, file_name, &b.app_id, &who(), &progress)
        })();
        let _ = std::fs::remove_file(&dl);
        let state = if result.is_ok() { "done" } else { "failed" };
        *self.placing.lock().unwrap_or_else(|e| e.into_inner()) = Some(json!({"state": state, "elapsed": t0.elapsed().as_secs_f64()}));
        if result.is_ok() {
            self.wake();
        }
        result.map_err(|e| (400, e))
    }

    /// 配る版を決める（POST /__desktop/release/distribute）。
    pub fn distribute(&self, version: &str) -> Result<Value, (u16, String)> {
        let (found, _) = release::locate_source(&self.source()).map_err(|e| (400, e))?;
        let r = distribute::set_distributed(&found, version, &brand().app_id, &who()).map_err(|e| (400, e))?;
        self.wake();
        Ok(r)
    }

    /// 置いた版を消す（POST /__desktop/release/remove）。配っている版は消さない。
    pub fn remove(&self, version: &str) -> Result<Value, (u16, String)> {
        let (found, _) = release::locate_source(&self.source()).map_err(|e| (400, e))?;
        let r = distribute::remove_version(&found, version, &brand().app_id).map_err(|e| (400, e))?;
        self.wake();
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(dir: &std::path::Path, version: &str) {
        let d = dir.join("App").join(version);
        std::fs::create_dir_all(&d).unwrap();
        let rel = json!({"schema": 1, "app_id": brand().app_id, "name": "App", "version": version, "zip": format!("App-{version}.zip"),
                         "sha256": "a".repeat(64), "exe": "App.exe", "bytes": 10});
        std::fs::write(d.join(release::MARKER), serde_json::to_vec(&rel).unwrap()).unwrap();
    }

    #[test]
    fn release_view_answers_from_memory_when_asked_and_rescans_otherwise() {
        let d = std::env::temp_dir().join(format!("sv-updater-view-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let rels = d.join("Releases");
        put(&rels, "3.1.0");
        let upd = Updater::new(Paths::new(&d.join("local")), false, "3.1.0".into(), rels.display().to_string(), vec![]);
        let n = |v: &Value| v["versions"].as_array().map(Vec::len).unwrap_or(0);
        let first = upd.release_view(true);
        assert_eq!((n(&first), first["cached"].as_bool()), (1, Some(false)), "覚えが無ければたどる: {first}");
        put(&rels, "3.2.0");
        let mem = upd.release_view(true);
        assert_eq!((n(&mem), mem["cached"].as_bool()), (1, Some(true)), "cached は覚えた答え（置き場をたどらない）");
        assert_eq!(mem["checkedAt"], first["checkedAt"], "いつたどった答えかを言う");
        let fresh = upd.release_view(false);
        assert_eq!((n(&fresh), fresh["cached"].as_bool()), (2, Some(false)), "cached なしはたどり直す");
        let none = Updater::new(Paths::new(&d.join("local2")), false, "3.1.0".into(), d.join("無い").display().to_string(), vec![]);
        let v = none.release_view(true);
        assert_eq!(v["reachable"], false, "届かない置き場は理由: {v}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn checking_stages_the_newest_version_and_reports_ready() {
        let d = std::env::temp_dir().join(format!("sv-updater-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let p = Paths::new(&d.join("local"));
        let off = Updater::new(p.clone(), true, "1.0.0".into(), String::new(), vec![]);
        off.check(|_| {});
        assert_eq!(off.status()["state"], "off", "置き場が決まっていなければ取り込まない");
        let missing = Updater::new(p, true, "1.0.0".into(), d.join("無い").display().to_string(), vec![]);
        missing.check(|_| {});
        assert_eq!(missing.status()["state"], "error");
        assert!(missing.status()["problem"].as_str().unwrap_or("").contains("見つかりません"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 使っている間に、置き場の新しい版を裏で取り込み（ready）、次の起動の最初に入れ替える
    #[test]
    fn a_version_placed_while_running_is_staged_then_applied_on_the_next_start() {
        use std::io::Write;
        let d = std::env::temp_dir().join(format!("sv-updater-stage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let b = brand();
        let dist = |dir: &std::path::Path, v: &str| {
            std::fs::create_dir_all(dir.join("program").join("app")).unwrap();
            std::fs::write(dir.join(&b.exe), "exe").unwrap();
            std::fs::write(dir.join("program").join("app").join("brand.json"), format!(r#"{{"app_id":"{}"}}"#, b.app_id)).unwrap();
            std::fs::write(dir.join("program").join("app").join("version.py"), format!("APP_VERSION = \"{v}\"\n")).unwrap();
        };
        let p = Paths::new(&d.join("local"));
        dist(&d.join("share"), "1.0.0");
        update::localize(&p, &d.join("share"), &b.exe, &b.app_id).unwrap();
        // 置き場に 1.0.1
        let rel = d.join("Releases").join("App").join("1.0.1");
        std::fs::create_dir_all(&rel).unwrap();
        dist(&d.join("new"), "1.0.1");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(rel.join("App.zip")).unwrap());
            for f in [b.exe.clone(), "program/app/brand.json".into(), "program/app/version.py".into()] {
                z.start_file(format!("App/{f}"), zip::write::SimpleFileOptions::default()).unwrap();
                z.write_all(&std::fs::read(d.join("new").join(&f)).unwrap()).unwrap();
            }
            z.finish().unwrap();
        }
        let sha = update::sha256_file(&rel.join("App.zip")).unwrap();
        let v = json!({"schema": 1, "app_id": b.app_id, "name": "App", "version": "1.0.1", "zip": "App.zip", "sha256": sha, "exe": b.exe});
        std::fs::write(rel.join(release::MARKER), serde_json::to_vec(&v).unwrap()).unwrap();
        let upd = Updater::new(p.clone(), true, "1.0.0".into(), d.join("Releases").display().to_string(), vec![]);
        upd.check(|_| {});
        let st = upd.status();
        assert_eq!((st["state"].as_str(), st["ready"].as_str()), (Some("ready"), Some("1.0.1")), "{st}");
        assert_eq!(update::program_version(&p.app().join("program")).as_deref(), Some("1.0.0"), "動いている app には触らない");
        assert_eq!(update::apply_staged(&p, &b.exe).unwrap().map(|a| a.to), Some("1.0.1".into()), "次の起動の最初に入れ替える");
        assert_eq!(update::remembered_source(&p), Some(d.join("Releases").display().to_string()), "置き場を控える");
        let _ = std::fs::remove_dir_all(&d);
    }
}
