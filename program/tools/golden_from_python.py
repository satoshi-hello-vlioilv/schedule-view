# -*- coding: utf-8 -*-
"""移行の評価関数: 旧版（Python・Flask）の答えを「正しい答え」として記録する。Rust の試験（desktop/tests/parity.rs）が突き合わせる。

    python program/tools/golden_from_python.py [出力先=desktop/tests/golden]

旧版のコードは git のコミット（OLD_COMMIT の app/）から一時フォルダへ取り出して動かす（このリポジトリにはもう置かない）。
Flask が要る（pip install flask）。データは fixture.py の作り物、CM3 は mock_cm3.py の作り物。
毎回変わる値（取得・コピー・読込の時刻、手元のパス、Excel の作成時刻）は VOLATILE の印に置き換えて残す。
"""
from __future__ import annotations

import importlib
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
OLD_COMMIT = "9ad8a4c"
VOLATILE = "<VOLATILE>"
VOLATILE_KEYS = {"fetchedAt", "copiedAt", "loadedAt", "sourceMtime", "local", "source"}

sys.path.insert(0, str(HERE))
import fixture  # noqa: E402
import mock_cm3  # noqa: E402

# 文字の直し・正規化の試す値（Rust の単体と同じ物を両方で使う）
TEXT_CASES = [
    "", "  ", "L4061S0", "&#12288;点検板&#12288;", "缶材<br>(蓋)", "a &amp; b", "&lt;x&gt;", "è¨ˆç”»",
    "å°åˆ·ç‰ˆ", "Ｚ３Ｒ２", "ｶｰｴｱｺﾝ", "café", "100%", "©2026", "  前後  ", "1.0", "&#35373;備", "<span>停止</span>",
    "ﾜｰｸﾛｰﾙ", "　全角空白　", "naïve", "Ã©", "&nbsp;x", "&#x41;B", "&unknown;", "&amp",
]
KEY_CASES = [
    "", " ", "L4061S0", "l4061s0", " L4061 S0 ", "Ｌ４０６１Ｓ０", "0.300", "0.3", "840.00", "840", "-1.50", "+2",
    "1e3", "007", "1.", ".5", "１２３", "ﾛｯﾄ", "1,000", "12345678901234567890", "0.0000001", "3.14159265",
    "\t1\n", "NaN", "inf",
]
NAME_CASES = ["ﾛｯﾄ番号", "ロット番号", "ロット No.", "LOT_NO", "lot-no", "ｵｰﾀﾞｰ・板厚", "設計_PIT炉番炉順", "ロットＮｏ", "A B"]
DISPLAY_CASES = [None, "", "2026-10-30 00:00:00", "2026-10-18 22:07:42", "2026-10-18", " x ", 12, 1.5, 3.0, "2026-1-1 00:00:00"]

XLSX_BODY = {
    "sheet": "L-3:[試]/?*",
    "fileKey": "L-3",
    "headers": ["No", "完了予定", "ロットNo", "圧下率", "台帳 & 備考"],
    "widths": [6, 12.5, "x", 200, 1],
    "aligns": ["right", "left", "center", "right", "bogus"],
    "info": "CM3 L-3　CM3更新 2026/10/19 13:38:03　取得 A&B",
    "rows": [
        {"cells": [{"n": 1}, {"v": "10/19 14:05"}, {"v": "L4061S0"}, {"n": 45.5, "fmt": "0.0"}, {"v": "<x>\"y\"\u0001"}],
         "bg": "#ffffff", "color": "", "bold": False, "styles": {"3": {"bg": "#fdecec", "color": "#8a1c13", "bold": True}}},
        {"cells": [{"n": 2.0}, {"v": ""}, {}, {"n": 1234567, "fmt": "#,##0"}, {"v": "x", "align": "center"}],
         "bg": "#eef8ee", "bold": True},
        {"cells": [{"n": 3}, {"v": "10/19 15:00"}, {"v": "【設備停止】ワークロール交換計画 7分"}, {}, {}],
         "bg": "fdecec", "color": "#8a1c13", "merge": [2, 4]},
        {"cells": [{"n": True}, {"n": 0.1, "fmt": "0.00%"}, {"v": None}, {"n": -3.25, "fmt": "0.000"}, {"v": 12}],
         "merge": [3, 9]},
    ],
}


def checkout_old(dest: Path) -> Path:
    """旧版の app/ を dest へ取り出す。"""
    app = dest / "app"
    app.mkdir(parents=True)
    names = subprocess.run(["git", "ls-tree", "-r", "--name-only", OLD_COMMIT, "app"], cwd=REPO, check=True,
                           capture_output=True, text=True).stdout.split()
    for n in names:
        out = dest / n
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_bytes(subprocess.run(["git", "show", f"{OLD_COMMIT}:{n}"], cwd=REPO, check=True, capture_output=True).stdout)
    return app


def scrub(v):
    """毎回変わる値を印に置き換える。"""
    if isinstance(v, dict):
        return {k: (VOLATILE if k in VOLATILE_KEYS and v[k] not in (None, "") else scrub(x)) for k, x in v.items()}
    if isinstance(v, list):
        return [scrub(x) for x in v]
    return v


def xlsx_parts(data: bytes) -> dict:
    with zipfile.ZipFile(io.BytesIO(data)) as z:
        parts = {n: z.read(n).decode("utf-8") for n in z.namelist()}
    core = parts["docProps/core.xml"]
    i, j = core.find("<dcterms:created"), core.find("</dcterms:created>")
    parts["docProps/core.xml"] = core[:i] + VOLATILE + core[j + len("</dcterms:created>"):]
    return parts


def main(out: Path) -> None:
    tmp = Path(tempfile.mkdtemp(prefix="sv-golden-"))
    data = tmp / "data"
    fixture.build(data)
    srv = mock_cm3.serve(data, 0)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    port = srv.server_address[1]
    app_dir = checkout_old(tmp / "old")
    cfg = json.loads((app_dir / "config.json").read_text(encoding="utf-8"))
    cfg.update({"cm3_base_url": f"http://127.0.0.1:{port}", "open_browser": False})
    cfg["master"]["source_path"] = str(data / "LOTNOW.sqlite3")
    (app_dir / "config.json").write_text(json.dumps(cfg, ensure_ascii=False, indent=2), encoding="utf-8")
    shutil.copy(REPO / "program" / "tools" / "fixture.py", tmp / "fixture_used.py")

    os.chdir(app_dir)
    sys.path.insert(0, str(app_dir))
    cm3 = importlib.import_module("cm3_client")
    master = importlib.import_module("master")
    app_mod = importlib.import_module("app")
    client = app_mod.app.test_client()

    g: dict = {"oldCommit": OLD_COMMIT, "config": cfg | {"cm3_base_url": "<MOCK>"}}
    g["text"] = [[s, cm3.fix_text(s)] for s in TEXT_CASES]
    g["normKey"] = [[s, master.norm_key(s)] for s in KEY_CASES]
    g["normName"] = [[s, master.norm_name(s), master.label(s)] for s in NAME_CASES]
    g["display"] = [[s, master.display_value(s)] for s in DISPLAY_CASES]
    g["normalize"] = {}
    for eq in fixture.EQUIPMENTS:
        raw = json.loads((data / "cm3" / f"{eq}.json").read_text(encoding="utf-8"))
        g["normalize"][eq] = cm3.normalize(raw, eq, ["点検板"])
    g["normalizeErrors"] = []
    for bad in [None, {"status": "ERROR"}, {"status": "SUCCESS", "entity": {"error": True, "errorMessage": "&#35373;備エラー"}},
                {"status": "SUCCESS", "entity": {"error": True}}]:
        try:
            cm3.normalize(bad, "L-1")
            g["normalizeErrors"].append([bad, None])
        except cm3.Cm3Error as e:
            g["normalizeErrors"].append([bad, str(e)])

    def call(method, url, body=None):
        r = client.open(url, method=method, json=body) if body is not None else client.open(url, method=method)
        ctype = r.headers.get("Content-Type", "")
        if "json" in ctype:
            payload = scrub(r.get_json())
        elif "spreadsheetml" in ctype:
            payload = xlsx_parts(r.data)
        else:
            payload = r.data.decode("utf-8-sig")
        return {"method": method, "url": url, "body": body, "status": r.status_code, "type": ctype.split(";")[0],
                "disposition": r.headers.get("Content-Disposition", ""), "payload": payload}

    calls = [
        ("GET", "/api/view/settings", None),
        ("GET", "/api/view/default", None),
        ("GET", "/api/ui/settings", None),
        ("GET", "/api/cm3/schedule?setsubi=L-1", None),
        ("GET", "/api/cm3/schedule?setsubi=L-2", None),
        ("GET", "/api/cm3/schedule?setsubi=L-3", None),
        ("GET", "/api/cm3/schedule?setsubi=X-9", None),
        ("GET", "/api/hot", None),
        ("GET", "/api/cm3/schedule.csv?setsubi=L-2", None),
        ("GET", "/api/view/sample?setsubi=L-2", None),
        ("GET", "/api/master", None),
        ("POST", "/api/master/test", {"table": "", "keys": [{"schedule": "lotNo", "master": ""}]}),
        ("POST", "/api/master/test", {"table": "仕掛", "keys": [{"schedule": "lotNo", "master": "ﾛｯﾄ番号"},
                                                                 {"schedule": "zaishitsu", "master": "製造材質"}]}),
        ("POST", "/api/master/test", {"keys": []}),
        ("POST", "/api/master/test", {"keys": [{"schedule": "seq", "master": ""}]}),
        ("POST", "/api/master/test", {"table": "無い", "keys": [{"schedule": "lotNo", "master": ""}]}),
        ("POST", "/api/master/test", {"keys": [{"schedule": "lotNo", "master": ""}, {"schedule": "keijo", "master": ""}]}),
        ("POST", "/api/master/test", {"keys": [{"schedule": "lotNo", "master": "ﾛｯﾄ番号"}, {"schedule": "lotNo", "master": "ﾛｯﾄ番号"}]}),
        ("POST", "/api/view/settings", {"columns": []}),
        ("POST", "/api/view/settings", {"columns": [{"id": "a", "source": "x"}]}),
        ("POST", "/api/view/settings", {"columns": [{"id": "a", "source": "calc"}, {"id": "a", "source": "calc"}]}),
        ("POST", "/api/view/settings", {"columns": [{"id": "a", "source": "calc"}], "maps": []}),
        ("POST", "/api/export/xlsx", XLSX_BODY),
        ("POST", "/api/export/xlsx", {"headers": []}),
        ("POST", "/api/ui/settings", {"inspect_keywords": [" 点検板 ", "", "試験材"], "colors": {"accent": "#ABCDEF", "bogus": "#000000",
                                                                                                  "stop_bg": "red"}}),
        ("GET", "/api/cm3/schedule?setsubi=L-1&force=1", None),
        ("POST", "/api/master/settings", {"table": "仕掛", "keys": [{"schedule": "lotNo", "master": "ﾛｯﾄ番号"},
                                                                     {"schedule": "zaishitsu", "master": "製造材質"}]}),
        ("GET", "/api/cm3/schedule?setsubi=L-3", None),
        ("POST", "/api/master/settings", {"table": "", "keys": [{"schedule": "lotNo", "master": ""}]}),
        ("POST", "/api/master/reload", None),
    ]
    g["calls"] = [call(m, u, b) for m, u, b in calls]

    # 旧い形の列の設定（版 1）を読み込んだときの付け足し
    old_view = {"columns": [{"id": "s:seq", "source": "schedule", "field": "seq"},
                            {"id": "s:passNo", "source": "schedule", "field": "passNo"},
                            {"id": "m:設計_PIT炉番炉順", "source": "master", "field": "設計_PIT炉番炉順", "visible": False},
                            {"id": "s:lotNo", "source": "schedule", "field": "lotNo"}]}
    g["migrate"] = [old_view, app_mod.migrate_view(json.loads(json.dumps(old_view)))[0]]
    g["boot"] = {"equipments": cfg["equipments"], "scheduleColumns": [{"key": k, "label": v} for k, v in app_mod.SCHEDULE_COLUMNS],
                 "scheduleFields": [{"key": k, "label": v} for k, v in app_mod.SCHEDULE_FIELDS], "uiDefault": app_mod.DEFAULT_UI}
    srv.shutdown()
    out.mkdir(parents=True, exist_ok=True)
    (out / "python.json").write_text(json.dumps(g, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"{out / 'python.json'}: 呼び出し {len(g['calls'])} 件")
    shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    main(Path(sys.argv[1]) if len(sys.argv) > 1 else REPO / "desktop" / "tests" / "golden")
