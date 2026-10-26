# CM3 スケジュールビューア（ScheduleView）

CM3 スケジューリングシステムの設備ごとの作業予定（L-1・L-2・L-3）を一覧で見て、仕掛台帳（LOTNOW.sqlite3）の項目を突き合わせて並べるアプリです。
HOT コイル作業順（前工程が HOT のロットを 3 設備まとめて作業予定順に）・列の設定（並び・計算列・読み替え・色分け）・CSV／Excel 出力ができます。

版 12.0.0 から、**exe（`ScheduleView.exe`）だけで動くデスクトップ版**になりました。Python・Flask は要りません。

## 配り方（フォルダの形）

```
ScheduleView\
  ScheduleView.exe        ← これを開く
  program\app\            画面・現場の設定（config.json）・設定の初期値・名前・版
  README.md
```

配る ZIP（`ScheduleView-<版>-windows.zip`）は GitHub の Releases に版ごとに載ります（main に入ると CI が作って載せる）。

## はじめて使う

1. 共有フォルダ（または展開したフォルダ）の `ScheduleView.exe` を開きます。
2. 起動画面に段が出て、**この PC へ写してから開きます**（`%LOCALAPPDATA%\ScheduleView\app`）。2 回目からは共有に関係なく起動できます。
3. 「ショートカットを作りますか」と尋ねるので、作るとデスクトップとスタートに、この PC のアプリを指すショートカットができます。

配る版を決めてあれば、置き場のアプリのフォルダにある **配る入口**（`<置き場>\ScheduleView\ScheduleView.exe`）のアドレスを渡すだけで、新しい PC にも入ります。

## 更新のしかた

- 各 PC は、起動したとき・使っている間 30 分ごとに **更新の置き場**（`program\app\config.json` の `update.source`）を確かめ、新しい版（配る版が決まっていればその版）を裏で取り込みます。
- 取り込み終えると画面に知らせが出ます。開き直すと新しい版に入れ替わります。閉じている間に配った版も、ショートカットから 1 回開くだけで入ります。
- 新しい版が開けなかったときは、印を付けて前の版に戻して開き直します（その版はもう取り込みません）。

### 版を配る（管理する人）

1. Releases から `ScheduleView-<版>-windows.zip` を取ります。
2. アプリの「版の配布」で ZIP を選ぶと、中身を確かめてから更新の置き場に版のフォルダ（`<置き場>\ScheduleView\<版>\`）を作ります。置いただけでは配りません。
3. 試してから「この版を配る」を押すと、`distribute.json` と配る入口ができ、各 PC がその版にそろいます。前の版を選び直せば戻せます。

置いてよいのは、置き場の共有フォルダに書ける人です（書けなければ理由を出して断ります）。

## 設定（`program\app\config.json`）

| 項目 | 意味 |
|---|---|
| `cm3_base_url`・`init_path`・`client_ip`・`timeout_sec`・`use_system_proxy` | CM3 への問い合わせ（プロキシは既定で通さない。`true` のときは環境変数 HTTP_PROXY などを使う） |
| `cm3_view_url_template`・`lot_url_template` | 「CM3画面を開く」・ロット番号のリンク（いつものブラウザで開く） |
| `equipments`・`default_setsubi` | 設備のタブ |
| `master` | 台帳の元ファイル（共有）・手元の写し（作業場所の下）・確かめる間隔 |
| `hot` | HOT コイル作業順の設備・前工程の値・列 |
| `cache_sec`・`auto_refresh_sec` | スケジュールを覚える秒・画面の自動更新の秒 |
| `settings_dir` | 列の設定・配色・台帳設定の置き場（空ならこの PC の作業場所の `settings`。共有のフォルダを書けば全員で同じ設定） |
| `update.source` | 更新の置き場（共有フォルダ。「;」で候補を並べられる・`%USERNAME%` などの環境変数も使える） |

この PC の作業場所（`%LOCALAPPDATA%\ScheduleView`）: `app`（この PC のアプリ）・`settings`（設定）・`data`（台帳の手元の写し）・`logs\desktop.log`（記録）・`update`（取り込んだ版・控え）。

前の版（ブラウザ版）の列の設定を引き継ぐときは、前の `app\view_settings.json` を「列の設定」→「設定ファイル ▾」→「読み込み…」で読み込むか、
`settings` フォルダへ写してください（配色は `ui_settings.json`、台帳設定は `master_settings.json`）。

## 開発する人へ

- 作り: `desktop/`（Rust・Tauri）が窓とアプリの中身のすべて。画面は `program/app/web`。設計は `program/docs/DESIGN.md`、決まりは `CLAUDE.md`。
- 作る: `cd desktop && cargo build --release`（Windows では Rust と Visual Studio の C++ ビルドツール、Linux では WebKitGTK が要る。アイコンを描くのに Python も使う）。
- 確かめる: `CLAUDE.md` の「push 前の確かめ」。旧版（Python）の答えとの突き合わせは `cargo test`（`desktop/tests/parity.rs`）。
- 画面だけをブラウザで見る: `cd desktop && cargo build --release --example serve` のあと
  `SV_FIXTURE=<fixture のフォルダ> desktop/target/release/examples/serve 18080`（作り物のデータは `python program/tools/fixture.py <フォルダ>`）。
- 速さを測る: `cargo run --release --example bench -- <台帳の行数> <CM3 の遅れの秒>`。
