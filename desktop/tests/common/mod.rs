//! 試験の道具: 作り物のデータ（program/tools/fixture.py）・作り物の CM3（小さな HTTP）・アプリを作る。
#![allow(dead_code)]

use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use sv_core::app::{App, Paths};

pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

pub fn python() -> String {
    std::env::var("SV_PYTHON").unwrap_or_else(|_| if cfg!(windows) { "python".into() } else { "python3".into() })
}

/// 試験ごとの一時フォルダ（前の残りは消す）。
pub fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sv-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 作り物のデータを作る（cm3/<設備>.json・LOTNOW.sqlite3）。
pub fn fixture(dir: &Path, rows: usize) {
    let st = std::process::Command::new(python())
        .arg(repo().join("program/tools/fixture.py"))
        .arg(dir)
        .arg(rows.to_string())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("Python で fixture.py を動かせません（SV_PYTHON で場所を渡せます）");
    assert!(st.status.success(), "fixture.py: {}", String::from_utf8_lossy(&st.stderr));
}

/// 作り物の CM3（要求の setsubiName の応答 <dir>/cm3/<設備>.json を返す）。delay は答える前の待ち。→ 根の URL
pub fn mock_cm3(dir: &Path, delay: Duration) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let dir = dir.to_path_buf();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let dir = dir.clone();
            std::thread::spawn(move || {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                let _ = r.read_line(&mut line);
                let mut len = 0usize;
                loop {
                    let mut h = String::new();
                    if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0u8; len];
                let _ = r.read_exact(&mut body);
                let eq = serde_json::from_slice::<Value>(&body).ok().and_then(|v| v["setsubiName"].as_str().map(str::to_string)).unwrap_or_default();
                std::thread::sleep(delay);
                let mut s = s;
                match std::fs::read(dir.join("cm3").join(format!("{eq}.json"))) {
                    Ok(data) if line.contains("/init") => {
                        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/json;charset=UTF-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len());
                        let _ = s.write_all(&data);
                    }
                    _ => {
                        let _ = write!(s, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    }
                }
            });
        }
    });
    url
}

/// アプリを作る（config.json を一時フォルダへ写し、CM3 と台帳を作り物に向ける）。
pub fn app(work: &Path, data: &Path, cm3_url: &str, edit: impl Fn(&mut Value)) -> Arc<App> {
    let mut cfg: Value = serde_json::from_slice(&std::fs::read(repo().join("program/app/config.json")).unwrap()).unwrap();
    cfg["cm3_base_url"] = Value::String(cm3_url.into());
    cfg["master"]["source_path"] = Value::String(data.join("LOTNOW.sqlite3").display().to_string());
    edit(&mut cfg);
    let config = work.join("config.json");
    std::fs::write(&config, serde_json::to_vec_pretty(&cfg).unwrap()).unwrap();
    let paths = Paths { program: repo().join("program"), local_root: work.join("local"), config };
    App::new(paths, "test".into()).unwrap()
}
