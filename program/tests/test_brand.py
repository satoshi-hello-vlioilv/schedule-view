# -*- coding: utf-8 -*-
"""名前の定義（app/brand.json）が 1 か所で、作る道具（Cargo.toml・tauri.conf.json）と食い違わないこと。"""
import json
import re
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BRAND = json.loads((REPO / "program" / "app" / "brand.json").read_text(encoding="utf-8"))


class Brand(unittest.TestCase):
    def test_names_agree_with_the_build_tools(self):
        conf = json.loads((REPO / "desktop" / "tauri.conf.json").read_text(encoding="utf-8"))
        cargo = (REPO / "desktop" / "Cargo.toml").read_text(encoding="utf-8")
        self.assertEqual(conf["productName"], BRAND["name"])
        self.assertEqual(conf["mainBinaryName"] + ".exe", BRAND["exe"])
        self.assertEqual(conf["identifier"], BRAND["app_id"], "app_id は変えない目印")
        self.assertRegex(cargo, r'\[\[bin\]\]\nname = "%s"' % re.escape(BRAND["exe"][:-4]))

    def test_safe_names(self):
        for k in ("name", "exe", "data_dir"):
            self.assertTrue(BRAND[k] and not re.search(r'[\\/:*?"<>|]', BRAND[k]), k)
        self.assertTrue(BRAND["exe"].endswith(".exe"))


class SingleEntry(unittest.TestCase):
    def test_the_only_entry_is_the_exe(self):
        """入口は exe だけ（Flask・start.bat の名残が無い）。"""
        top = [p.name for p in REPO.iterdir()]
        self.assertFalse([n for n in top if n.endswith((".bat", ".vbs", ".cmd"))], top)
        self.assertFalse((REPO / "app").exists(), "旧版の app フォルダ（Flask）は外した")
        for p in (REPO / "program").rglob("*.py"):
            text = p.read_text(encoding="utf-8")
            self.assertIsNone(re.search(r"^\s*(from|import)\s+flask\b", text, re.M | re.I), p)


if __name__ == "__main__":
    unittest.main()
