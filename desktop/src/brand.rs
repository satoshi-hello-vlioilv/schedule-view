//! アプリの名前（`program/app/brand.json` の 1 か所の定義を、作るときに取り込む）。
//!
//! 名前（`name`・`title`・`subtitle`・`exe`）は窓の題名・起動画面・ショートカット・配る ZIP に使う。
//! `app_id`・`data_dir` は変えない目印（Tauri の identifier・更新の置き場で「このアプリの版」を見分ける・この PC の作業場所のフォルダ名）。

use serde::Deserialize;
use std::sync::OnceLock;

const SOURCE: &str = include_str!("../../program/app/brand.json");

#[derive(Debug, Deserialize)]
pub struct Brand {
    pub name: String,
    pub title: String,
    pub subtitle: String,
    pub exe: String,
    pub app_id: String,
    pub data_dir: String,
}

pub fn brand() -> &'static Brand {
    static B: OnceLock<Brand> = OnceLock::new();
    B.get_or_init(|| serde_json::from_str(SOURCE).expect("program/app/brand.json が読めません"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agrees_with_the_build_tools() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let b = brand();
        assert_eq!(conf["identifier"], b.app_id.as_str(), "Tauri の identifier は変えない目印（WebView の保存先が変わらない）");
        assert_eq!(conf["productName"], b.name.as_str());
        assert_eq!(format!("{}.exe", conf["mainBinaryName"].as_str().unwrap()), b.exe, "exe の名前");
        let cargo = include_str!("../Cargo.toml");
        assert!(cargo.contains(&format!("[[bin]]\nname = \"{}\"", b.exe.trim_end_matches(".exe"))), "Cargo.toml の [[bin]] name");
        assert!(!b.data_dir.is_empty() && !b.title.is_empty() && !b.subtitle.is_empty());
    }
}
