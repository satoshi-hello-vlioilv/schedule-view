# -*- coding: utf-8 -*-
"""配る形と更新の通しを、本物の窓で確かめる（評価関数。LotScope の verify.py と同じ筋書き）。Windows の CI と、手元の Linux（xvfb-run の下）で流す。

    python verify.py update --exe <作った exe> [--work <作業フォルダ>] [--keep]

利用者の PC を真似る: 作業場所（SV_LOCAL_ROOT）はまっさら、配る形は共有を真似たフォルダ、更新の置き場は共有を真似たフォルダ。
CM3 と台帳は作り物（program/tools/fixture.py・mock_cm3.py）。窓は自己診断（SV_SELFTEST）で起こし、結果のファイル
（selftest.js が窓の情報・更新の係・ショートカットの係を添える）を読む。

  1. 共有の exe を開く → この PC の app へ写り、版ごとの写しの窓で開く。起動の最初に置き場の次の版（+1）を取り込んで入れ替える
     （置き場は config.json にあるので、初めての起動から確かめる。使っている間に裏で取り込む流れは Rust の単体試験 updater.rs）
  2. app の exe（ショートカットの行き先）を開く → +1 のまま開く。共有を指すショートカットを直す
  3. 閉じている間に次の版（+2）を置く → ショートカットから 1 回開くだけで +2 になる
  4. 閉じている間に開けない版（+3。画面のファイルが欠けた版）を置いて 1 回開く → 入れ替えるが開けず、控え（+2）へ戻して開き直す。印が付き、もう取り込まない
  5. 配る版を前の版（+1。distribute.json）に戻す → 1 回開くだけで配る版になり、配る版を作業場所に覚える（want.json）
  6. 新しい PC（まっさらな作業場所）が配る入口を開くだけで、配る版をこの PC へ入れて開き、入口の置き場を控える
  あわせて: 共有の配る形は 1 ファイルも変わらない。
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import zipfile
from pathlib import Path

import bundle

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
sys.path.insert(0, str(REPO / "program" / "tools"))
import fixture  # noqa: E402
import mock_cm3  # noqa: E402
import selftest_config  # noqa: E402

WAIT = 240


def bump(v: str, n: int) -> str:
    a, b, c = (int(x) for x in v.split("."))
    return f"{a}.{b}.{c + n}"


def tree_digest(root: Path) -> str:
    h = hashlib.sha256()
    for p in sorted(x for x in root.rglob("*") if x.is_file()):
        h.update(p.relative_to(root).as_posix().encode())
        h.update(p.read_bytes())
    return h.hexdigest()


def place(stage: Path, version: str, rel_root: Path, broken: bool = False) -> None:
    """配る形 stage の版を書き換えて ZIP にし、置き場の <アプリ>/<版>/ に release.json と並べて置く。"""
    b = bundle.brand()
    work = Path(tempfile.mkdtemp(prefix="sv-variant-"))
    try:
        top = work / b["name"]
        shutil.copytree(stage, top)
        vp = top / "program" / "app" / "version.py"
        old = bundle.app_version(top / "program")
        vp.write_text(vp.read_text(encoding="utf-8").replace(f'APP_VERSION = "{old}"', f'APP_VERSION = "{version}"', 1), encoding="utf-8")
        if broken:   # 開けない版（取り込みの確かめは通る）: 画面のファイルが欠けている
            (top / "program" / "app" / "web" / "index.html").unlink()
        dest = rel_root / b["name"] / version
        dest.mkdir(parents=True, exist_ok=True)
        zip_name = f"{b['name']}-{version}-windows.zip"
        bundle.zip_dir(top, b["name"], dest / zip_name)
        rel = bundle.release_json(b, version, zip_name, bundle.sha256_file(dest / zip_name))
        (dest / "release.json").write_text(json.dumps(rel, ensure_ascii=False, indent=1), encoding="utf-8")
    finally:
        shutil.rmtree(work, ignore_errors=True)


class Run:
    def __init__(self, work: Path, env: dict):
        self.work, self.env, self.n = work, env, 0

    def open(self, exe: Path, label: str) -> dict:
        """exe を開き、自己診断の結果を待つ（入口・写し・開き直しの窓をたどった先の窓が書く）。"""
        self.n += 1
        out = self.work / f"selftest-{self.n}-{label}.json"
        env = {**self.env, "SV_SELFTEST": str(out)}
        subprocess.Popen([str(exe)], cwd=str(exe.parent), env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        end = time.time() + WAIT
        while time.time() < end and not out.exists():
            time.sleep(1)
        time.sleep(3)    # 窓が終わるのを待つ
        if not out.exists():
            return {"ok": False, "error": f"{WAIT} 秒たっても結果がありません（{label}）"}
        r = json.loads(out.read_text(encoding="utf-8"))
        for x in r.get("results", []):
            if not x["ok"]:
                print(f"  [{label}] FAIL {x['name']} -- {x['info']}")
        return r


def check(cond: bool, what: str, info="") -> bool:
    print(("PASS " if cond else "FAIL ") + what + (f" -- {info}" if info else ""))
    return bool(cond)


def make_link(link: Path, target: Path, workdir: Path) -> None:
    """人が作ったショートカットを真似る（Windows は WScript.Shell、ほかは窓の lnk.rs と同じ小さな JSON）。"""
    link.parent.mkdir(parents=True, exist_ok=True)
    if os.name == "nt":
        ps = (f"$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{link}');$s.TargetPath='{target}';"
              f"$s.WorkingDirectory='{workdir}';$s.Save()")
        subprocess.run(["powershell", "-NoProfile", "-Command", ps], check=True)
    else:
        link.write_text(json.dumps({"target": str(target), "workdir": str(workdir), "description": ""}), encoding="utf-8")


def read_link(link: Path):
    """→ (指す先, 作業フォルダ)。無ければ None。"""
    if not link.exists():
        return None
    if os.name == "nt":
        ps = f"$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{link}');$s.TargetPath;$s.WorkingDirectory"
        out = subprocess.run(["powershell", "-NoProfile", "-Command", ps], capture_output=True, text=True).stdout.splitlines()
        return (Path(out[0]), Path(out[1])) if len(out) >= 2 else None
    v = json.loads(link.read_text(encoding="utf-8"))
    return Path(v["target"]), Path(v["workdir"])


def same(a, b) -> bool:
    return os.path.normcase(os.path.realpath(str(a))) == os.path.normcase(os.path.realpath(str(b)))


def update(exe: Path, work: Path) -> bool:
    b = bundle.brand()
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True)
    built = bundle.build(exe, work / "out")
    v0 = built["version"]
    v1, v2, v3 = bump(v0, 1), bump(v0, 2), bump(v0, 3)
    share = work / "share"
    with zipfile.ZipFile(built["path"]) as z:
        z.extractall(share)
    share_app = share / b["name"]
    share_exe = share_app / b["exe"]
    if os.name != "nt":
        share_exe.chmod(0o755)
    share_digest = tree_digest(share_app)
    rel_root = work / "Releases"
    place(Path(built["stage"]), v0, rel_root)     # 置き場には配った版も並ぶ（配る版を前の版に戻すときに選ぶ）
    place(Path(built["stage"]), v1, rel_root)
    data = work / "data"
    fixture.build(data, 500)
    srv = mock_cm3.serve(data, 0)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    conf = work / "config.json"
    selftest_config.make(data, f"http://127.0.0.1:{srv.server_address[1]}", conf, str(rel_root))
    local = work / "local"
    links = [work / "links" / "Desktop", work / "links" / "Programs"]
    env = {k: v for k, v in os.environ.items() if k not in ("SV_NO_HANDOFF", "SV_PROGRAM_DIR", "SV_SELFTEST")}
    env.update({"SV_CONFIG": str(conf), "SV_LOCAL_ROOT": str(local), "SV_LINK_DIRS": os.pathsep.join(map(str, links))})
    run = Run(work, env)
    app = local / "app"
    ok = True

    # 1. 共有の exe を開く
    r = run.open(share_exe, "share")
    info, upd = r.get("info") or {}, r.get("update") or {}
    ok &= check(r.get("ok") is True, "1. 共有の exe で開くと自己診断が通る", r.get("error", ""))
    ok &= check(same(info.get("program", ""), app / "program"), "1. この PC の app の program で動く", info.get("program"))
    ok &= check(str(local / "desktop") in str(info.get("exe", "")), "1. 窓は版ごとの写しで動く（app を掴まない）", info.get("exe"))
    ok &= check(info.get("installed") is True and info.get("version") == v1, f"1. 起動の最初に置き場の {v1} を取り込み、その版で開く",
                json.dumps(info, ensure_ascii=False)[:300])
    ok &= check(bundle.app_version(local / "update" / "old" / "program") == v0, f"1. 写した版 {v0} は控えに")
    ok &= check(upd.get("state") == "latest", "1. 開いたあとは置き場の最新を使っている", json.dumps(upd, ensure_ascii=False)[:300])
    sc = (r.get("install") or {}).get("shortcut") or {}
    ok &= check(sc.get("ask") is True and not sc.get("links"), "1. ショートカットが無いので画面が尋ねる", sc)
    lnk = links[0] / f"{b['title']}.lnk"
    make_link(lnk, share_exe, share_app)   # 人が作った、共有の exe を指し作業フォルダも共有のショートカット（次の起動で直る）

    # 2. ショートカットの行き先（app の exe）で開く
    r = run.open(app / b["exe"], "app")
    info, upd = r.get("info") or {}, r.get("update") or {}
    ok &= check(r.get("ok") is True, "2. app の exe で開くと自己診断が通る", r.get("error", ""))
    ok &= check(info.get("version") == v1 and same(info.get("program", ""), app / "program"), f"2. この PC の app の {v1} で開く", info.get("version"))
    ok &= check(upd.get("state") == "latest", "2. 置き場の最新を使っている", upd.get("state"))
    time.sleep(1)
    got = read_link(lnk)
    ok &= check(bool(got) and same(got[0], app / b["exe"]) and same(got[1], app),
                "2. 共有を指していたショートカットを、この PC の app（作業フォルダも app）に直した", got)
    ok &= check((local / "update" / "source.json").is_file(), "2. 更新の置き場を控えた（次の起動の最初に確かめる）")

    # 3. 閉じている間に次の版を置く → ショートカットから 1 回開くだけで入れ替わる
    place(Path(built["stage"]), v2, rel_root)
    r = run.open(app / b["exe"], "closed-new")
    info, upd = r.get("info") or {}, r.get("update") or {}
    ok &= check(r.get("ok") is True and info.get("version") == v2, f"3. 閉じている間に置いた {v2} に、1 回の起動で入れ替わった", info.get("version"))
    ok &= check(upd.get("state") == "latest", "3. 開いたあとは置き場の最新を使っている", upd.get("state"))

    # 4. 閉じている間に開けない版を置いて 1 回開く → 入れ替えるが開けず、控えへ戻して開き直す
    place(Path(built["stage"]), v3, rel_root, broken=True)
    r = run.open(app / b["exe"], "revert")
    info, upd = r.get("info") or {}, r.get("update") or {}
    ok &= check(r.get("ok") is True and info.get("version") == v2, f"4. {v3} は開けず、{v2} に戻して開き直した", info.get("version"))
    failed_file = local / "update" / "FAILED.json"
    failed = json.loads(failed_file.read_text(encoding="utf-8")).get("versions", []) if failed_file.exists() else []
    ok &= check(failed == [v3], f"4. {v3} に開けなかった印", failed)
    ok &= check(upd.get("state") == "latest" and v3 in (upd.get("failed") or []), "4. 開けなかった版はもう取り込まない", json.dumps(upd, ensure_ascii=False)[:300])

    # 5. 配る版を前の版（v1）に戻す → 1 回開くだけで配る版になり、配る版を作業場所に覚える
    (rel_root / b["name"] / "distribute.json").write_text(json.dumps(
        {"schema": 1, "app_id": b["app_id"], "version": v1, "entry": b["exe"], "setAt": time.strftime("%Y-%m-%dT%H:%M:%S"),
         "setBy": "verify", "setPc": "verify", "previous": v2}, ensure_ascii=False), encoding="utf-8")
    r = run.open(app / b["exe"], "distribute-down")
    info, upd = r.get("info") or {}, r.get("update") or {}
    want = json.loads((local / "want.json").read_text(encoding="utf-8")) if (local / "want.json").exists() else {}
    ok &= check(r.get("ok") is True and info.get("version") == v1, f"5. 配る版を {v1} に戻すと、1 回の起動でその版になる", info.get("version"))
    ok &= check(want.get("version") == v1, "5. 配る版を作業場所に覚えた", want)
    ok &= check(upd.get("state") == "latest", "5. 開いたあとは配る版を使っている", json.dumps(upd, ensure_ascii=False)[:300])

    # 6. 新しい PC: 配る入口（置き場のアプリのフォルダの exe）を開くだけで、配る版を取り込んでこの PC で開く
    entry = rel_root / b["name"] / b["exe"]
    shutil.copy2(share_exe, entry)
    local2 = work / "local-new-pc"
    run2 = Run(work, {**env, "SV_LOCAL_ROOT": str(local2), "SV_LINK_DIRS": os.pathsep.join(str(work / "links2" / n) for n in ("Desktop", "Programs"))})
    r = run2.open(entry, "entry")
    info = r.get("info") or {}
    ok &= check(r.get("ok") is True and info.get("version") == v1 and info.get("installed") is True,
                f"6. 配る入口を開くと、配る版 {v1} をこの PC の app に入れて開く", json.dumps(info, ensure_ascii=False)[:300])
    ok &= check(same(info.get("program", ""), local2 / "app" / "program"), "6. 動くのはこの PC の app", info.get("program"))
    entry_file = local2 / "entry.json"
    remembered = json.loads(entry_file.read_text(encoding="utf-8")).get("dir") if entry_file.exists() else None
    ok &= check(bool(remembered) and same(remembered, rel_root / b["name"]), "6. 入口の置き場を控えた（設定の置き場が見えない PC でも更新が届く）", remembered)

    ok &= check(tree_digest(share_app) == share_digest, "共有の配る形は 1 ファイルも変わらない")
    srv.shutdown()
    return ok


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("update")
    s.add_argument("--exe", required=True, type=Path)
    s.add_argument("--work", type=Path, default=Path(tempfile.gettempdir()) / "sv-verify")
    s.add_argument("--keep", action="store_true")
    a = ap.parse_args(argv)
    ok = update(a.exe.resolve(), a.work.resolve())
    if not a.keep and ok:
        shutil.rmtree(a.work, ignore_errors=True)
    print("OK" if ok else "FAILED")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
