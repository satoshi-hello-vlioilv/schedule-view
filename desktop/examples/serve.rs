//! 開発用: アプリの中身（App）をこの PC の中だけの HTTP で出す（ブラウザで画面を見る・撮る・測るため。配る exe は HTTP を開かない）。
//!
//!   cargo run --release --example serve -- [ポート=18080] [config.json] [作業場所]
//!   作り物で: python program/tools/mock_cm3.py <fixture のフォルダ> &  のあと、config.json の cm3_base_url を作り物へ向ける
//!
//! 環境変数 SV_FIXTURE=<fixture のフォルダ> を渡すと、作り物の CM3 を中で起こし、台帳もそのフォルダの物を使う（撮影の道具が使う）。

#[path = "../tests/common/mod.rs"]
mod common;

use std::path::PathBuf;
use std::time::Duration;
use sv_core::app::{App, Paths, Request};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let port: u16 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(18080);
    let app = match std::env::var_os("SV_FIXTURE") {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            let delay = std::env::var("SV_CM3_DELAY").ok().and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let url = common::mock_cm3(&dir, Duration::from_secs_f64(delay));
            let work = PathBuf::from(args.get(3).cloned().unwrap_or_else(|| std::env::temp_dir().join(format!("sv-serve-{port}")).display().to_string()));
            let _ = std::fs::create_dir_all(&work);
            common::app(&work, &dir, &url, |_| {})
        }
        None => {
            let repo = common::repo();
            let config = args.get(2).map(PathBuf::from).unwrap_or_else(|| repo.join("program/app/config.json"));
            let local = args.get(3).map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("sv-serve"));
            App::new(Paths { program: repo.join("program"), local_root: local, config }, "dev".into()).expect("設定を読めません")
        }
    };
    app.start();
    let server = tiny_http::Server::http(("127.0.0.1", port)).expect("ポートを開けません");
    println!("http://127.0.0.1:{port}/");
    for mut rq in server.incoming_requests() {
        let app = app.clone();
        std::thread::spawn(move || {
            let mut body = vec![];
            let _ = rq.as_reader().read_to_end(&mut body);
            let url = rq.url().to_string();
            let (path, query) = url.split_once('?').unwrap_or((&url, ""));
            let method = rq.method().as_str().to_string();
            let r = app.handle(&Request { method: &method, path, query, body: &body });
            let mut resp = tiny_http::Response::from_data(r.body).with_status_code(r.status);
            resp.add_header(tiny_http::Header::from_bytes("Content-Type", r.ctype.as_bytes()).unwrap());
            for (k, v) in r.headers {
                if let Ok(h) = tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()) {
                    resp.add_header(h);
                }
            }
            let _ = rq.respond(resp);
        });
    }
}
