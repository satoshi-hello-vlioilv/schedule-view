# -*- coding: utf-8 -*-
"""版の管理（app/version.py・app_build.release_digest）。版を上げずに配る物を変えることを止める。
失敗したら、表示された手順で版を上げて記録し直す。"""
import json
import re
import unittest
from datetime import date
from pathlib import Path

import app_build

V = app_build._version_module()
APP_VERSION, CHANGELOG, RELEASE, version_key = V["APP_VERSION"], V["CHANGELOG"], V["RELEASE"], V["version_key"]
REPO = Path(app_build.REPO)


class Changelog(unittest.TestCase):
    def test_head_is_the_app_version(self):
        self.assertRegex(APP_VERSION, r"^\d+\.\d+\.\d+$")
        self.assertEqual(CHANGELOG[0]["version"], APP_VERSION, "CHANGELOG の先頭にいまの版の1件を足してください")

    def test_newest_first_and_filled(self):
        keys = [version_key(e["version"]) for e in CHANGELOG]
        self.assertEqual(keys, sorted(keys, reverse=True), "新しい版を上に")
        self.assertEqual(len(keys), len(set(keys)), "同じ版が2つある")
        dates = [date.fromisoformat(e["date"]) for e in CHANGELOG]
        self.assertEqual(dates, sorted(dates, reverse=True))
        for e in CHANGELOG:
            self.assertTrue(e["notes"] and all(n.strip() for n in e["notes"]), e["version"])

    def test_versions_compare_as_numbers(self):
        self.assertGreater(version_key("1.10.0"), version_key("1.9.3"))
        self.assertEqual(version_key("1.0"), (1, 0, 0))


class Release(unittest.TestCase):
    def test_program_matches_the_recorded_release(self):
        how = ("APP_VERSION を上げて CHANGELOG の先頭に1件足し（desktop/Cargo.toml と tauri.conf.json の version も）、"
               "`cd program && python app_build.py --release` で記録してください（まだ配っていない版を直しただけなら --amend）。")
        self.assertEqual(RELEASE["version"], APP_VERSION, f"版 {APP_VERSION} がまだ記録されていません。{how}")
        self.assertEqual(RELEASE["digest"], app_build.release_digest(), f"版 {APP_VERSION} を記録したあとに配る物が変わっています。{how}")

    def test_digest_covers_the_exe_and_the_screen(self):
        files = app_build.release_files()
        self.assertNotIn("program/app/version.py", files, "RELEASE を書くと要約が変わってしまう")
        for f in ("program/app/web/app.js", "program/app/config.json", "desktop/src/main.rs", "desktop/Cargo.lock"):
            self.assertIn(f, files)
        self.assertFalse(any(k.startswith(("program/tests/", "program/tools/", "program/docs/")) or k.endswith(".md") for k in files),
                         "説明・試験・道具は含めない")


class Desktop(unittest.TestCase):
    def test_same_version(self):
        """exe（desktop/）の版は、アプリの版と同じ（exe が名乗る版と配る物の版がずれない）。"""
        cargo = re.search(r'^version\s*=\s*"([^"]+)"', (REPO / "desktop" / "Cargo.toml").read_text(encoding="utf-8"), re.M).group(1)
        conf = json.loads((REPO / "desktop" / "tauri.conf.json").read_text(encoding="utf-8"))["version"]
        self.assertEqual((cargo, conf), (APP_VERSION, APP_VERSION), "desktop/Cargo.toml と tauri.conf.json の version をアプリの版に合わせてください")


if __name__ == "__main__":
    unittest.main()
