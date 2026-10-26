# -*- coding: utf-8 -*-
"""自己診断・通しの確かめ用の設定（config.json）を作る。CM3 と台帳を作り物（fixture.py・mock_cm3.py）へ向ける。

    python program/tools/selftest_config.py <fixture のフォルダ> <作り物の CM3 の URL> <書き出す config.json> [更新の置き場]
"""
import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]


def make(data: Path, cm3_url: str, out: Path, source: str = "") -> dict:
    cfg = json.loads((REPO / "program" / "app" / "config.json").read_text(encoding="utf-8"))
    cfg["cm3_base_url"] = cm3_url
    cfg["master"]["source_path"] = str(data / "LOTNOW.sqlite3")
    cfg["update"] = {"source": source}
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(cfg, ensure_ascii=False, indent=2), encoding="utf-8")
    return cfg


if __name__ == "__main__":
    a = sys.argv[1:]
    if len(a) < 3:
        sys.exit(__doc__)
    make(Path(a[0]), a[1], Path(a[2]), a[3] if len(a) > 3 else "")
