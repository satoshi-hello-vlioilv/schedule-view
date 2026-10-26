# -*- coding: utf-8 -*-
"""作り物のデータ（tools/fixture.py）が何度作っても同じであること（旧版の答え desktop/tests/golden/python.json と突き合わせる前提）。"""
import hashlib
import importlib.util
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

_spec = importlib.util.spec_from_file_location("fixture", Path(__file__).resolve().parents[1] / "tools" / "fixture.py")
fixture = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(fixture)


class Fixture(unittest.TestCase):
    def test_same_every_time(self):
        digests = []
        for _ in range(2):
            with tempfile.TemporaryDirectory() as d:
                fixture.build(Path(d), 200, 20)
                h = hashlib.sha256()
                for eq in fixture.EQUIPMENTS:
                    h.update((Path(d) / "cm3" / f"{eq}.json").read_bytes())
                con = sqlite3.connect(Path(d) / "LOTNOW.sqlite3")
                for row in con.execute('SELECT * FROM 仕掛 ORDER BY rowid'):
                    h.update(json.dumps(row, ensure_ascii=False).encode("utf-8"))
                con.close()
                digests.append(h.hexdigest())
        self.assertEqual(digests[0], digests[1])

    def test_shapes_seen_in_the_real_data(self):
        with tempfile.TemporaryDirectory() as d:
            fixture.build(Path(d), 200, 20)
            rows = json.loads((Path(d) / "cm3" / "L-2.json").read_text(encoding="utf-8"))["entity"]["meisaiList"]
            self.assertTrue(any(r.get("roll") == "ワークロ" for r in rows), "設備停止行（停止名を項目の順に区切る）")
            self.assertTrue(any(r.get("lotNo") == "点検板" for r in rows), "点検行")
            self.assertTrue(any(r["kanryoYotei"].startswith("     ") for r in rows), "日付の無い時刻だけの行")


if __name__ == "__main__":
    unittest.main()
