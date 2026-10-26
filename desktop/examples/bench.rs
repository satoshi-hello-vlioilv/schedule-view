//! 速さを測る（旧版と同じ作り物のデータ・同じ手順。結果は program/docs/DESIGN.md に残す）。
//!
//!   cargo run --release --example bench -- <行数=6881> <CM3 の遅れ(秒)=1.0>
//!
//! 測る物: 台帳の初めの読み込み（写し＋読み込み）・最初の一覧・HOT コイル作業順（3 設備）・2 回目からの一覧・台帳の読み直し・
//! 台帳設定の画面（候補の判定）・列の設定の実データ・読み込み中に一覧が待たないか・メモリ。

#[path = "../tests/common/mod.rs"]
mod common;

use std::time::{Duration, Instant};
use sv_core::app::Request;

fn rss_mb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmHWM:")).and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok()))
        .map(|kb| kb / 1024)
        .unwrap_or(0)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rows: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(6881);
    let delay: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let data = common::tmp(&format!("bench-data-{rows}"));
    common::fixture(&data, rows);
    let work = common::tmp(&format!("bench-work-{rows}"));
    let url = common::mock_cm3(&data, Duration::from_secs_f64(delay));
    let app = common::app(&work, &data, &url, |_| {});
    let get = |path: &str, query: &str| app.handle(&Request { method: "GET", path, query, body: &[] });
    let ms = |t: Instant| (t.elapsed().as_secs_f64() * 1000.0).round();

    let t = Instant::now();
    app.master.refresh(false);
    let master_first = ms(t);
    let t = Instant::now();
    assert_eq!(get("/api/cm3/schedule", "setsubi=L-2").status, 200);
    let first = ms(t);
    let t = Instant::now();
    assert_eq!(get("/api/hot", "force=1").status, 200);
    let hot = ms(t);
    let mut warm = vec![];
    for _ in 0..5 {
        let t = Instant::now();
        get("/api/cm3/schedule", "setsubi=L-2");
        warm.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    warm.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let t = Instant::now();
    app.master.refresh(true);
    let reload = ms(t);
    let t = Instant::now();
    get("/api/master", "");
    let minfo = ms(t);
    let t = Instant::now();
    get("/api/view/sample", "setsubi=L-2");
    let sample = ms(t);
    // 読み込み中も一覧は待たない: 裏で読み直しながら一覧を引く
    let m = app.master.clone();
    let bg = std::thread::spawn(move || m.refresh(true));
    std::thread::sleep(Duration::from_millis(20));
    let t = Instant::now();
    get("/api/cm3/schedule", "setsubi=L-2");
    let during = ms(t);
    bg.join().unwrap();
    println!(
        "{{\"rows\": {rows}, \"delay_s\": {delay}, \"master_first_load_ms\": {master_first}, \"schedule_first_ms\": {first}, \"hot_force_ms\": {hot}, \
\"schedule_warm_median_ms\": {:.1}, \"master_reload_ms\": {reload}, \"api_master_ms\": {minfo}, \"view_sample_ms\": {sample}, \
\"schedule_during_reload_ms\": {during}, \"maxrss_mb\": {}}}",
        warm[2],
        rss_mb()
    );
    let _ = std::fs::remove_dir_all(&data);
    let _ = std::fs::remove_dir_all(&work);
}
