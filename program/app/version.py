# -*- coding: utf-8 -*-
"""アプリの版と更新履歴（LotScope と同じ形）。

- APP_VERSION は「大.中.小」。配るたびに上げる（直した・足した: 小、画面や使い方が変わる: 中、作り直し: 大）。
  exe（desktop/Cargo.toml・desktop/tauri.conf.json の version）と同じにする（tests/test_version.py が止める）。
  exe は起動のとき、この行（APP_VERSION = "X.Y.Z"）を読んで版を名乗り、更新の置き場の版と比べる。
- CHANGELOG の先頭は APP_VERSION と同じ。新しい版を上に足す。
- RELEASE は「配った版と、そのときの配る物の中身の要約」。手で書かず `python app_build.py --release` で書く。
  手順: 直す → APP_VERSION を上げ、CHANGELOG の先頭に1件足す → python app_build.py --release → 試験
"""
APP_VERSION = "12.0.0"
RELEASE = {"version": "12.0.0", "digest": "dd5bae89971ee004"}

CHANGELOG = [
    {"version": "12.0.0", "date": "2026-10-26", "notes": [
        "exe（ScheduleView.exe）から起動するデスクトップ版にしました。Python・Flask は要りません（中身はすべて exe が受け持ちます）。",
        "台帳（LOTNOW）の写しと読み込みを裏で行うようにしました。読み込み中も一覧は前の台帳のまま見られ、読み込みは速くなりました。",
        "HOTコイル作業順は、L-1・L-2・L-3 のスケジュールを同時に取りに行くようにしました（待ち時間がいちばん遅い設備の分だけになります）。",
        "配布と更新の仕組みを入れました。共有の置き場に版を置いて配ると、各 PC は起動のとき・30 分ごとに確かめて取り込み、開き直すと新しい版になります。",
        "共有にある exe を開くと、この PC へ写してから開き、ショートカットを作るか尋ねます（次からは共有に関係なく起動できます）。",
    ]},
    {"version": "11.0.0", "date": "2026-10-19", "notes": [
        "ブラウザ版（start.bat で Flask を起こし、ブラウザで開く）。",
    ]},
]


def version_key(v):
    """版を数の組に（1.10.0 は 1.9.0 より新しい）。"""
    return tuple(int(x) for x in (v.split(".") + ["0", "0"])[:3])
