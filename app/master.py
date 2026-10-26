# -*- coding: utf-8 -*-
"""
Lot master (LOTNOW.sqlite3) loader.

  1. Copy the shared file to a local folder (never open the shared file with SQLite,
     so the program that updates it is not blocked by database locks).
  2. Open only the local copy, read-only.
  3. Find the lot key column (ロット番号 / ﾛｯﾄ番号 / ロットNo ...) from the column names
     AND the actual data.
  4. Join master columns to schedule rows by one or more key pairs
     (schedule field = master column, all pairs must match).
"""
import json
import os
import pathlib
import re
import shutil
import sqlite3
import threading
import time
import unicodedata
from datetime import datetime

BASE_DIR = os.path.dirname(os.path.abspath(__file__))
SETTINGS_FILE = os.path.join(BASE_DIR, "master_settings.json")

# Column names treated as "lot number" (compared after normalization)
KEY_NAME_CANDIDATES = [
    "ロット番号", "ﾛｯﾄ番号", "ロットNo", "ﾛｯﾄNo", "ロットNO", "ロットＮｏ",
    "ロットNO.", "ロット", "LotNo", "LOT_NO", "LOTNO", "Lot Number",
]
LOT_PATTERN = re.compile(r"^[A-Z][0-9A-Z]{6}$")
_NUM = re.compile(r"^[+-]?\d+(\.\d+)?$")

DEFAULT_SETTINGS = {
    "table": "",
    "keys": [{"schedule": "lotNo", "master": ""}],
    "display_columns": [],
}


# ---------------------------------------------------------------- helpers

def norm_name(s):
    """Normalize a column name: half/full width, case, separators."""
    s = unicodedata.normalize("NFKC", str(s or "")).upper()
    return re.sub(r"[\s_\-\.・]", "", s)


def norm_key(v):
    """
    Normalize a key value for matching.
      - NFKC (full/half width), upper case, all spaces removed
      - numbers are compared as numbers: "0.300" == "0.3", "840.00" == "840"
    """
    if v is None:
        return ""
    s = re.sub(r"\s", "", unicodedata.normalize("NFKC", str(v)).upper())
    if _NUM.match(s):
        try:
            return ("%.6f" % float(s)).rstrip("0").rstrip(".")
        except ValueError:
            pass
    return s


def label(col):
    """Column label for display (half-width kana -> full-width)."""
    return unicodedata.normalize("NFKC", col or "")


_DATE0 = re.compile(r"^(\d{4})-(\d{2})-(\d{2}) 00:00:00$")
_DATETIME = re.compile(r"^(\d{4})-(\d{2})-(\d{2}) (\d{2}):(\d{2}):\d{2}$")


def display_value(v):
    if v is None:
        return ""
    s = str(v).strip()
    m = _DATE0.match(s)
    if m:
        return "%s/%s/%s" % m.groups()
    m = _DATETIME.match(s)
    if m:
        return "%s/%s/%s %s:%s" % m.groups()
    return s


KEY_NAMES = {norm_name(x) for x in KEY_NAME_CANDIDATES}


def _q(name):
    return '"%s"' % str(name).replace('"', '""')


def load_settings():
    try:
        with open(SETTINGS_FILE, "r", encoding="utf-8") as f:
            s = json.load(f)
    except (OSError, ValueError):
        s = {}
    out = dict(DEFAULT_SETTINGS)
    out.update({k: s[k] for k in ("table", "keys", "display_columns") if k in s})
    # old format: {"key_column": "..."}
    if "keys" not in s and s.get("key_column"):
        out["keys"] = [{"schedule": "lotNo", "master": s["key_column"]}]
    keys = [k for k in (out.get("keys") or []) if isinstance(k, dict) and k.get("schedule")]
    out["keys"] = keys or [dict(DEFAULT_SETTINGS["keys"][0])]
    return out


def save_settings(s):
    tmp = SETTINGS_FILE + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(s, f, ensure_ascii=False, indent=2)
    os.replace(tmp, SETTINGS_FILE)


# ---------------------------------------------------------------- master

class LotMaster:
    def __init__(self, cfg):
        self.cfg = cfg
        self.enabled = bool(cfg.get("enabled", True))
        self.source = cfg.get("source_path", "")
        local = cfg.get("local_copy", "data/LOTNOW.sqlite3")
        self.local = local if os.path.isabs(local) else os.path.join(BASE_DIR, local)
        self.info_file = self.local + ".copyinfo.json"
        self.interval = int(cfg.get("check_interval_sec", 60))
        self.lock = threading.RLock()
        self.last_check = 0.0
        self.settings = load_settings()

        self.tables = {}          # table -> [columns]
        self.table = ""
        self.keys = []            # resolved [(schedule_field, master_column)]
        self.key_warning = ""
        self.auto_table, self.auto_key = "", ""
        self.rows = {}            # tuple(norm keys) -> {col: value}
        self.row_count = 0
        self.dup_count = 0        # master rows dropped because the key was duplicated
        self.meta = {}            # contents of _更新情報 if exists
        self.status = {"state": "未読込", "message": "", "copiedAt": None,
                       "sourceMtime": None, "loadedAt": None}

    # ---------- copy

    def _read_copyinfo(self):
        try:
            with open(self.info_file, "r", encoding="utf-8") as f:
                return json.load(f)
        except (OSError, ValueError):
            return {}

    def _copy_if_changed(self, force):
        """Copy source -> local when size/mtime changed. Returns True if copied."""
        st = os.stat(self.source)
        sig = [st.st_size, int(st.st_mtime)]
        info = self._read_copyinfo()
        self.status["sourceMtime"] = datetime.fromtimestamp(st.st_mtime).strftime("%Y/%m/%d %H:%M:%S")
        if not force and info.get("sig") == sig and os.path.exists(self.local):
            self.status["copiedAt"] = info.get("copiedAt")
            return False

        # The writer is in the middle of a transaction -> do not copy now
        for suffix in ("-journal", "-wal"):
            if os.path.exists(self.source + suffix):
                raise RuntimeError("元ファイルが更新中のため、前回のコピーを使用しています")

        os.makedirs(os.path.dirname(self.local), exist_ok=True)
        tmp = self.local + ".tmp"
        # plain file copy: no SQLite lock is taken on the shared file
        shutil.copyfile(self.source, tmp)

        st2 = os.stat(self.source)
        if [st2.st_size, int(st2.st_mtime)] != sig:
            os.remove(tmp)
            raise RuntimeError("コピー中に元ファイルが更新されたため、次回再取得します")

        # validate the copy before replacing the previous one
        con = sqlite3.connect(pathlib.Path(tmp).as_uri() + "?mode=ro", uri=True)
        try:
            ok = con.execute("PRAGMA quick_check").fetchone()[0]
        finally:
            con.close()
        if ok != "ok":
            os.remove(tmp)
            raise RuntimeError("コピーしたファイルが壊れているため、前回のコピーを使用しています")

        os.replace(tmp, self.local)
        copied_at = time.strftime("%Y/%m/%d %H:%M:%S")
        with open(self.info_file, "w", encoding="utf-8") as f:
            json.dump({"sig": sig, "copiedAt": copied_at, "source": self.source}, f, ensure_ascii=False)
        self.status["copiedAt"] = copied_at
        return True

    # ---------- key column detection

    def _connect(self):
        return sqlite3.connect(pathlib.Path(self.local).as_uri() + "?mode=ro", uri=True)

    def column_stats(self, con, table, col, lots=None, sample=3000):
        q = "SELECT %s FROM %s LIMIT %d" % (_q(col), _q(table), sample)
        vals = [norm_key(r[0]) for r in con.execute(q)]
        vals = [v for v in vals if v]
        n = len(vals)
        pattern = sum(1 for v in vals if LOT_PATTERN.match(v))
        uniq = len(set(vals))
        name_match = norm_name(col) in KEY_NAMES
        is_cand = name_match or (n and pattern / n >= 0.5)
        hit = None
        if lots and is_cand:
            full = {norm_key(r[0]) for r in con.execute("SELECT %s FROM %s" % (_q(col), _q(table)))}
            hit = sum(1 for l in lots if l in full)
        return {
            "table": table,
            "column": col,
            "label": label(col),
            "nameMatch": name_match,
            "isCandidate": bool(is_cand),
            "patternRate": round(pattern / n * 100, 1) if n else 0.0,
            "uniqueRate": round(uniq / n * 100, 1) if n else 0.0,
            "scheduleHit": hit,
            "sample": vals[:3],
        }

    def _auto_detect(self, con):
        best = None
        for tb, cols in self.tables.items():
            for c in cols:
                st = self.column_stats(con, tb, c)
                if not st["isCandidate"]:
                    continue
                score = (st["nameMatch"], st["patternRate"], st["uniqueRate"])
                if best is None or score > best[0]:
                    best = (score, tb, c)
        return (best[1], best[2]) if best else ("", "")

    def candidates(self, lots=None):
        """Lot key candidates judged from the column name AND the data."""
        res = []
        with self.lock:
            if not os.path.exists(self.local):
                return res
            con = self._connect()
            try:
                for t, cols in self.tables.items():
                    for c in cols:
                        st = self.column_stats(con, t, c, lots)
                        if st["isCandidate"]:
                            res.append(st)
            finally:
                con.close()
        res.sort(key=lambda s: (s["scheduleHit"] or 0, s["nameMatch"], s["patternRate"], s["uniqueRate"]),
                 reverse=True)
        return res

    # ---------- resolve settings -> table + key pairs

    def _resolve(self, settings):
        """
        Returns (table, [(schedule_field, master_column)], warning).
        An empty master column means "auto" (the detected lot number column).
        """
        table = settings.get("table") or ""
        if table not in self.tables:
            table = self.auto_table
        cols = self.tables.get(table, [])
        pairs, bad = [], []
        for k in settings.get("keys") or []:
            s, m = k.get("schedule"), k.get("master") or ""
            if not m:
                m = self.auto_key if table == self.auto_table else ""
            if s and m in cols:
                pairs.append((s, m))
            else:
                bad.append(label(k.get("master")) or "(自動)")
        warn = ("台帳に無いキー列を除外しました: " + ", ".join(bad)) if bad else ""
        if not pairs and table == self.auto_table and self.auto_key:
            pairs = [("lotNo", self.auto_key)]
        return table, pairs, warn

    # ---------- load

    def _build_index(self, con, table, pairs):
        rows, dup = {}, 0
        if not (table and pairs):
            return rows, dup
        cur = con.execute("SELECT * FROM %s" % _q(table))
        cols = [d[0] for d in cur.description]
        idx = [cols.index(m) for _, m in pairs]
        for r in cur:
            k = tuple(norm_key(r[i]) for i in idx)
            if not all(k):
                continue
            if k in rows:
                dup += 1
                continue
            rows[k] = dict(zip(cols, r))
        return rows, dup

    def _load(self):
        con = self._connect()
        try:
            names = [r[0] for r in con.execute(
                "SELECT name FROM sqlite_master WHERE type IN ('table','view') "
                "AND name NOT LIKE 'sqlite_%' ORDER BY name")]
            self.tables = {t: [r[1] for r in con.execute("PRAGMA table_info(%s)" % _q(t))] for t in names}

            self.meta = {}
            for t in names:
                if t.startswith("_") and self.tables[t][:2] == ["項目", "値"]:
                    self.meta.update({str(k): str(v) for k, v in con.execute("SELECT * FROM %s" % _q(t))})

            self.auto_table, self.auto_key = self._auto_detect(con)
            self.table, self.keys, self.key_warning = self._resolve(self.settings)
            self.rows, self.dup_count = self._build_index(con, self.table, self.keys)
            self.row_count = len(self.rows)
        finally:
            con.close()
        self.status["loadedAt"] = time.strftime("%Y/%m/%d %H:%M:%S")

    # ---------- public

    def refresh(self, force=False):
        """Check the source at most every `interval` seconds, copy if changed, reload."""
        if not self.enabled:
            self.status.update(state="無効", message="config.json の master.enabled が false です")
            return
        with self.lock:
            now = time.time()
            if not force and self.rows and now - self.last_check < self.interval:
                return
            self.last_check = now
            copied = False
            try:
                copied = self._copy_if_changed(force)
                self.status.update(state="正常", message="")
            except FileNotFoundError:
                self.status.update(state="警告", message="元ファイルが見つかりません: %s" % self.source)
            except PermissionError:
                self.status.update(state="警告", message="元ファイルにアクセスできません（権限）")
            except OSError as e:
                self.status.update(state="警告", message="元ファイルにアクセスできません: %s" % e)
            except (RuntimeError, sqlite3.DatabaseError) as e:
                self.status.update(state="警告", message=str(e))

            if not os.path.exists(self.local):
                self.status.update(state="エラー")
                self.rows = {}
                return
            if copied or force or not self.rows:
                try:
                    self._load()
                except sqlite3.DatabaseError as e:
                    self.status.update(state="エラー", message="手元コピーを読めません: %s" % e)

    def apply_settings(self, new):
        with self.lock:
            keys = [{"schedule": str(k.get("schedule") or ""), "master": str(k.get("master") or "")}
                    for k in (new.get("keys") or []) if k.get("schedule")]
            self.settings = {
                "table": new.get("table", ""),
                "keys": keys or [dict(DEFAULT_SETTINGS["keys"][0])],
                "display_columns": list(new.get("display_columns", self.settings.get("display_columns", []))),
            }
            save_settings(self.settings)
            if os.path.exists(self.local):
                self._load()

    def test_keys(self, settings, key_tuples):
        """
        Try key settings without saving.
        key_tuples: list of dicts (schedule items) -> counts how many match.
        """
        with self.lock:
            table, pairs, warn = self._resolve(settings)
            if not os.path.exists(self.local) or not pairs:
                return {"table": table, "pairs": [], "warning": warn or "キーがありません",
                        "hit": 0, "total": 0, "masterKeys": 0, "duplicates": 0}
            con = self._connect()
            try:
                rows, dup = self._build_index(con, table, pairs)
            finally:
                con.close()
        keys = {tuple(norm_key(it.get(s)) for s, _ in pairs) for it in key_tuples}
        keys = {k for k in keys if all(k)}
        return {
            "table": table,
            "pairs": [{"schedule": s, "master": m, "masterLabel": label(m)} for s, m in pairs],
            "warning": warn,
            "hit": sum(1 for k in keys if k in rows),
            "total": len(keys),
            "masterKeys": len(rows),
            "duplicates": dup,
        }

    def columns(self):
        return list(self.tables.get(self.table, []))

    def find_column(self, name):
        """Column of the current table matching `name` (exact, then normalized)."""
        cols = self.columns()
        if name in cols:
            return name
        n = norm_name(name)
        for c in cols:
            if norm_name(c) == n:
                return c
        return ""

    def key_columns(self):
        return [m for _, m in self.keys]

    def key_label(self):
        return " + ".join(label(m) for _, m in self.keys)

    def row_for(self, item):
        """Full master row for a schedule item (or None)."""
        if not self.keys:
            return None
        k = tuple(norm_key(item.get(s)) for s, _ in self.keys)
        return self.rows.get(k) if all(k) else None

    def join(self, items, cols):
        """
        item['m']    = {master column: value} for the requested columns
        item['mHit'] = True / False (lot rows) / None (inspect, stop rows)
        """
        hit = 0
        with self.lock:
            for it in items:
                if it.get("rowKind") != "lot" or not self.keys:
                    it["m"], it["mHit"] = None, None
                    continue
                r = self.row_for(it)
                if r is None:
                    it["m"], it["mHit"] = {}, False
                else:
                    it["m"] = {c: display_value(r.get(c)) for c in cols}
                    it["mHit"] = True
                    hit += 1
        return hit

    def describe(self):
        return {
            "enabled": self.enabled,
            "source": self.source,
            "local": self.local,
            "table": self.table,
            "autoTable": self.auto_table,
            "autoKey": self.auto_key,
            "autoKeyLabel": label(self.auto_key),
            "keys": [{"schedule": s, "master": m, "masterLabel": label(m)} for s, m in self.keys],
            "keyLabel": self.key_label(),
            "keyWarning": self.key_warning,
            "settings": self.settings,
            "rowCount": self.row_count,
            "duplicates": self.dup_count,
            "meta": self.meta,
            "tables": {t: [{"name": c, "label": label(c)} for c in cols] for t, cols in self.tables.items()},
            "status": self.status,
        }
