//! 設定・記録のファイル（JSON）を読む・書く。書くときは一時ファイル → 名前の付け替え（途中で止まっても前か後のどちらか）。

use serde_json::Value;
use std::path::Path;
use std::time::Duration;

/// JSON を読む（無い・壊れていれば None。先頭の BOM は外す）。
pub fn read_json(p: &Path) -> Option<Value> {
    let b = std::fs::read(p).ok()?;
    serde_json::from_slice(b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&b)).ok()
}

/// JSON を書く（字下げ 2・日本語はそのまま）。
pub fn write_json(p: &Path, v: &Value) -> Result<(), String> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{} を作れません: {e}", d.display()))?;
    }
    let mut tmp = p.as_os_str().to_os_string();
    tmp.push(format!(".tmp-{}", std::process::id()));
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, serde_json::to_vec_pretty(v).unwrap_or_default()).map_err(|e| format!("{} を書けません: {e}", tmp.display()))?;
    replace(&tmp, p).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{} を書けません: {e}", p.display())
    })
}

/// 名前を付け替えて置き換える。「使用中」（Windows の 5・32・33。ウイルス対策の検査など）は 3 秒まで待ってやり直す。
pub fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut waited = Duration::ZERO;
    loop {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if matches!(e.raw_os_error(), Some(5 | 32 | 33)) && waited < Duration::from_secs(3) => {
                std::thread::sleep(Duration::from_millis(150));
                waited += Duration::from_millis(150);
            }
            Err(e) => return Err(e),
        }
    }
}
