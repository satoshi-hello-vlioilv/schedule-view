# -*- coding: utf-8 -*-
"""配る ZIP の組み立て（bundle.py）: 中身・名前・目印（release.json）・指紋。"""
import json
import tempfile
import unittest
import zipfile
from pathlib import Path

import bundle


class Bundle(unittest.TestCase):
    def test_build_makes_the_shape_the_exe_reads(self):
        b = bundle.brand()
        with tempfile.TemporaryDirectory() as d:
            exe = Path(d) / "fake.exe"
            exe.write_bytes(b"MZ-fake")
            s = bundle.build(exe, Path(d) / "out")
            with zipfile.ZipFile(s["path"]) as z:
                names = z.namelist()
            top = b["name"] + "/"
            self.assertIn(top + b["exe"], names)
            for f in ("program/app/brand.json", "program/app/version.py", "program/app/config.json", "program/app/web/index.html", "README.md"):
                self.assertIn(top + f, names)
            self.assertFalse([n for n in names if "/tests/" in n or "/tools/" in n or n.endswith((".pyc", "app_icon.py"))], "道具・試験・.pyc は入れない")
            rel = json.loads((Path(d) / "out" / "release.json").read_text(encoding="utf-8"))
            self.assertEqual((rel["app_id"], rel["version"], rel["exe"], rel["schema"]), (b["app_id"], bundle.app_version(), b["exe"], 1))
            self.assertEqual(rel["sha256"], bundle.sha256_file(Path(s["path"])))
            sha = (Path(d) / "out" / (rel["zip"] + ".sha256")).read_text(encoding="ascii").split()[0]
            self.assertEqual(sha, rel["sha256"])

    def test_names_are_single_names(self):
        self.assertTrue(bundle.safe_name("A-1.0.0-windows.zip"))
        for bad in ("", "..", "a/b", "a\\b", "C:x", " a"):
            self.assertFalse(bundle.safe_name(bad), bad)

    def test_release_notes_come_from_the_changelog(self):
        rel = {"version": bundle.app_version(), "zip": "x.zip", "sha256": "a" * 64}
        notes = bundle.release_notes(rel, bundle.changelog())
        self.assertIn("### 変わったこと", notes)
        self.assertIn("a" * 64, notes)
        with self.assertRaises(SystemExit):
            bundle.release_notes({**rel, "version": "0.0.1"}, bundle.changelog())


if __name__ == "__main__":
    unittest.main()
