# -*- coding: utf-8 -*-
"""配る ZIP を組み立てる（exe と、アプリの program/app）。置き場の形は desktop/src/release.rs・distribute.rs（LotScope と同じ）。

    python bundle.py brand <項目>                    名前の定義（program/app/brand.json）の値を出す（CI が名前を書き写さない）
    python bundle.py build --exe <exe> --out <出力先> 配る ZIP と release.json・.sha256 を作る
    python bundle.py version                         アプリの版（program/app/version.py）を出す（Releases の札 v<版> に使う）
    python bundle.py notes --release <release.json>  Releases の説明（その版の更新履歴・配り方・指紋）を出す

配る形（ZIP の中。先頭のフォルダはアプリ名）:
    <name>/<exe>          exe（Rust）。画面・CM3・台帳・配布と更新のすべて。上へたどって program/app/brand.json を探す
    <name>/program/app/   画面（web）・現場の設定（config.json）・設定の初期値（defaults）・名前（brand.json）・版（version.py）
                          （git ls-files の物だけ。手元の設定・.pyc を混ぜない。開発の道具・試験・文書は入れない）
    <name>/README.md

Python は要らない（利用者の PC で動くのは exe だけ）。
"""
from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import subprocess
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
PROGRAM = REPO / "program"
MAX_BYTES = 1 << 30          # 取り込む側（desktop/src/update.rs）が断る大きさ
SHIPPED = ("program/app",)   # 配る program の中身（git が知っている物だけ）
SKIP_SUFFIX = (".pyc",)
SKIP_NAMES = ("app_icon.py",)   # 作るときだけ使う（exe に絵として入る）


def brand() -> dict:
    return {k: v for k, v in json.loads((PROGRAM / "app" / "brand.json").read_text(encoding="utf-8")).items() if not k.startswith("_")}


def _version_py(program: Path = PROGRAM) -> dict:
    ns: dict = {}
    exec(compile((program / "app" / "version.py").read_text(encoding="utf-8"), "version.py", "exec"), ns)
    return ns


def app_version(program: Path = PROGRAM) -> str:
    return _version_py(program)["APP_VERSION"]


def changelog(program: Path = PROGRAM) -> list[dict]:
    """更新履歴（新しい版が先）。"""
    return _version_py(program)["CHANGELOG"]


def release_notes(rel: dict, history: list[dict]) -> str:
    """GitHub Releases の説明（Markdown）。その版の更新履歴・配り方（README の「配り方」と同じ言葉）・指紋。"""
    entry = next((e for e in history if e["version"] == rel["version"]), None)
    if entry is None:
        raise SystemExit(f"更新履歴（program/app/version.py の CHANGELOG）に版 {rel['version']} がありません")
    lines = [f"## 版 {rel['version']}（{entry['date']}）", "", "### 変わったこと", *[f"- {n}" for n in entry["notes"]], "",
             "### 配り方", "",
             f"1. 下の `{rel['zip']}` を取ります（`release.json`・`.sha256` も同じ版の物です）。",
             "2. アプリの「アプリ ▾」→「版の配布…」→「ZIP から版を置く…」で選ぶと、中身を確かめてから更新の置き場に版のフォルダを作ります。"
             "置いただけでは配りません。",
             "3. 試してから、一覧の「この版を配る」を押します。各 PC は起動したとき・30 分ごとに確かめ、開き直したときにこの版になります。", "",
             f"指紋（SHA-256）: `{rel['sha256']}`", ""]
    return "\n".join(lines)


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def program_files() -> list[str]:
    """配るアプリのファイル（git が知っている物だけ）。"""
    out = subprocess.run(["git", "ls-files", "-z", *SHIPPED], cwd=REPO, check=True, capture_output=True).stdout
    names = [p for p in out.decode("utf-8").split("\0") if p]
    return sorted(p for p in names if not p.endswith(SKIP_SUFFIX) and Path(p).name not in SKIP_NAMES)


def safe_name(name: str) -> bool:
    """release.json の zip・exe は 1 つの名前（区切り・..・ドライブ名を含まない）。"""
    return bool(name) and name not in (".", "..") and not any(c in name for c in "/\\:") and name == name.strip()


def release_json(b: dict, version: str, zip_name: str, sha: str) -> dict:
    """版のフォルダの目印（desktop/src/release.rs が読む形・schema 1）。"""
    if not (safe_name(zip_name) and safe_name(b["exe"])):
        raise SystemExit(f"名前に使えない文字があります: {zip_name} / {b['exe']}")
    return {"schema": 1, "app_id": b["app_id"], "name": b["name"], "version": version, "zip": zip_name, "sha256": sha, "exe": b["exe"]}


def zip_dir(root: Path, top: str, out: Path) -> int:
    """root の中身を top/… として ZIP にする（並びは名前順・実行の許可も残す）。→ ファイルの数"""
    files = sorted(p for p in root.rglob("*") if p.is_file())
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for p in files:
            z.write(p, f"{top}/{p.relative_to(root).as_posix()}")
    return len(files)


def build(exe: Path, out: Path) -> dict:
    b, version = brand(), app_version()
    stage = out / "stage" / b["name"]
    shutil.rmtree(out / "stage", ignore_errors=True)
    stage.mkdir(parents=True)
    shutil.copy2(exe, stage / b["exe"])
    for rel in program_files():
        dst = stage / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(REPO / rel, dst)
    shutil.copy2(REPO / "README.md", stage / "README.md")
    zip_name = f"{b['name']}-{version}-windows.zip"
    zpath = out / zip_name
    n = zip_dir(stage, b["name"], zpath)
    if zpath.stat().st_size > MAX_BYTES:
        raise SystemExit("配る ZIP が 1 GB を超えました（取り込む側が断ります）")
    sha = sha256_file(zpath)
    rel = release_json(b, version, zip_name, sha)
    (out / "release.json").write_text(json.dumps(rel, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    (out / f"{zip_name}.sha256").write_text(f"{sha}  {zip_name}\n", encoding="ascii")
    summary = {**rel, "path": str(zpath), "files": n, "bytes": zpath.stat().st_size, "stage": str(stage)}
    print(json.dumps(summary, ensure_ascii=False, indent=1))
    return summary


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("brand")
    s.add_argument("key")
    s = sub.add_parser("build")
    s.add_argument("--exe", required=True, type=Path)
    s.add_argument("--out", required=True, type=Path)
    sub.add_parser("version")
    s = sub.add_parser("notes")
    s.add_argument("--release", required=True, type=Path)
    a = ap.parse_args(argv)
    if a.cmd == "brand":
        print(brand()[a.key])
    elif a.cmd == "version":
        print(app_version())
    elif a.cmd == "notes":
        print(release_notes(json.loads(a.release.read_text(encoding="utf-8")), changelog()), end="")
    else:
        build(a.exe, a.out)


if __name__ == "__main__":
    main()
