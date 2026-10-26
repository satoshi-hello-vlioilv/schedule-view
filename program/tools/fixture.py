# -*- coding: utf-8 -*-
"""試験・開発・測定に使う作り物のデータ（CM3 の応答と台帳 LOTNOW.sqlite3）を作る。

    python program/tools/fixture.py <出力フォルダ> [台帳の行数=6881] [ロットの数=60]

出力:
    cm3/<設備>.json      CM3 の init の応答（設備ごと。L-1・L-2・L-3）
    LOTNOW.sqlite3       台帳（テーブル「仕掛」と「_更新情報」。全列 TEXT。本物と同じ形）

本物で確かめた形（program/docs/CM3_API_spec.md）をなぞる: 日付の付く行と時刻だけの行・日付の繰り上がり・
同じロットの複数パス・設備停止行（停止名を項目の順に区切って入れる）・点検板・文字化け・HTML の文字参照・
台帳に無いロット・前工程が HOT のロット。乱数の種は固定（何度作っても同じ物）。
"""
from __future__ import annotations

import json
import random
import sqlite3
import sys
from pathlib import Path

EQUIPMENTS = ["L-1", "L-2", "L-3"]
UPDATE_TS = 1792384683000            # 2026/10/19 13:38:03（日本時間）
SEED = 20261019

# 台帳の主な列（本物の列名。半角カナのまま）
MAIN_COLUMNS = ["ﾛｯﾄ番号", "保留_区分", "特急区分", "ｵｰﾀﾞｰ板厚", "ｵｰﾀﾞｰ板幅", "製造材質", "製造調質",
                "実績_設備ｺｰｽ", "残仕掛設備ｺｰｽ", "設計_PIT炉番炉順", "BOX実績_良品重量", "工場納期", "約束納期"]
FILLER = 100                          # 本物は 100 列を超える。読み込みの重さを本物に近づける


def lot_no(rng: random.Random) -> str:
    return rng.choice("LNK") + "".join(rng.choice("0123456789") for _ in range(4)) + rng.choice("ABCSXZJE") + "0"


def build_master(path: Path, rows: int, lots: list[str], hot: dict[str, str]) -> None:
    """台帳を作る。lots は CM3 に出るロット（先頭の 9 割を台帳に入れる）。hot はロット → HOT の直後の設備。"""
    rng = random.Random(SEED + 1)
    path.unlink(missing_ok=True)
    con = sqlite3.connect(path)
    cols = MAIN_COLUMNS + [f"列{i:03d}" for i in range(1, FILLER + 1)]
    con.execute("CREATE TABLE 仕掛 (%s)" % ", ".join('"%s" TEXT' % c for c in cols))
    con.execute('CREATE TABLE _更新情報 ("項目" TEXT, "値" TEXT)')
    con.executemany("INSERT INTO _更新情報 VALUES (?, ?)", [("作成日時", "2026-10-19T13:30:00"), ("件数", str(rows))])
    in_master = lots[: int(len(lots) * 0.9)]
    seen, keys = set(), []
    for lot in in_master:
        if lot not in seen:
            seen.add(lot)
            keys.append(lot)
    while len(keys) < rows:
        k = lot_no(rng)
        if k not in seen:
            seen.add(k)
            keys.append(k)
    batch = []
    for i, k in enumerate(keys[:rows]):
        eq = hot.get(k)
        if eq:
            done, rest = "HOT", f"{eq} TLV KEN"
        else:
            done = " ".join(["HOT"] + rng.sample(["L-1", "L-2", "L-3", "ANI", "DL2"], rng.randint(1, 3)))
            rest = " ".join(rng.sample(["L-1", "L-2", "L-3", "TLV", "KEN", "ANF"], rng.randint(0, 3)))
        row = [
            k, rng.choice(["0", "8"]), rng.choice(["0", "1", "2"]),
            "%.3f" % rng.choice([0.3, 0.35, 0.508, 1.2, 35.0]), "%.2f" % rng.choice([840, 1020, 1220]),
            rng.choice(["NP56", "Z3R1", "A1100"]), rng.choice(["H14", "H18", "O"]),
            done, rest, (f"{rng.randint(1, 9)}-{rng.randint(1, 4)}" if eq or i % 3 == 0 else ""),
            "%.1f" % rng.uniform(1000, 9000), f"2026-{rng.randint(10, 12):02d}-{rng.randint(1, 28):02d} 00:00:00",
            f"2026-10-{rng.randint(1, 28):02d} {rng.randint(0, 23):02d}:{rng.randint(0, 59):02d}:42",
        ] + [f"値{i % 97}-{j}" for j in range(FILLER)]
        batch.append(row)
        if len(batch) >= 5000:
            con.executemany("INSERT INTO 仕掛 VALUES (%s)" % ",".join("?" * len(cols)), batch)
            batch = []
    if batch:
        con.executemany("INSERT INTO 仕掛 VALUES (%s)" % ",".join("?" * len(cols)), batch)
    # 同じキーの行（重複。先頭の 1 件を使い、件数を出す）
    if keys:
        con.execute("INSERT INTO 仕掛 (\"ﾛｯﾄ番号\", \"製造調質\") VALUES (?, ?)", (keys[0], "重複"))
    con.commit()
    con.close()


def mojibake(s: str, enc: str = "latin-1") -> str:
    """日本語を、UTF-8 のバイト列を欧文（latin-1・cp1252）として読んだ形にする（CM3 で見た文字化け）。"""
    return s.encode("utf-8").decode(enc)


def cm3_response(eq: str, lots: list[str], rng: random.Random) -> dict:
    """1 設備の init の応答。"""
    rows = []
    month, day, hh, mm = 10, 19, 13, 40
    for i, lot in enumerate(lots):
        mm += rng.randint(5, 25)
        if mm >= 60:
            hh, mm = hh + mm // 60, mm % 60
        new_day = hh >= 24
        if new_day:
            hh -= 24
            day += 1
        when = (f"{month:02d}/{day:02d} {hh:02d}:{mm:02d}" if i == 0 or new_day else f"     {hh:02d}:{mm:02d}")
        if i % 9 == 4:
            # 設備停止行: 停止名を項目の順に区切って入れる（roll → lotNo → zaishitsu）。時間は pass へ
            rows.append({"roll": "ワークロ", "lotNo": "ール交換計", "zaishitsu": "画*", "inItaAtsu": None, "outItaAtsu": None,
                         "reienHaba": None, "seihinItaAtsu": None, "seihinItaHaba": None, "keijo": None, "yotoMei": None,
                         "jiKotei": None, "kanryoYotei": when, "pass": "7分", "toroku": None,
                         "rowStyle": "cmschview-data-stop", "teisiStyle": "teisi", "ltNoRendered": False})
            continue
        if i % 13 == 6:
            rows.append({"roll": None, "lotNo": "点検板", "zaishitsu": "A1100", "inItaAtsu": "0.50", "outItaAtsu": "0.40",
                         "reienHaba": "1000", "seihinItaAtsu": "0.40", "seihinItaHaba": "1000", "keijo": "Sｺｲﾙ",
                         "yotoMei": "点検用", "yotoMeiFull": "点検用", "jiKotei": "-", "kanryoYotei": when,
                         "rowStyle": "cmschview-data-white", "teisiStyle": None, "ltNoRendered": True})
            continue
        t_in = rng.choice(["1.2", "0.9", "0.7", "0.508", "0.22"])
        t_out = "%.2f" % (float(t_in) * rng.uniform(0.55, 0.8))
        yoto = rng.choice(["カーエアコンフィン", "&#12450;&#12523;&#12511;箔", "缶材<br>(蓋)", mojibake("印刷版"), mojibake("計画", "cp1252"), "ﾌｨﾝ材"])
        rows.append({
            "roll": None, "lotNo": lot, "zaishitsu": rng.choice(["Z3R1", "NP56", "Ｚ３Ｒ２"]),
            "inItaAtsu": t_in, "outItaAtsu": t_out, "reienHaba": rng.choice(["1340", "1020.0", " 840 "]),
            "seihinItaAtsu": "0.07", "seihinItaHaba": "430", "keijo": rng.choice(["Sｺｲﾙ", "Sｲﾀ", "ﾊｸｼﾞ"]),
            "yotoMei": yoto[:6] + "...", "yotoMeiFull": yoto, "jiKotei": rng.choice([eq, "LS3", "TLV", "ANI", "KEN"]),
            "kanryoYotei": when, "lotToiawaseUrl": f"http://example.invalid/lotdsp/?ltno={lot}&mode=2",
            "rowStyle": "cmschview-data-white" if i % 2 else "cmschview-data-palegreen", "teisiStyle": None,
            "ltNoRendered": True, "pass": None, "toroku": None,
        })
    return {"status": "SUCCESS", "alerts": {"alerts": []},
            "entity": {"setsubi": eq, "setsubiName": eq, "termKbn": "4", "error": False, "errorMessage": None,
                       "updateTs": UPDATE_TS, "meisaiList": rows}}


def build(out: Path, master_rows: int = 6881, lots_per_eq: int = 60) -> dict:
    rng = random.Random(SEED)
    out.mkdir(parents=True, exist_ok=True)
    (out / "cm3").mkdir(exist_ok=True)
    all_lots: list[str] = []
    hot: dict[str, str] = {}
    for eq in EQUIPMENTS:
        uniq = [lot_no(rng) for _ in range(lots_per_eq)]
        seq: list[str] = []
        for j, lot in enumerate(uniq):
            seq.extend([lot] * (3 if j % 7 == 0 else 1))          # 同じロットの複数パス
            if j % 4 == 1:
                hot[lot] = eq                                       # 前工程が HOT・次がこの設備
        if eq == "L-2" and len(uniq) > 9:
            hot[uniq[9]] = "L-3"                                    # 次の設備が別（HOT 一覧から外れる）
        all_lots.extend(seq)
        (out / "cm3" / f"{eq}.json").write_text(json.dumps(cm3_response(eq, seq, rng), ensure_ascii=False, indent=1),
                                                 encoding="utf-8")
    build_master(out / "LOTNOW.sqlite3", master_rows, all_lots, hot)
    return {"out": str(out), "masterRows": master_rows, "lots": len(set(all_lots)), "hot": len(hot)}


if __name__ == "__main__":
    a = sys.argv[1:]
    if not a:
        sys.exit(__doc__)
    print(json.dumps(build(Path(a[0]), int(a[1]) if len(a) > 1 else 6881, int(a[2]) if len(a) > 2 else 60),
                     ensure_ascii=False))
