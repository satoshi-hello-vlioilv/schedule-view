# CLAUDE.md

このリポジトリで作業するときの決まり（利用者が決めたこと）。どのアプリにも当てはまる決まりと、このリポジトリの手順とを分けて書く。

## 返答・報告・解説は、常に日本語で書く

利用者への返答は、どんなに短くても日本語で書く。英語に戻さない。対象は次の全て:

- 返答と解説
- 途中の一言（いま何をしているか）
- 状況報告（CI の結果、PR の状態、定期確認の結果、マージ、exe の配置）
- PR の説明・コミットの説明

## push の決まり

1. **push の前に、手元で型・構文（typecheck）・lint・test をすべて通す（green にしてから push する）。**
   CI で初めて失敗に気づく push はしない。下の「push 前の確かめ」を全部通してから push する。1 つでも落ちていれば push しない。
   文書だけの変更でも同じ。
2. **レビューの指摘は、1 件ずつ直して push しない。まとめて直し、1 回で push する。**
   届いた指摘をすべて直し、「push 前の確かめ」を通してから 1 回だけ push する。
3. 失敗が残っているときは push しない。直せない理由があるときは、push せずに利用者へ伝える。

## 構成（どこに何があるか）

| 場所 | 中身 |
|---|---|
| `program/app/` | 配る物のうち exe 以外: 画面（`web/`）・現場の設定（`config.json`）・設定の初期値（`defaults/`）・名前（`brand.json`）・版と更新履歴（`version.py`）・アイコンの絵（`app_icon.py`） |
| `program/tools/` | 開発の道具（作り物のデータ・作り物の CM3・旧版の答えの記録・画面の撮影と採点） |
| `program/tests/` | Python の試験（版・名前・道具） |
| `program/docs/` | 設計書（`DESIGN.md`）・CM3 の仕様書 |
| `desktop/` | exe（Rust・Tauri）。窓・起動・配布・更新と、アプリの中身（CM3・台帳・列の設定・Excel）のすべて |
| `desktop/bundle/` | 配る ZIP を作る（`bundle.py`）・本物の窓での通しの確かめ（`verify.py`） |

利用者の PC で動くのは exe だけ（Python は要らない）。Python は開発の道具・試験・作るときのアイコンの描画に使う。

## push 前の確かめ（CI と同じ物を手元で）

リポジトリの最上位で実行する:

```sh
# 1. 型・構文（JavaScript・Python・Rust）
bash program/tools/js-check.sh   # import を使う .js はモジュールとして確かめる（node --check だけでは見逃す）
python -m compileall -q program desktop/bundle
(cd desktop && cargo build --locked)
# 2. lint（Python・Rust）
ruff check program desktop/bundle
(cd desktop && cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings)
# 3. テスト（Python・Rust）
(cd program && python -m unittest discover -s tests -t .)
python -m unittest discover -s desktop/bundle -t desktop/bundle
(cd desktop && cargo test --locked)
```

- 型検査の道具（TypeScript・mypy）は入れていないので、JavaScript・Python は構文の確かめで代えている。入れたら差し替える。Rust はコンパイラが型を確かめる。
- JavaScript の lint の設定と、JavaScript だけのテスト（`npm test`）はまだ無い。画面の振る舞いは本物の窓の自己診断（`desktop/src/selftest.js`）と撮影の道具で確かめる。
- CI（`.github/workflows/ci.yml`）も同じ物を流す。ruff と Rust（clippy）は手元と同じ版に留める（ruff 0.15.20・Rust 1.97.0。
  版が上がると決まりが増え、手元で通った物が CI で落ちるため）。上げるときは手元と CI を一緒に上げる。
- 配る物（`program/app/`・`desktop/` の exe の元）を変えたら、版を上げる: `program/app/version.py` の `APP_VERSION` と `CHANGELOG` の先頭、
  `desktop/Cargo.toml` と `desktop/tauri.conf.json` の `version` をそろえ、`cd program && python app_build.py --release` で `RELEASE` を書き直す
  （`tests/test_version.py` が上げ忘れと食い違いを止める）。まだ配っていない版を直しただけなら `--amend`。
- 窓（Tauri）を Linux で作るには WebKitGTK などが要る（`libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libxdo-dev libssl-dev`）。
- Windows だけのコード（`#[cfg(windows)]`）は Linux の確かめではコンパイルされない。変えたときは `rustup target add x86_64-pc-windows-msvc` のうえで
  `cargo clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings` も通す。窓全体が Linux から Windows 向けに組めないときは、その部分だけを
  `windows` クレートに頼る小さな作業用クレートへ写し、`cargo check --target x86_64-pc-windows-msvc` で型を確かめる。
- 起動・更新・配布の流れ（`desktop/src/install.rs`・`update.rs`・`distribute.rs`・`main.rs`）を変えたら、`desktop/bundle/verify.py` の通し
  （local・update・distribute）を本物の窓で確かめる（Windows は CI。手元の Linux では Windows だけの部分を差し替えて流す）。
- Windows の本物の WebView2 での自己診断は CI（`.github/workflows/desktop.yml`）が行う。考え方は `program/docs/DESIGN.md`。
- 結果は**終了コードと「Ran … / OK / FAILED」「test result: ok」の行で**読む。出力の最後の 1 行だけ（`tail -1` など）で判断しない
  （Python の試験は最後に ResourceWarning などが出ることがあり、FAILED の行が隠れる）。

## 直すとき・移すときは、評価関数を先に作る

不具合を直す・作りを移す・速くするときは、先に「何が正しいか」を確かめる物（評価関数）を作り、問題を数で明らかにしてから取りかかる。

- 答えが変わってはいけない作り替えは、前の作りの答えを記録して突き合わせる（例: 旧版の答え `desktop/tests/golden/python.json` と `desktop/tests/parity.rs`）
- 速さ・作りの改良は、前と後を同じデータ・同じ手順で測る（例: `program/tools/fixture.py` の作り物のデータ）
- 評価関数が甘くないかを、わざと壊して確かめる（壊したら落ちること）

## 改良するときは、別のエージェントが現物を撮って採点し、合格点以上の案に決める

UI/UX に限らず、何かを改良するときは、必ず次の順で設計する。案を文章だけで採点しない。案を作った者が自分の案を採点しない。
利用者に選んでもらうのを待たず、この流れで自律的に 1 案に決めて作る。

1. 今の物を撮り（`program/tools/ui-check.mjs`）、画像を見て、何が使いにくいか・分かりにくいかを、画像で見えたことを根拠に挙げる
   （画面に出ない改良〈速さ・作り・保存の確かさ など〉は、動かして測った数を根拠にする）
2. 改良案を**最低 5 案**出す。案の方向（何で解くか: 置き場・見せ方・減らし方・流れの変え方 など）を並べ、偏っていないかを確かめる
3. **採点に渡す前に、案を作った者が自分で画像を見て確かめる**。案ごとに `program/tools/ui-variants.mjs` で撮った画像
   （1728・1536・1280）を自分で開き、案の狙いどおりに見えるか・再現の崩れ（切れ・重なり・折り返し・空白・届かない操作）が無いかを見て、
   崩れていれば案の定義を直して撮り直す。確かめたこと・直したことは採点のファイルの回の `"self_check"` に書く（無ければ `ui_score.py` が判定しない）。
   作った後（7）も、採点に渡す前に自分で前後の画像を見て確かめる
4. 採点は**別のエージェント**（Agent。案を作っていない者）に頼む。採点する者は、自分で案ごとに画面を撮り（`ui-variants.mjs`）、
   画像の現物を直接見て採点する（画面に出ない改良は、自分で動かして測る）。採点の物差しは下の加点と減点。結果は採点のファイル（JSON。
   `program/tools/ui-proposals/`）に書き、`python program/tools/ui_score.py <ファイル>` で合計と判定を出す
5. 1 位が**合格点以上**なら、1 位に決める（同じ点なら、重い分類の点が高い方）。合格点は**案出しの回によらず一律 80 点**。
   採点のファイルには `"rule": "2026-10-17"` を書く
6. 合格点に届かないときは決めない。低い項目と減点を見直し、**案出しからやり直す**（5 案以上。前の回で効いた所は持ち越してよいが、
   新しい案を 3 案以上入れる）。採点はまた別のエージェントが、撮り直した画像で付ける。3 回目でも届かなければ、いちばん高い案に決め、
   足りない点とその理由を利用者に伝える
7. 決めた案を作ったら、作る前と後を `ui-check.mjs` で撮り、別のエージェントが同じ物差しで採点する（採点のファイルの `"built"`）。
   作った後も、合格点以上か、減点が残っていないかを確かめ、届かなければ直して撮り直す。比べた画像は比較のページ（Artifact）で見せる

**加点**（100 点満点。4 分類・11 項目を 0〜10 で付け、重みを掛けて足す）

| 分類 | 項目（重み） |
|---|---|
| わかりやすさ（32） | 順序（8: 目と手が使う順に流れる）・チャンク（8: まとまりが一目で分かれる）・階層化（8: 最初に見る物が 1 つ・主と従の差）・視覚化（8: 数・状態を形・色・位置で見せる） |
| 使いやすさ（30） | ステップ数（10: 目的までの手が少ない）・考えさせない（10: 探させない・思い出させない・推測させない）・直感的（10: 見た目どおりに動く・慣れた型） |
| 統一感（20） | 形と色（10: 同じ物は同じ形・同じ色）・言葉と置き場（10: 同じ物は同じ言葉・同じ場所） |
| 美しさ（18） | 整列・余白・釣り合い（9）・色と文字の調子（9: 色数・強弱・読みやすさ） |

**減点**（見つけた所ごとに引く。どの画像のどこで見たかを書く）: 機能が消える・届かない −10、見切れ・はみ出し −5、重なり −5、
想定外の折り返し（1 段のはずが 2 段・ボタンの字が折れる） −3、読みにくい字（11px 未満・コントラスト 4.5 未満の本文） −3、
要らないスクロール −3、状態が見えない −3、前より手が増える −2／手、色だけで区別 −2

画面の大きさの前提: 利用者の PC は 2160×1440・表示倍率 125%（画面の CSS の大きさで 1728×1152）。ほかのフル HD 以上の PC
（1920×1080・125% で 1536×864）でも崩れないこと。
