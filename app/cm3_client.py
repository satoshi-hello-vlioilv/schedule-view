# -*- coding: utf-8 -*-
"""
CM3 scheduling system client.

POST /Cm3SchWeb/service/schapp/cmschview/init  ->  entity.meisaiList
Normalizes the list for the UI. Standard library only.

Row kinds
  lot     : normal production row
  inspect : production-like row whose lot/use name contains an inspection keyword
            (e.g. 点検板 - rolled, but for inspection)
  stop    : equipment stop row (time and text only)
"""
import html
import json
import re
import unicodedata
import urllib.error
import urllib.request
from datetime import datetime, timedelta, timezone

JST = timezone(timedelta(hours=9))


class Cm3Error(Exception):
    """Raised when CM3 cannot be reached or returns an error."""


# ---------------------------------------------------------------- request

def build_payload(setsubi, term_kbn, client_ip):
    """Request body observed in DevTools (Payload tab of the init request)."""
    return {
        "ip": client_ip,
        "userID": None,
        "termID": None,
        "setsubiName": setsubi,
        "useKbn": None,
        "termKbn": term_kbn,
        "authInfo": None,
        "result": 0,
        "error": False,
        "errorMessage": "",
        "version": None,
        "setsubi": "",
        "updateTs": None,
        "meisaiList": [],
        "urlSetsubi": setsubi,
        "urlTermKbn": term_kbn,
    }


def _decode_body(raw_bytes, content_type):
    """Decode response bytes. Uses charset header, then UTF-8, then CP932."""
    m = re.search(r"charset=([\w\-]+)", content_type or "", re.I)
    candidates = ([m.group(1)] if m else []) + ["utf-8", "cp932"]
    for enc in candidates:
        try:
            return raw_bytes.decode(enc)
        except (LookupError, UnicodeDecodeError):
            continue
    return raw_bytes.decode("utf-8", errors="replace")


def fetch_raw(config, setsubi, term_kbn):
    """Return the raw JSON (dict) from CM3."""
    base = config["cm3_base_url"].rstrip("/")
    body = json.dumps(
        build_payload(setsubi, term_kbn, config.get("client_ip")),
        ensure_ascii=False,
    ).encode("utf-8")

    req = urllib.request.Request(
        base + config["init_path"],
        data=body,
        method="POST",
        headers={
            "Accept": "application/json, text/plain, */*",
            "Content-Type": "application/json;charset=UTF-8",
            "Origin": base,
            "Referer": base + "/Cm3SchWeb/",
        },
    )

    if config.get("use_system_proxy"):
        opener = urllib.request.build_opener()
    else:
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    try:
        with opener.open(req, timeout=config.get("timeout_sec", 15)) as res:
            text = _decode_body(res.read(), res.headers.get("Content-Type"))
    except urllib.error.HTTPError as e:
        raise Cm3Error("CM3がHTTPエラーを返しました: %s %s" % (e.code, e.reason))
    except urllib.error.URLError as e:
        raise Cm3Error("CM3に接続できません: %s" % e.reason)
    except TimeoutError:
        raise Cm3Error("CM3の応答がタイムアウトしました")

    try:
        return json.loads(text)
    except ValueError:
        raise Cm3Error("CM3の応答がJSONではありません")


# ---------------------------------------------------------------- text repair

_JP = re.compile(r"[\u3000-\u30ff\u3400-\u9fff\uff01-\uff9f]")
_LATIN_HIGH = re.compile(r"[\u0080-\u00ff\u0152-\u0192\u02c6-\u02dc\u2013-\u2122]")
_TAG = re.compile(r"<[^>]+>")


def fix_text(value):
    """
    Repair text for display.
      - HTML entities  (&#12288; &amp; ...)  -> characters
      - HTML tags      (<br> <span> ...)     -> removed
      - mojibake: UTF-8 / CP932 bytes that were read as Latin-1 / CP1252
    Normal Japanese text is returned unchanged.
    """
    if not isinstance(value, str) or not value:
        return value
    s = value
    if "<" in s and ">" in s:
        s = _TAG.sub(" ", s)
    if "&" in s:
        s = html.unescape(s)
    if _LATIN_HIGH.search(s):
        for src, dst in (("latin-1", "utf-8"), ("cp1252", "utf-8"),
                         ("latin-1", "cp932"), ("cp1252", "cp932")):
            try:
                t = s.encode(src).decode(dst)
            except (UnicodeEncodeError, UnicodeDecodeError):
                continue
            if _JP.search(t):
                s = t
                break
    return s.replace("\u3000", " ").strip()


# ---------------------------------------------------------------- row kinds

# Data fields that a production (lot) row always has.
_LOT_DATA_KEYS = ["zaishitsu", "inItaAtsu", "outItaAtsu", "reienHaba",
                  "seihinItaAtsu", "seihinItaHaba", "keijo", "jiKotei"]
_DIMENSION_KEYS = ["inItaAtsu", "outItaAtsu", "reienHaba", "seihinItaAtsu", "seihinItaHaba"]
LOT_MIN_FIELDS = 4

# fields that are only for screen styling / links
_NON_TEXT_KEYS = {"rowStyle", "teisiStyle", "ltNoRendered", "lotToiawaseUrl", "yotoMei"}
_DURATION = re.compile(r"^\d+(\.\d+)?\s*(分|h|H|時間)?$")


def _to_float(v):
    try:
        return float(v)
    except (TypeError, ValueError):
        return None


def _filled(v):
    return v is not None and str(v).strip() != ""


def is_stop_row(m):
    """
    Classify by the amount of data in the row (not by style flags).
      production row : 4+ of the 8 data fields filled AND a thickness/width is numeric
      stop row       : otherwise (only a time and a text)
    """
    filled = sum(1 for k in _LOT_DATA_KEYS if _filled(m.get(k)))
    numeric = sum(1 for k in _DIMENSION_KEYS if _to_float(m.get(k)) is not None)
    return not (filled >= LOT_MIN_FIELDS and numeric >= 1)


def _nfkc(s):
    return unicodedata.normalize("NFKC", str(s or "")).upper()


def is_inspect_row(lot, yoto, keywords):
    """Production-like row whose lot number or use name contains an inspection keyword."""
    text = _nfkc(lot) + " " + _nfkc(yoto)
    return any(k and _nfkc(k) in text for k in (keywords or []))


def _stop_text(m):
    """
    Build the stop text in the SAME ORDER as the fields in the CM3 response.
    CM3 splits one stop name into fixed-width pieces from the left column
      roll="ワークロ" lotNo="ール交換計" zaishitsu="画"  ->  "ワークロール交換計画"
    Pieces are concatenated without spaces. A duration ("7分") goes last.
    """
    name, extra = "", []
    for k in m:
        if k in _NON_TEXT_KEYS or k == "kanryoYotei":
            continue
        v = m.get(k)
        if v is None or isinstance(v, (bool, dict, list)):
            continue
        v = fix_text(str(v))
        if not v:
            continue
        if _DURATION.match(v):
            extra.append(v)
        else:
            name += v
    return " ".join([name] + extra).strip() or "設備停止"


# ---------------------------------------------------------------- normalize

_DATE_TIME = re.compile(r"^\s*(\d{1,2})/(\d{1,2})\s+(\d{1,2}):(\d{2})\s*$")
_TIME_ONLY = re.compile(r"^\s*(\d{1,2}):(\d{2})\s*$")


def _ts_to_dt(update_ts):
    if update_ts is None:
        return None
    try:
        return datetime.fromtimestamp(int(update_ts) / 1000, tz=JST)
    except (TypeError, ValueError, OSError):
        return None


def _reduction(in_t, out_t):
    a, b = _to_float(in_t), _to_float(out_t)
    if not a or b is None:
        return None
    return round((a - b) / a * 100, 1)


def normalize(raw, own_setsubi, inspect_keywords=None):
    """Convert CM3 response into the structure used by the UI."""
    if not isinstance(raw, dict):
        raise Cm3Error("CM3応答の形式が不正です")
    if raw.get("status") != "SUCCESS":
        raise Cm3Error("CM3応答 status=%s" % raw.get("status"))

    entity = raw.get("entity") or {}
    if entity.get("error"):
        raise Cm3Error("CM3エラー: %s" % (fix_text(entity.get("errorMessage")) or "詳細不明"))

    update_dt = _ts_to_dt(entity.get("updateTs"))
    base_year = (update_dt or datetime.now(JST)).year
    cur_month = update_dt.month if update_dt else None
    cur_day = update_dt.day if update_dt else None
    prev_dt = None
    prev_end = None          # completion of the previous row = estimated start of this row

    lot_count = {}
    items = []

    for idx, m in enumerate(entity.get("meisaiList") or [], start=1):
        # kanryoYotei: "10/19 14:05" when the date is shown, "     14:27" otherwise
        raw_kanryo = fix_text(m.get("kanryoYotei") or "")
        kanryo_dt = None
        md = _DATE_TIME.match(raw_kanryo)
        mt = _TIME_ONLY.match(raw_kanryo)
        if md:
            cur_month, cur_day = int(md.group(1)), int(md.group(2))
            hh, mm = int(md.group(3)), int(md.group(4))
        elif mt:
            hh, mm = int(mt.group(1)), int(mt.group(2))
        else:
            hh = mm = None

        if hh is not None and cur_month is not None:
            year = base_year
            if update_dt and cur_month < update_dt.month - 6:
                year += 1
            try:
                kanryo_dt = datetime(year, cur_month, cur_day, hh, mm, tzinfo=JST)
                if prev_dt and kanryo_dt < prev_dt and not md:
                    kanryo_dt += timedelta(days=1)
                    cur_month, cur_day = kanryo_dt.month, kanryo_dt.day
                prev_dt = kanryo_dt
            except ValueError:
                kanryo_dt = None

        item = {
            "seq": idx,
            "setsubi": own_setsubi,
            "kanryoYoteiRaw": raw_kanryo,
            "kanryoYotei": kanryo_dt.strftime("%m/%d %H:%M") if kanryo_dt else raw_kanryo,
            "kanryoYoteiIso": kanryo_dt.isoformat() if kanryo_dt else None,
            "startYotei": prev_end.strftime("%m/%d %H:%M") if prev_end else "",
            "startYoteiIso": prev_end.isoformat() if prev_end else None,
            "teisiStyle": m.get("teisiStyle"),
            "rowStyle": m.get("rowStyle"),
            "passNo": None,
            "passTotal": None,
        }
        if kanryo_dt:
            prev_end = kanryo_dt

        if is_stop_row(m):
            item.update({
                "rowKind": "stop",
                "isStop": True,
                "stopText": _stop_text(m),
                "lotNo": "", "zaishitsu": "", "inItaAtsu": "", "outItaAtsu": "",
                "reductionRate": None, "reienHaba": "", "seihinItaAtsu": "",
                "seihinItaHaba": "", "keijo": "", "yotoMei": "", "jiKotei": "",
                "selfNext": False,
            })
        else:
            lot = fix_text(m.get("lotNo") or "")
            yoto = fix_text(m.get("yotoMeiFull") or m.get("yotoMei") or "")
            ji_kotei = fix_text(m.get("jiKotei") or "")
            kind = "inspect" if is_inspect_row(lot, yoto, inspect_keywords) else "lot"
            if kind == "lot":
                lot_count[lot] = lot_count.get(lot, 0) + 1
                item["passNo"] = lot_count[lot]
            item.update({
                "rowKind": kind,
                "isStop": False,
                "stopText": "",
                "lotNo": lot,
                "zaishitsu": fix_text(m.get("zaishitsu") or ""),
                "inItaAtsu": fix_text(m.get("inItaAtsu") or ""),
                "outItaAtsu": fix_text(m.get("outItaAtsu") or ""),
                "reductionRate": _reduction(m.get("inItaAtsu"), m.get("outItaAtsu")),
                "reienHaba": fix_text(m.get("reienHaba") or ""),
                "seihinItaAtsu": fix_text(m.get("seihinItaAtsu") or ""),
                "seihinItaHaba": fix_text(m.get("seihinItaHaba") or ""),
                "keijo": fix_text(m.get("keijo") or ""),
                "yotoMei": yoto,
                "jiKotei": ji_kotei,
                "selfNext": ji_kotei == own_setsubi,
            })
        items.append(item)

    for it in items:
        if it["rowKind"] == "lot":
            it["passTotal"] = lot_count.get(it["lotNo"], 1)

    return {
        "setsubi": entity.get("setsubiName") or own_setsubi,
        "termKbn": entity.get("termKbn"),
        "updateTs": entity.get("updateTs"),
        "updateTime": update_dt.strftime("%Y/%m/%d %H:%M:%S") if update_dt else None,
        "count": sum(1 for i in items if i["rowKind"] == "lot"),
        "inspectCount": sum(1 for i in items if i["rowKind"] == "inspect"),
        "stopCount": sum(1 for i in items if i["rowKind"] == "stop"),
        "items": items,
    }


def get_schedule(config, setsubi, term_kbn, inspect_keywords=None):
    return normalize(fetch_raw(config, setsubi, term_kbn), setsubi, inspect_keywords)
