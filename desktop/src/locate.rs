//! 置き場所を探す: アプリの中身（program フォルダ）・現場の設定（config.json）・この PC の作業場所。

use crate::brand::brand;
use std::env;
use std::path::{Path, PathBuf};

/// program フォルダを渡す引数（`--program <フォルダ>`）。
pub const PROGRAM_ARG: &str = "--program";

/// program フォルダの目印（配る形の確かめと同じ: update::validate_dist）
fn is_program(p: &Path) -> bool {
    p.join("app").join("brand.json").is_file()
}

/// program フォルダ。探す順: 引数 `--program` → 環境変数 SV_PROGRAM_DIR（開発・試験）→ この exe から上へたどって
/// `program/app/brand.json` がある所（配る形は exe と program が並ぶ・作る途中は 3 つ上）。
pub fn program_dir() -> Result<PathBuf, String> {
    let given = arg_value(PROGRAM_ARG).or_else(|| env::var_os("SV_PROGRAM_DIR").map(PathBuf::from));
    match given {
        Some(p) if is_program(&p) => Ok(p),
        Some(p) => Err(format!("指定された program フォルダに app\\brand.json がありません: {}", p.display())),
        None => {
            let exe = env::current_exe().map_err(|e| e.to_string())?;
            find_program_from(&exe).ok_or_else(|| {
                format!(
                    "アプリの中身（program フォルダ）が見つかりません。配られた ZIP を展開したフォルダの {} から開いてください（exe と program フォルダが並んでいる形）。\n探し始めた場所: {}",
                    brand().exe,
                    exe.parent().unwrap_or(&exe).display()
                )
            })
        }
    }
}

/// 引数 `name <値>` の値（`--name=値` も受ける）。
pub fn arg_value(name: &str) -> Option<PathBuf> {
    let args: Vec<String> = env::args().collect();
    let eq = format!("{name}=");
    args.iter()
        .enumerate()
        .find_map(|(i, a)| if a == name { args.get(i + 1).map(PathBuf::from) } else { a.strip_prefix(&eq).map(PathBuf::from) })
}

pub fn find_program_from(start: &Path) -> Option<PathBuf> {
    start.ancestors().skip(1).take(6).map(|d| d.join("program")).find(|p| is_program(p))
}

/// 現場の設定（SV_CONFIG → program/app/config.json）。
pub fn config_file(program: &Path) -> PathBuf {
    env::var_os("SV_CONFIG").map(PathBuf::from).unwrap_or_else(|| program.join("app").join("config.json"))
}

/// この PC の作業場所（SV_LOCAL_ROOT → (LOCALAPPDATA → TEMP → ホーム)\<data_dir>）。
pub fn local_root() -> PathBuf {
    local_root_from(|k| env::var_os(k).filter(|v| !v.is_empty()), home())
}

fn local_root_from(var: impl Fn(&str) -> Option<std::ffi::OsString>, home: PathBuf) -> PathBuf {
    if let Some(p) = var("SV_LOCAL_ROOT") {
        return PathBuf::from(p);
    }
    let base = var("LOCALAPPDATA").or_else(|| var("TEMP")).map(PathBuf::from).unwrap_or(home);
    base.join(&brand().data_dir)
}

fn home() -> PathBuf {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    env::var_os(key).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_work_place_follows_the_environment() {
        let none = |_: &str| None;
        assert_eq!(local_root_from(none, PathBuf::from("/h")), PathBuf::from("/h").join(&brand().data_dir));
        let la = |k: &str| (k == "LOCALAPPDATA").then(|| "C:/Users/a/AppData/Local".into());
        assert_eq!(local_root_from(la, PathBuf::from("/h")), PathBuf::from("C:/Users/a/AppData/Local").join(&brand().data_dir));
        let given = |k: &str| (k == "SV_LOCAL_ROOT").then(|| "/work".into());
        assert_eq!(local_root_from(given, PathBuf::from("/h")), PathBuf::from("/work"));
    }

    #[test]
    fn the_program_folder_is_found_above_the_exe() {
        let d = std::env::temp_dir().join(format!("sv-locate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("program").join("app")).unwrap();
        std::fs::write(d.join("program").join("app").join("brand.json"), "{}").unwrap();
        std::fs::create_dir_all(d.join("desktop").join("target").join("debug")).unwrap();
        assert_eq!(find_program_from(&d.join("App.exe")), Some(d.join("program")), "配る形: exe と program が並ぶ");
        assert_eq!(find_program_from(&d.join("desktop").join("target").join("debug").join("App.exe")), Some(d.join("program")), "作る途中");
        assert_eq!(find_program_from(&std::env::temp_dir().join("無い").join("App.exe")), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
