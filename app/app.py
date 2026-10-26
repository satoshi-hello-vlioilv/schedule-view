# -*- coding: utf-8 -*-
"""CM3 Schedule Viewer - Flask application.  Start with ..\\start.bat"""
import copy
import csv
import io
import json
import os
import re
import sys
import threading
import time
import webbrowser

from flask import Flask, Response, jsonify, render_template, request

import cm3_client
import master as master_mod
import xlsx_writer

BASE_DIR = os.path.dirname(os.path.abspath(__file__))
UI_FILE = os.path.join(BASE_DIR, "ui_settings.json")
VIEW_FILE = os.path.join(BASE_DIR, "view_settings.json")

with open(os.path.join(BASE_DIR, "config.json"), "r", encoding="utf-8") as f:
    CONFIG = json.load(f)

EQUIPMENTS = {e["code"]: e for e in CONFIG["equipments"]}
MASTER = master_mod.LotMaster(CONFIG.get("master", {}))

# HOT coil work order (custom list)
HOT = {
    "title": "HOTコイル作業順",
    "equipments": ["L-1", "L-2", "L-3"],
    "prev_value": "HOT",
    "result_course_column": "実績_設備ｺｰｽ",
    "remain_course_column": "残仕掛設備ｺｰｽ",
    "kama_column": "設計_PIT炉番炉順",
}
HOT.update(CONFIG.get("hot") or {})
HOT["equipments"] = [e for e in HOT["equipments"] if e in EQUIPMENTS]

# Schedule fields (key, label). Used for columns, join keys and formula references.
SCHEDULE_COLUMNS = [
    ("seq", "No"),
    ("kanryoYotei", "完了予定"),
    ("lotNo", "ロットNo"),
    ("passNo", "パス"),
    ("zaishitsu", "材質"),
    ("inItaAtsu", "入側板厚"),
    ("outItaAtsu", "出側板厚"),
    ("reductionRate", "圧下率"),
    ("reienHaba", "冷延幅"),
    ("seihinItaAtsu", "製品板厚"),
    ("seihinItaHaba", "製品幅"),
    ("keijo", "形状"),
    ("yotoMei", "用途名"),
    ("jiKotei", "次工程"),
    ("setsubi", "設備"),
    ("startYotei", "開始予定(推定)"),
]
HIDDEN_BY_DEFAULT = {"setsubi", "startYotei"}
# Schedule fields that can be used as join keys
SCHEDULE_FIELDS = [(k, v) for k, v in SCHEDULE_COLUMNS
                   if k not in ("seq", "kanryoYotei", "passNo", "reductionRate", "setsubi", "startYotei")]

# ---------------------------------------------------------------- color settings

DEFAULT_UI = {
    "inspect_keywords": ["点検板"],
    "colors": {
        "normal_bg": "#ffffff",
        "stripe_bg": "#eef8ee",
        "self_next_text": "#8a5300",
        "inspect_bg": "#fff4c2",
        "inspect_text": "#6b4e00",
        "inspect_tag": "#c99400",
        "stop_bg": "#fdecec",
        "stop_text": "#8a1c13",
        "stop_tag": "#b42318",
        "date_line": "#9aa5b1",
        "accent": "#0f6cbd",
    },
}
_HEX = re.compile(r"^#[0-9a-fA-F]{6}$")


def _read_json(path):
    try:
        with open(path, "r", encoding="utf-8") as f:
            return json.load(f)
    except (OSError, ValueError):
        return None


def _write_json(path, obj):
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(obj, f, ensure_ascii=False, indent=2)
    os.replace(tmp, path)


def load_ui():
    ui = copy.deepcopy(DEFAULT_UI)
    saved = _read_json(UI_FILE) or {}
    if isinstance(saved.get("inspect_keywords"), list):
        ui["inspect_keywords"] = [str(k).strip() for k in saved["inspect_keywords"] if str(k).strip()]
    for k, v in (saved.get("colors") or {}).items():
        if k in ui["colors"] and isinstance(v, str) and _HEX.match(v):
            ui["colors"][k] = v.lower()
    return ui


UI = load_ui()

# ---------------------------------------------------------------- view (column) settings

VIEW_VERSION = 2
DEFAULT_MASTER_COLUMNS = ["製造調質", "ｵｰﾀﾞｰ板厚", "ｵｰﾀﾞｰ板幅", "BOX実績_良品重量",
                          "残仕掛設備ｺｰｽ", "特急区分", "保留_区分", "工場納期", "約束納期"]


def _col(cid, source, field, **kw):
    c = {"id": cid, "source": source, "field": field, "label": "", "visible": True,
         "width": None, "align": "auto", "bold": False,
         "format": {"type": "none"}, "map": ""}
    c.update(kw)
    return c


def prev_process_col():
    """前工程 = last item of 実績_設備ｺｰｽ (space separated equipment history)."""
    return _col("c:prev", "calc", "", label="前工程", align="center",
                expr='split([%s]," ",-1)' % HOT["result_course_column"])


def kama_col():
    """設計_PIT炉番炉順 shown as 釜番."""
    return _col("m:" + HOT["kama_column"], "master", HOT["kama_column"], label="釜番", align="center")


def default_view():
    cols = []
    for k, _ in SCHEDULE_COLUMNS:
        kw = {}
        if k == "reductionRate":
            kw["format"] = {"type": "number", "decimals": 1, "suffix": "%"}
        if k in HIDDEN_BY_DEFAULT:
            kw["visible"] = False
        cols.append(_col("s:" + k, "schedule", k, **kw))
        if k == "passNo":
            cols.append(prev_process_col())
            cols.append(kama_col())
    old = (master_mod.load_settings().get("display_columns") or DEFAULT_MASTER_COLUMNS)
    for m in old:
        if m != HOT["kama_column"]:
            cols.append(_col("m:" + m, "master", m))
    # example of a formula column (hidden)
    cols.append(_col("c:1", "calc", "", label="次の設備", visible=False,
                     expr='if(split([残仕掛設備ｺｰｽ]," ",1)="","完了",split([残仕掛設備ｺｰｽ]," ",1))'))
    return {"version": VIEW_VERSION, "columns": cols, "maps": {}, "rules": []}


def migrate_view(v):
    """Settings saved by older versions: add the new default columns (前工程, 釜番) once."""
    if int(v.get("version") or 1) >= VIEW_VERSION:
        return v, False
    cols = v["columns"]
    pos = next((i + 1 for i, c in enumerate(cols) if c.get("id") == "s:passNo"), len(cols))
    has_prev = any(c.get("id") == "c:prev" or
                   (c.get("source") == "calc" and (c.get("label") or "").strip() == "前工程") for c in cols)
    kid = "m:" + HOT["kama_column"]
    kama = next((c for c in cols if c.get("id") == kid), None)
    add = []
    if not has_prev:
        add.append(prev_process_col())
    if kama is None:
        add.append(kama_col())
    else:
        if not (kama.get("label") or "").strip():
            kama["label"] = "釜番"
        kama["visible"] = True
    cols[pos:pos] = add
    v["version"] = VIEW_VERSION
    return v, True


def load_view():
    v = _read_json(VIEW_FILE)
    if not isinstance(v, dict) or not isinstance(v.get("columns"), list):
        return default_view()
    v.setdefault("maps", {})
    v.setdefault("rules", [])
    v, changed = migrate_view(v)
    if changed:
        try:
            _write_json(VIEW_FILE, v)
        except OSError:
            pass
    return v


def valid_view(v):
    if not isinstance(v, dict):
        return "形式が不正です"
    if not isinstance(v.get("columns"), list) or not v["columns"]:
        return "列がありません"
    ids = set()
    for c in v["columns"]:
        if not isinstance(c, dict) or c.get("source") not in ("schedule", "master", "calc") or not c.get("id"):
            return "列の形式が不正です"
        if c["id"] in ids:
            return "列IDが重複しています: %s" % c["id"]
        ids.add(c["id"])
    if not isinstance(v.get("maps", {}), dict) or not isinstance(v.get("rules", []), list):
        return "変換表またはルールの形式が不正です"
    if len(json.dumps(v, ensure_ascii=False)) > 2_000_000:
        return "設定が大きすぎます"
    return ""


VIEW = load_view()

_REF = re.compile(r"\[([^\]]+)\]")


def needed_master_columns(extra=()):
    """Master columns the screen needs: visible master columns + those referenced in formulas/rules."""
    cols = MASTER.columns()
    if not cols:
        return []
    by_norm = {master_mod.norm_name(c): c for c in cols}
    need = set(c for c in extra if c in cols)
    texts = []
    for c in VIEW["columns"]:
        if c["source"] == "master" and c.get("field") in cols:
            if c.get("visible", True):
                need.add(c["field"])
        if c["source"] == "calc":
            texts.append(c.get("expr") or "")
    for r in VIEW.get("rules", []):
        texts.append(r.get("expr") or "")
        for k in ("col", "cellCol"):
            cid = r.get(k) or ""
            if cid.startswith("m:") and cid[2:] in cols:
                need.add(cid[2:])
    for t in texts:
        for name in _REF.findall(t):
            hit = name if name in cols else by_norm.get(master_mod.norm_name(name))
            if hit:
                need.add(hit)
    return [c for c in cols if c in need]


# ---------------------------------------------------------------- app

app = Flask(__name__)
app.json.ensure_ascii = False

_cache = {}
_cache_lock = threading.Lock()


def get_schedule_cached(setsubi, force=False):
    term_kbn = EQUIPMENTS[setsubi].get("term_kbn", "4")
    ttl = CONFIG.get("cache_sec", 10)
    now = time.time()
    with _cache_lock:
        hit = _cache.get(setsubi)
        if hit and not force and now - hit[0] < ttl:
            return hit[1]
    data = cm3_client.get_schedule(CONFIG, setsubi, term_kbn, UI["inspect_keywords"])
    data["fetchedAt"] = time.strftime("%Y/%m/%d %H:%M:%S")
    with _cache_lock:
        _cache[setsubi] = (now, data)
    return data


def _master_summary():
    created = MASTER.meta.get("作成日時", "")
    return {
        "state": MASTER.status["state"],
        "message": " / ".join(x for x in (MASTER.status["message"], MASTER.key_warning) if x),
        "keyLabel": MASTER.key_label(),
        "copiedAt": MASTER.status["copiedAt"],
        "created": created.replace("T", " ").replace("-", "/"),
        "rowCount": MASTER.row_count,
    }


def schedule_with_master(setsubi, force=False):
    """Schedule (cached) + needed master columns joined by the key pairs."""
    data = dict(get_schedule_cached(setsubi, force))
    data["items"] = [dict(i) for i in data["items"]]
    MASTER.refresh()
    data["masterHit"] = MASTER.join(data["items"], needed_master_columns())
    data["masterAll"] = MASTER.columns()
    data["masterKeyCols"] = MASTER.key_columns()
    data["master"] = _master_summary()
    return data


def _tokens(v):
    return str(v or "").split()


def hot_list(force=False):
    """
    HOT coil work order across L-1 / L-2 / L-3.
      - lot rows whose 前工程 (last item of 実績_設備ｺｰｽ) is HOT
      - and whose next equipment (first item of 残仕掛設備ｺｰｽ) is this equipment
        (the same lot can be planned on several lines; only the line right after HOT counts)
      - first pass only (2nd and later passes come after this line, not after HOT)
      - sorted by 完了予定
    """
    MASTER.refresh()
    res_col = MASTER.find_column(HOT["result_course_column"])
    rem_col = MASTER.find_column(HOT["remain_course_column"])
    kama = MASTER.find_column(HOT["kama_column"])
    need = needed_master_columns([c for c in (res_col, rem_col, kama) if c])
    prev_value = master_mod.norm_key(HOT["prev_value"])

    items, errors, per = [], [], {}
    counts = {"lots": 0, "noMaster": 0, "otherNext": 0, "laterPass": 0}
    fetched, updated = [], {}
    for eq in HOT["equipments"]:
        per[eq] = 0
        try:
            data = get_schedule_cached(eq, force)
        except cm3_client.Cm3Error as e:
            errors.append("%s: %s" % (eq, e))
            continue
        fetched.append(data["fetchedAt"])
        updated[eq] = data.get("updateTime")
        its = [dict(i) for i in data["items"]]
        MASTER.join(its, need)
        for it in its:
            if it["rowKind"] != "lot":
                continue
            counts["lots"] += 1
            if not it["mHit"] or not res_col:
                counts["noMaster"] += 1
                continue
            m = it["m"]
            hist = _tokens(m.get(res_col))
            prev = hist[-1] if hist else ""
            if master_mod.norm_key(prev) != prev_value:
                continue
            if rem_col:
                rem = _tokens(m.get(rem_col))
                if rem and master_mod.norm_key(rem[0]) != master_mod.norm_key(eq):
                    counts["otherNext"] += 1
                    continue
            if (it.get("passNo") or 1) != 1:
                counts["laterPass"] += 1
                continue
            it["prevProcess"] = prev
            it["kama"] = m.get(kama, "") if kama else ""
            items.append(it)
            per[eq] += 1

    order = {e: i for i, e in enumerate(HOT["equipments"])}
    items.sort(key=lambda it: (it.get("kanryoYoteiIso") or "9999", order.get(it["setsubi"], 99), it["seq"]))
    for i, it in enumerate(items, start=1):
        it["hotSeq"] = i
        it["rowStyle"] = "cmschview-data-palegreen" if i % 2 == 0 else "cmschview-data-white"

    return {
        "setsubi": "__HOT__",
        "items": items,
        "count": len(items),
        "inspectCount": 0,
        "stopCount": 0,
        "fetchedAt": max(fetched) if fetched else time.strftime("%Y/%m/%d %H:%M:%S"),
        "updateTime": " / ".join("%s %s" % (k, (v or "-")[5:16]) for k, v in updated.items()),
        "masterAll": MASTER.columns(),
        "masterKeyCols": MASTER.key_columns(),
        "master": _master_summary(),
        "hot": {
            "title": HOT["title"],
            "equipments": HOT["equipments"],
            "prevValue": HOT["prev_value"],
            "resultColumn": res_col,
            "remainColumn": rem_col,
            "kamaColumn": kama,
            "kamaConfigured": HOT["kama_column"],
            "perEquipment": per,
            "counts": counts,
            "errors": errors,
        },
    }


def _setsubi():
    setsubi = request.args.get("setsubi") or CONFIG["default_setsubi"]
    if setsubi not in EQUIPMENTS:
        raise ValueError("未登録の設備です: %s" % setsubi)
    return setsubi


# ---------------------------------------------------------------- pages / schedule

@app.route("/")
def index():
    return render_template(
        "index.html",
        equipments=CONFIG["equipments"],
        default_setsubi=CONFIG["default_setsubi"],
        refresh_sec=CONFIG.get("auto_refresh_sec", 60),
        lot_url_template=CONFIG.get("lot_url_template", ""),
        cm3_view_url_template=CONFIG.get("cm3_view_url_template", ""),
        ui=UI,
        ui_default=DEFAULT_UI,
        view=VIEW,
        hot={"title": HOT["title"], "equipments": HOT["equipments"]},
        schedule_columns=[{"key": k, "label": v} for k, v in SCHEDULE_COLUMNS],
        schedule_fields=[{"key": k, "label": v} for k, v in SCHEDULE_FIELDS],
    )


@app.route("/favicon.ico")
def favicon():
    return Response(status=204)


@app.route("/api/cm3/schedule")
def api_schedule():
    try:
        data = schedule_with_master(_setsubi(), request.args.get("force") == "1")
        return jsonify({"success": True, **data})
    except ValueError as e:
        return jsonify({"success": False, "message": str(e)}), 400
    except cm3_client.Cm3Error as e:
        return jsonify({"success": False, "message": str(e)}), 502


@app.route("/api/hot")
def api_hot():
    data = hot_list(request.args.get("force") == "1")
    if not data["items"] and len(data["hot"]["errors"]) == len(HOT["equipments"]):
        return jsonify({"success": False, "message": " / ".join(data["hot"]["errors"])}), 502
    return jsonify({"success": True, **data})


KIND_LABEL = {"lot": "製造", "inspect": "点検", "stop": "設備停止"}


@app.route("/api/cm3/schedule.csv")
def api_schedule_csv():
    """Raw CSV for other tools (values before 読み替え・書式). The screen exports its own CSV."""
    try:
        setsubi = _setsubi()
        data = schedule_with_master(setsubi)
    except (ValueError, cm3_client.Cm3Error) as e:
        return Response(str(e), status=502, mimetype="text/plain; charset=utf-8")

    mcols = needed_master_columns()
    buf = io.StringIO()
    w = csv.writer(buf, lineterminator="\r\n")
    w.writerow(["区分"] + [v for _, v in SCHEDULE_COLUMNS] + ["総パス", "設備停止内容", "台帳一致"]
               + ["[台帳]" + master_mod.label(c) for c in mcols])
    for it in data["items"]:
        row = [KIND_LABEL.get(it["rowKind"], "")]
        row += ["" if it.get(k) is None else it.get(k) for k, _ in SCHEDULE_COLUMNS]
        row += ["" if it.get("passTotal") is None else it["passTotal"], it.get("stopText", "")]
        row.append("" if it.get("mHit") is None else ("○" if it["mHit"] else "×"))
        row += [(it.get("m") or {}).get(c, "") for c in mcols]
        w.writerow(row)

    fname = "cm3_%s_%s_raw.csv" % (setsubi, time.strftime("%Y%m%d_%H%M"))
    return Response(buf.getvalue().encode("utf-8-sig"), mimetype="text/csv",
                    headers={"Content-Disposition": "attachment; filename=%s" % fname})


@app.route("/api/export/xlsx", methods=["POST"])
def api_export_xlsx():
    """Excel file of the table as shown (the screen sends the cells, colors and widths)."""
    body = request.get_json(force=True, silent=True) or {}
    try:
        data = xlsx_writer.build_xlsx(body)
    except (ValueError, TypeError, KeyError) as e:
        return jsonify({"success": False, "message": "Excelを作れませんでした: %s" % e}), 400
    key = re.sub(r"[^0-9A-Za-z_\-]", "_", str(body.get("fileKey") or body.get("sheet") or "cm3"))
    fname = "cm3_%s_%s.xlsx" % (key, time.strftime("%Y%m%d_%H%M"))
    return Response(data, mimetype="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                    headers={"Content-Disposition": "attachment; filename=%s" % fname})


@app.route("/api/cm3/raw")
def api_raw():
    """CM3 response as-is (for checking field names and stop rows)."""
    try:
        setsubi = _setsubi()
        raw = cm3_client.fetch_raw(CONFIG, setsubi, EQUIPMENTS[setsubi].get("term_kbn", "4"))
    except (ValueError, cm3_client.Cm3Error) as e:
        return Response(str(e), status=502, mimetype="text/plain; charset=utf-8")
    return Response(json.dumps(raw, ensure_ascii=False, indent=2),
                    mimetype="application/json; charset=utf-8")


# ---------------------------------------------------------------- color settings

@app.route("/api/ui/settings", methods=["GET", "POST"])
def api_ui_settings():
    global UI
    if request.method == "POST":
        body = request.get_json(force=True, silent=True) or {}
        ui = copy.deepcopy(DEFAULT_UI)
        kws = body.get("inspect_keywords")
        if isinstance(kws, list):
            ui["inspect_keywords"] = [str(k).strip() for k in kws if str(k).strip()]
        for k, v in (body.get("colors") or {}).items():
            if k in ui["colors"] and isinstance(v, str) and _HEX.match(v):
                ui["colors"][k] = v.lower()
        _write_json(UI_FILE, ui)
        UI = ui
        with _cache_lock:
            _cache.clear()      # row kinds depend on the inspection keywords
    return jsonify({"success": True, "settings": UI, "default": DEFAULT_UI})


# ---------------------------------------------------------------- view (column) settings

@app.route("/api/view/settings", methods=["GET", "POST"])
def api_view_settings():
    global VIEW
    if request.method == "POST":
        body = request.get_json(force=True, silent=True)
        err = valid_view(body)
        if err:
            return jsonify({"success": False, "message": err}), 400
        body.setdefault("maps", {})
        body.setdefault("rules", [])
        body["version"] = VIEW_VERSION
        _write_json(VIEW_FILE, body)
        VIEW = body
    return jsonify({"success": True, "settings": VIEW})


@app.route("/api/view/default")
def api_view_default():
    return jsonify({"success": True, "settings": default_view()})


@app.route("/api/view/sample")
def api_view_sample():
    """Lot rows with ALL master columns (for previews in the column designer)."""
    try:
        setsubi = _setsubi()
        data = get_schedule_cached(setsubi)
    except (ValueError, cm3_client.Cm3Error) as e:
        return jsonify({"success": False, "message": str(e)}), 502
    MASTER.refresh()
    items = []
    for it in data["items"]:
        if it["rowKind"] == "stop":
            continue
        it = dict(it)
        r = MASTER.row_for(it) if it["rowKind"] == "lot" else None
        it["m"] = {c: master_mod.display_value(v) for c, v in r.items()} if r else ({} if it["rowKind"] == "lot" else None)
        it["mHit"] = (r is not None) if it["rowKind"] == "lot" else None
        items.append(it)
        if len(items) >= 300:
            break
    return jsonify({"success": True, "items": items, "masterAll": MASTER.columns(),
                    "masterKeyCols": MASTER.key_columns()})


# ---------------------------------------------------------------- lot master

def _cached_lot_items():
    """Lot rows of the schedules already fetched (used to test keys)."""
    items = []
    with _cache_lock:
        for _, data in _cache.values():
            items += [it for it in data["items"] if it["rowKind"] == "lot"]
    return items


@app.route("/api/master")
def api_master():
    MASTER.refresh()
    lots = {master_mod.norm_key(it["lotNo"]) for it in _cached_lot_items() if it["lotNo"]}
    return jsonify({
        "success": True,
        "scheduleLots": len(lots),
        "candidates": MASTER.candidates(lots),
        **MASTER.describe(),
    })


@app.route("/api/master/reload", methods=["POST"])
def api_master_reload():
    MASTER.refresh(force=True)
    return jsonify({"success": True, **MASTER.describe()})


def _valid_key_body(body):
    valid_fields = {k for k, _ in SCHEDULE_FIELDS}
    keys = body.get("keys") or []
    if not keys:
        return "キーを1つ以上指定してください"
    for k in keys:
        if k.get("schedule") not in valid_fields:
            return "スケジュール側の項目が不正です: %s" % k.get("schedule")
    t = body.get("table") or ""
    if t and t not in MASTER.tables:
        return "テーブルが見つかりません: %s" % t
    cols = MASTER.tables.get(t or MASTER.auto_table, [])
    for i, k in enumerate(keys):
        m = k.get("master") or ""
        if (m or i > 0) and m not in cols:
            return "台帳の列が見つかりません: %s" % (master_mod.label(m) or "(未選択)")
    seen = set()
    for k in keys:
        p = (k.get("schedule"), k.get("master") or "")
        if p in seen:
            return "同じキーが重複しています"
        seen.add(p)
    return ""


@app.route("/api/master/test", methods=["POST"])
def api_master_test():
    body = request.get_json(force=True, silent=True) or {}
    err = _valid_key_body(body)
    if err:
        return jsonify({"success": False, "message": err}), 400
    res = MASTER.test_keys(body, _cached_lot_items())
    return jsonify({"success": True, **res})


@app.route("/api/master/settings", methods=["POST"])
def api_master_settings():
    body = request.get_json(force=True, silent=True) or {}
    err = _valid_key_body(body)
    if err:
        return jsonify({"success": False, "message": err}), 400
    MASTER.apply_settings(body)
    return jsonify({"success": True, **MASTER.describe()})


# ---------------------------------------------------------------- main

def main():
    host = CONFIG.get("host", "127.0.0.1")
    port = int(CONFIG.get("port", 5000))
    url = "http://%s:%d/" % ("127.0.0.1" if host == "0.0.0.0" else host, port)
    print("CM3 Schedule Viewer: %s" % url)
    print("Lot master: copying / loading ...")
    MASTER.refresh(force=False)
    print("Lot master: %s %s rows=%d key=%s" % (
        MASTER.status["state"], MASTER.status["message"], MASTER.row_count, MASTER.key_label()))
    if CONFIG.get("open_browser", True) and "--no-browser" not in sys.argv:
        threading.Timer(1.2, lambda: webbrowser.open(url)).start()
    app.run(host=host, port=port, debug=False, threaded=True)


if __name__ == "__main__":
    main()
