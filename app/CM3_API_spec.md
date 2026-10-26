# CM3スケジューリングシステム データ取得API 仕様書

| 項目 | 内容 |
|---|---|
| 文書名 | CM3スケジューリングシステム データ取得API 仕様書 |
| 版 | 2.8 |
| 更新日 | 2026-10-19 |
| 対象 | CM3スケジュール表示画面（cmschview）の明細データ、および仕掛台帳（LOTNOW.sqlite3）との結合 |
| 想定読者 | CM3のスケジュールを自作アプリ（Python / Excel VBA / PowerShell / Webアプリ等）に取り込みたい人 |
| 根拠 | ブラウザ開発者ツール（Network / Fetch/XHR）で観測した実通信、LOTNOW.sqlite3 の実ファイル |

> **この文書について**
> CM3の公式仕様書ではありません。画面表示時の通信を観測して整理したものです。
> 各項目に **［確認済］**（実データで確認）／**［推定］**（観測からの推定）を付けています。
> CM3側・台帳側の改修で予告なく変わる可能性があります。

---

## 1. 目的と目標

### 目的

CM3の画面に表示されるスケジュールを、転記せずに他のアプリから直接データとして取得し、仕掛台帳（LOTNOW）の情報と結び付けて使えるようにする。

### 目標

| 項目 | 目標値 |
|---|---|
| CM3への通信回数 | 1設備あたり 1回（POST 1回で全明細を取得） |
| 応答待ち時間の上限 | 15秒 |
| CM3への取得間隔の下限 | 30秒 |
| 台帳の更新確認間隔 | 60秒（変更があったときだけコピー） |
| 対応設備 | L-1 / L-2 / L-3 |
| 行の区分 | 製造／点検／設備停止 の3区分 |

---

## 2. まとめ（最短で使うために）

```
POST  http://nlmfangyweb1a:9085/Cm3SchWeb/service/schapp/cmschview/init
Content-Type: application/json;charset=UTF-8

{ ...4.3 の要求JSON（設備コードを2か所差し替え）... }

→ 応答JSON の entity.meisaiList がスケジュール明細（配列の順番＝処理順）
→ 行を「製造／点検／設備停止」に分ける（5.3）
→ 製造行の lotNo（＋必要なら他の項目）と台帳の列を照合して結合（6章）
```

- 認証ヘッダ・ログインは不要（観測範囲）［確認済］
- 社内ネットワークからのみ到達可能。プロキシは通さず直接接続する
- 読み取り専用として使う

---

## 3. CM3の仕組み

### 3.1 画面URL ［確認済］

```
http://nlmfangyweb1a:9085/Cm3SchWeb/#/schapp/cmschview?termkbn=4&setsubi=L-2
```

| 部分 | 意味 |
|---|---|
| `/Cm3SchWeb/` | Webアプリのルート |
| `#/schapp/cmschview` | 画面の切替用。`#` 以降はブラウザ内でのみ処理され、サーバーには送られない |
| `termkbn=4` | 端末区分（値の意味は未確認） |
| `setsubi=L-2` | 表示する設備 |

### 3.2 画面URLを直接読んでもデータは取れない

CM3は AngularJS の画面です［確認済］。画面URLの中身は空の枠だけで、スケジュールは画面表示後に `init` API を呼んで取得しています。**他アプリからは `init` の呼び出しだけを再現すればよい。**

### 3.3 画面表示時の通信 ［確認済］

| 通信 | 役割 | 取り込みに必要か |
|---|---|---|
| `rules?rules=CMSchViewKoshinInfo&rules=CMSchViewInfo` | 画面ルール［推定］ | 不要 |
| `auth` | 端末情報［推定］ | 不要 |
| `model?model=CMSchViewKoshinInfo` | 画面モデル定義［推定］ | 不要 |
| `setsubiOptions?refresh=false` | 設備の選択肢［推定］ | 不要 |
| `CMSchView.html` などの `.html` | 画面テンプレート | 不要 |
| **`init`** | **スケジュール明細** | **必要** |

サーバーは Java Servlet（`X-Powered-By: Servlet/3.0`）［確認済］。

---

## 4. init API 詳細

### 4.1 基本情報 ［確認済］

| 項目 | 値 |
|---|---|
| URL | `http://nlmfangyweb1a:9085/Cm3SchWeb/service/schapp/cmschview/init` |
| メソッド | `POST` |
| 文字コード | UTF-8 |
| 要求・応答形式 | JSON |
| 正常時 HTTPステータス | 200 |

※ ブラウザ上の表記は `cmschview//init`（スラッシュ2つ）。404等になる場合はこちらで試す。

### 4.2 要求ヘッダ

| ヘッダ | 値 | 必須 |
|---|---|---|
| Content-Type | `application/json;charset=UTF-8` | 必須 |
| Accept | `application/json, text/plain, */*` | 推奨 |
| Origin | `http://nlmfangyweb1a:9085` | 任意（必須かは未確認） |
| Referer | `http://nlmfangyweb1a:9085/Cm3SchWeb/` | 任意（同上） |

### 4.3 要求JSON ［確認済］

**`setsubiName` と `urlSetsubi` の2か所** を取得したい設備コードに差し替えて送る。

```json
{
  "ip": "150.150.107.35",
  "userID": null,
  "termID": null,
  "setsubiName": "L-2",
  "useKbn": null,
  "termKbn": "4",
  "authInfo": null,
  "result": 0,
  "error": false,
  "errorMessage": "",
  "version": null,
  "setsubi": "",
  "updateTs": null,
  "meisaiList": [],
  "urlSetsubi": "L-2",
  "urlTermKbn": "4"
}
```

| 項目 | 設定する値 | 説明 |
|---|---|---|
| setsubiName / urlSetsubi | **設備コード** | 取得する設備 |
| termKbn / urlTermKbn | `"4"` | 端末区分 |
| ip | `"150.150.107.35"` | ブラウザが送っていた値。省略・変更の可否は未確認 |
| setsubi / meisaiList / updateTs | `""` / `[]` / `null` | 空で送ると、応答で中身が入る |
| その他 | 上記JSONのまま | |

### 4.4 設備コード

| 設備 | 設備コード | termKbn | 状態 |
|---|---|---|---|
| L-1 | `L-1` | `4` | 動作実績あり |
| L-2 | `L-2` | `4` | ［確認済］ |
| L-3 | `L-3` | `4` | 動作実績あり |

### 4.5 応答JSON ［確認済］

```json
{
  "status": "SUCCESS",
  "alerts": { "alerts": [] },
  "entity": {
    "setsubi": "L-2",
    "error": false,
    "errorMessage": null,
    "updateTs": 1792384683000,
    "meisaiList": [ { ...明細... } ]
  }
}
```

| 判定 | 条件 |
|---|---|
| 正常 | HTTP 200 かつ `status == "SUCCESS"` かつ `entity.error == false` |
| 異常 | 上記以外。`entity.errorMessage` に理由が入る［推定］ |

| entity の項目 | 説明 |
|---|---|
| updateTs | CM3側の更新日時。UNIX時間（ミリ秒）。例 `1792384683000` → 2026/10/19 13:38:03（日本時間） |
| meisaiList | 明細。**配列の順番が処理順** |

### 4.6 明細項目（製造ロット行）

| 項目名 | 日本語名 | 例 | 注意 |
|---|---|---|---|
| lotNo | ロットNo | `L4061S0` | 同じロットが複数行に出る（5.2）。台帳との照合キー |
| zaishitsu | 材質 | `Z3R1` | |
| inItaAtsu | 入側板厚 (mm) | `0.22` | 数値だが文字列 |
| outItaAtsu | 出側板厚 (mm) | `0.12` | 同上 |
| reienHaba | 冷延幅 (mm) | `1340` | 同上 |
| seihinItaAtsu | 製品板厚 (mm) | `0.07` | 同上 |
| seihinItaHaba | 製品幅 (mm) | `430` | 同上 |
| keijo | 形状 | `Sｺｲﾙ` `Sｲﾀ` `ﾊｸｼﾞ` | 半角カナ |
| yotoMeiFull | 用途名 | `カーエアコンフィン` | 省略なしの名称 |
| yotoMei | 用途名（省略表示） | `カーエアコンフ...` | 画面幅用。使わない |
| jiKotei | 次工程 | `L-2` `LS3` `TLV` `CAF` `DL2` `FS4` `ANF` `ANI` `KEN` | |
| kanryoYotei | 完了予定 | `10/19 14:05` / `     14:27` | 日付が省略される行がある（5.1） |
| lotToiawaseUrl | ロット照会URL | `http://nlmfangyweb1a/lotdsp/?ltno=L4061S0&mode=2` | |
| rowStyle / teisiStyle / ltNoRendered | 画面表示用 | | 行の区分には使わない（5.3） |
| roll / pass / toroku | 製造行では空 | `null` | 設備停止行では文字が入る（5.3） |

---

## 5. 取り込み時の加工ルール

### 5.1 完了予定の日付補完 ［確認済］

日付が変わった行にだけ日付が付き、それ以外は「空白5文字 + 時刻」。

1. 前後の空白を除く
2. `MM/DD HH:MM` なら、その日付を「現在の日付」として覚える
3. `HH:MM` のみなら「現在の日付」を付ける
4. 年は `updateTs` の年。12月→1月のように月が戻ったら翌年

**開始予定（推定）**：CM3は完了予定しか持たないため、**1つ前の行（設備停止行を含む）の完了予定** をその行の開始予定とみなす。先頭行は空欄（作業中の可能性）。

### 5.2 同じロットが複数行に出る ［推定］

前の行の出側板厚が次の行の入側板厚と一致するため、同じロットの複数回の圧延（パス）と推定。

| ロットNo | 入側板厚 | 出側板厚 |
|---|---|---|
| L8106X0（1回目） | 1.2 | 0.9 |
| L8106X0（2回目） | 0.9 | 0.7 |
| L8106X0（3回目） | 0.7 | 0.53 |

- 行数（パス数）とロット数（重複を除く）を区別する
- ロットNoだけを主キーにしない（配列順 または ロットNo＋出現回数）
- 台帳との結合は「多（スケジュール）対 1（台帳）」。同じロットの全パス行に同じ台帳情報が付く
- パス番号は **製造行だけ** で数える（点検行は数えない）

### 5.3 行の区分（製造／点検／設備停止）

| 順 | 区分 | 判定 | 例 |
|---|---|---|---|
| 1 | 設備停止 | データ項目がほぼ空で、時刻と文字だけの行（5.3.1） | `ワークロール交換計画 7分` |
| 2 | 点検 | 1に当たらず、ロットNo または 用途名に **点検キーワード** を含む行（5.3.3） | `点検板` |
| 3 | 製造 | 上記以外 | `N8230Z0` |

| 区分 | 製造件数・ロット数 | パス番号 | 台帳との突合せ | 表示 |
|---|---|---|---|---|
| 製造 | 数える | 付ける | する | 通常（白／縞） |
| 点検 | 数えない（別に件数を出す） | 付けない | しない | 黄色系・「点検」の印 |
| 設備停止 | 数えない（別に件数を出す） | 付けない | しない | 赤系・横幅いっぱいの1行 |

#### 5.3.1 設備停止行の見分け方 ［確認済］

対象8項目：`zaishitsu` `inItaAtsu` `outItaAtsu` `reienHaba` `seihinItaAtsu` `seihinItaHaba` `keijo` `jiKotei`

| 条件 | 判定 |
|---|---|
| 8項目のうち4項目以上が埋まっている　かつ　板厚・幅のどれか1つ以上が数値 | 製造または点検 |
| 上記以外 | 設備停止 |

`teisiStyle` `ltNoRendered` `lotNo` の形は判定に使わない（製造行でも値が変わることがあり誤判定になった）。

#### 5.3.2 停止内容の組み立て方 ［確認済］

CM3は1つの停止名を一定の文字数で区切り、**JSONの項目順（roll → lotNo → zaishitsu → …）** に入れている。

| 項目 | roll | lotNo | zaishitsu | 時間の項目 |
|---|---|---|---|---|
| 値 | `ワークロ` | `ール交換計` | `画` | `7分` |

1. 応答JSONの **項目の並び順のまま** 取り出す（並べ替えない）
2. 文字は **空白を入れずに** つなげる → `ワークロール交換計画`
3. `7分` のような時間は最後に空白1つで付ける → `ワークロール交換計画 7分`
4. 末尾の `*` は停止名の一部として残す

#### 5.3.3 点検行 ［確認済（画面で確認）］

`点検板` は圧延はするが点検のための材料で、板厚・幅のデータを持つ一方、台帳には無い。

| 項目 | 内容 |
|---|---|
| キーワード（初期値） | `点検板` |
| 照合対象 | `lotNo` と `yotoMeiFull` |
| 照合方法 | 全角／半角・大文字／小文字をそろえて（NFKC）、キーワードを **含む** か |
| 変更 | アプリの「配色」画面で追加・変更できる（1行1語） |

### 5.4 文字化け対策

| 原因 | 対処 |
|---|---|
| 応答の文字コードがUTF-8でない | Content-Type の charset → UTF-8 → CP932 の順で解読 |
| 日本語が欧文として読まれている（`è¨ˆç”»` など） | 欧文として戻し、UTF-8 または CP932 で読み直す |
| HTML文字参照（`&#35373;`） | 文字に戻す |
| HTMLタグ（`<br>`） | 除去 |

### 5.5 数値への変換

板厚・幅は文字列で届くため計算前に数値化する（`0.508` など小数3桁あり。丸めない）。

$$
\text{圧下率(\%)} = \frac{\text{入側板厚} - \text{出側板厚}}{\text{入側板厚}} \times 100
$$

---

## 6. 仕掛台帳（LOTNOW.sqlite3）との結合

### 6.1 台帳ファイル ［確認済］

| 項目 | 内容 |
|---|---|
| 場所 | `\\Nlmsrvngy03\Read\【New】仕掛\台帳\LOTNOW.sqlite3` |
| 形式 | SQLite 3（ジャーナル方式。WALではない） |
| テーブル | `仕掛`（明細）、`_更新情報`（項目・値） |
| ロット番号の列 | `ﾛｯﾄ番号`（半角カナ、7桁、重複なし） |
| 全列の型 | TEXT（数値・日付も文字列） |

### 6.2 共有ファイルの読み方（更新を妨げない） ［必須］

共有フォルダの台帳は別のプログラムが更新している。**共有ファイルを SQLite で直接開かない。**

```
共有フォルダ                       手元（アプリのフォルダ）
LOTNOW.sqlite3 ──ファイルコピー──→ data/LOTNOW.sqlite3.tmp
                                       │ 検査（PRAGMA quick_check）
                                       ▼ 正常なら置き換え
                                   data/LOTNOW.sqlite3 ──SQLite 読み取り専用──→ アプリ
```

| 手順 | 内容 | 理由 |
|---|---|---|
| 1 | 元ファイルの「サイズ＋更新日時」を前回コピー時と比べる | 変わっていなければコピーしない |
| 2 | 元ファイルの横に `-journal` / `-wal` があればコピーしない | 更新の途中であり、壊れた状態を拾う |
| 3 | 通常のファイルコピーで一時ファイルに複製 | ファイルコピーはSQLiteのロックを取らない |
| 4 | コピー後に元ファイルのサイズ・更新日時が変わっていたら破棄 | コピー中に更新された |
| 5 | 一時ファイルを `PRAGMA quick_check` で検査 | 壊れたファイルで前回分を上書きしない |
| 6 | 正常なら手元コピーと置き換え（`os.replace`） | 置き換えは一瞬で、読み込み中のファイルを壊さない |
| 7 | 手元コピーを読み取り専用（`mode=ro`）で開く | 手元コピーも変更しない |

元ファイルに届かない・更新中などのときは、**前回の手元コピーをそのまま使い**、画面に警告を出す。

### 6.3 ロット番号列の選び方（表記ゆれ対応）

**列名の正規化**：Unicode NFKC（半角カナ→全角、全角英数→半角）、英字を大文字、空白・`_`・`-`・`.`・`・` を除去。正規化後に `ロット番号` `ロットNO` `ロット` `LOTNO` `LOTNUMBER` のどれかと一致すれば「列名一致」。

**実データの判定**

| 指標 | 内容 |
|---|---|
| ロット形式 | 値が「英字1文字＋英数字6文字」（例 `N8230Z0`）である割合 |
| 重複なし | 値の重複がない割合 |
| スケジュール一致 | 取得済みスケジュールのロットNoのうち、その列に存在する件数 |

- 候補：列名一致、またはロット形式が50%以上の列
- 自動選択：列名一致 → ロット形式の割合 → 重複なしの割合 の順で最も高い列
- 「台帳設定」で候補と各指標を確認し、手動で選び直せる

### 6.4 突合せキー（単一キー／複数キー）

| 指定 | 動作 |
|---|---|
| 1組（初期値） | `lotNo ＝ ロット番号の列（自動判定）` |
| 複数組 | **すべての組が一致した行** を結合する（AND条件、複合キー） |

照合前に値をそろえる：NFKC、大文字化、空白をすべて除去、数値は数値として比較（`0.300` = `0.3`、`840.00` = `840`）。

- 1番目の組だけ、台帳の列を「自動判定」にできる
- どれかのキーの値が空の行は照合しない
- 台帳側で同じキーが複数ある場合は先頭の1件を使い、重複件数を表示
- 製造行だけを照合する
- 「一致を確認」で、保存前に一致件数・重複件数を確かめられる

### 6.5 台帳の値の表示

| 元の値 | 表示 |
|---|---|
| `2026-10-30 00:00:00` | `2026/10/30` |
| `2026-10-18 22:07:42` | `2026/10/18 22:07` |
| 列名 `ｵｰﾀﾞｰ板厚` | `オーダー板厚`（NFKCで全角化） |

台帳から結合した列は、スケジュールの列と同じ見た目で並べる（専用の色は付けない）。

### 6.6 主な列（`仕掛` テーブル）

| 列名 | 例 | 用途 |
|---|---|---|
| ﾛｯﾄ番号 | `N8230Z0` | キー |
| 保留_区分 / 特急区分 | `0` `8` / `0` `1` `2` | 保留・特急の確認 |
| ｵｰﾀﾞｰ板厚 / 板幅 | `35.000` / `1220.00` | |
| 製造材質 / 製造調質 | `NP56` / `H14` | |
| **実績_設備ｺｰｽ** | `HOT L-3 L-3 L-3` | これまでに通った設備（半角空白区切り）。**最後が前工程**（6.7） |
| 残仕掛設備ｺｰｽ | `DL2 ANI L-3 TLV KEN` | これから通る設備。**最初が次の設備** |
| **設計_PIT炉番炉順** | （例は未確認） | **釜番** として表示（6.7） |
| BOX実績_良品重量 | `4049.5` | |
| 工場納期 / 約束納期 | `2026-10-30 00:00:00` | |

### 6.7 前工程・釜番（既定の列）

| 列 | 作り方 | 既定の位置 |
|---|---|---|
| 前工程 | 計算列 `split([実績_設備ｺｰｽ]," ",-1)`。半角空白で区切った最後の設備 | 「パス」の右 |
| 釜番 | 台帳の列 `設計_PIT炉番炉順` を表示名「釜番」で表示 | 「前工程」の右 |

例：`実績_設備ｺｰｽ` が `HOT L-3 L-3 L-3` → 前工程 `L-3`、`HOT` → 前工程 `HOT`。

- 以前の版で保存した列の設定（`view_settings.json`）にも、初回起動時に1回だけこの2列を追加する（既にあれば変えない）
- 2026-10-19 の実ファイル（6,881件時点）では `設計_PIT炉番炉順` 列を確認できていない。台帳に列が無い場合、釜番は空欄になる

---

## 7. 呼び出しサンプル

### 7.1 Python：CM3取得（標準ライブラリのみ）

```python
import json
import urllib.request

URL = "http://nlmfangyweb1a:9085/Cm3SchWeb/service/schapp/cmschview/init"


def fetch_cm3(setsubi, term_kbn="4", timeout=15):
    payload = {
        "ip": "150.150.107.35", "userID": None, "termID": None,
        "setsubiName": setsubi, "useKbn": None, "termKbn": term_kbn,
        "authInfo": None, "result": 0, "error": False, "errorMessage": "",
        "version": None, "setsubi": "", "updateTs": None, "meisaiList": [],
        "urlSetsubi": setsubi, "urlTermKbn": term_kbn,
    }
    req = urllib.request.Request(
        URL, data=json.dumps(payload).encode("utf-8"), method="POST",
        headers={"Content-Type": "application/json;charset=UTF-8"},
    )
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))  # プロキシを通さない
    with opener.open(req, timeout=timeout) as res:
        data = json.loads(res.read().decode("utf-8"))
    entity = data.get("entity") or {}
    if data.get("status") != "SUCCESS" or entity.get("error"):
        raise RuntimeError(entity.get("errorMessage") or "CM3取得エラー")
    return entity
```

### 7.2 Python：台帳を手元にコピーして結合し、前工程を出す

```python
import os, shutil, sqlite3, pathlib, unicodedata, re

SRC = r"\\Nlmsrvngy03\Read\【New】仕掛\台帳\LOTNOW.sqlite3"
DST = r"C:\Tools\data\LOTNOW.sqlite3"
KEYS = [("lotNo", "ﾛｯﾄ番号")]          # (スケジュールの項目, 台帳の列)。複数指定でAND


def norm(v):
    s = re.sub(r"\s", "", unicodedata.normalize("NFKC", str(v or "")).upper())
    if re.fullmatch(r"[+-]?\d+(\.\d+)?", s):
        s = ("%.6f" % float(s)).rstrip("0").rstrip(".")
    return s


def copy_master():
    if os.path.exists(SRC + "-journal"):              # 更新中はコピーしない
        return
    os.makedirs(os.path.dirname(DST), exist_ok=True)
    shutil.copyfile(SRC, DST + ".tmp")                # SQLiteで開かずにファイルコピー
    os.replace(DST + ".tmp", DST)


def load_master():
    con = sqlite3.connect(pathlib.Path(DST).as_uri() + "?mode=ro", uri=True)
    cur = con.execute('SELECT * FROM "仕掛"')
    cols = [d[0] for d in cur.description]
    idx = [cols.index(m) for _, m in KEYS]
    rows = {}
    for r in cur:
        k = tuple(norm(r[i]) for i in idx)
        if all(k):
            rows.setdefault(k, dict(zip(cols, r)))
    con.close()
    return rows


copy_master()
master = load_master()
for m in fetch_cm3("L-3")["meisaiList"]:   # 点検行・設備停止行の除外は 5.3 の判定を使う
    t = master.get(tuple(norm(m.get(s)) for s, _ in KEYS))
    hist = (t or {}).get("実績_設備ｺｰｽ", "").split()
    print(m.get("lotNo"), "前工程", hist[-1] if hist else "-", "釜番", (t or {}).get("設計_PIT炉番炉順", ""))
```

### 7.3 PowerShell：CM3取得

```powershell
$url  = "http://nlmfangyweb1a:9085/Cm3SchWeb/service/schapp/cmschview/init"
$s    = "L-2"
$body = @{
  ip="150.150.107.35"; userID=$null; termID=$null; setsubiName=$s; useKbn=$null
  termKbn="4"; authInfo=$null; result=0; error=$false; errorMessage=""
  version=$null; setsubi=""; updateTs=$null; meisaiList=@()
  urlSetsubi=$s; urlTermKbn="4"
} | ConvertTo-Json
$res = Invoke-RestMethod -Uri $url -Method Post -Body ([Text.Encoding]::UTF8.GetBytes($body)) `
       -ContentType "application/json;charset=UTF-8"
$res.entity.meisaiList | Select-Object kanryoYotei, lotNo, zaishitsu, jiKotei | Format-Table
```

### 7.4 Excel VBA：CM3取得

```vba
Sub GetCM3()
    Dim http As Object, body As String, s As String
    s = "L-2"
    body = "{""ip"":""150.150.107.35"",""userID"":null,""termID"":null," & _
           """setsubiName"":""" & s & """,""useKbn"":null,""termKbn"":""4""," & _
           """authInfo"":null,""result"":0,""error"":false,""errorMessage"":""""," & _
           """version"":null,""setsubi"":"""",""updateTs"":null,""meisaiList"":[]," & _
           """urlSetsubi"":""" & s & """,""urlTermKbn"":""4""}"
    Set http = CreateObject("MSXML2.ServerXMLHTTP.6.0")
    http.Open "POST", "http://nlmfangyweb1a:9085/Cm3SchWeb/service/schapp/cmschview/init", False
    http.setRequestHeader "Content-Type", "application/json;charset=UTF-8"
    http.send body
    Debug.Print Left(http.responseText, 500)   ' JSON解析は VBA-JSON 等を使用
End Sub
```

### 7.5 ブラウザのJavaScriptから呼ぶ場合

別サーバーの自作ページから `fetch` で直接呼ぶと CORS 制限で失敗する可能性が高い。**自作サーバー（Flask等）経由** にする。台帳も同様に、ブラウザから共有フォルダは読めないためサーバー側で処理する。

---

## 8. エラーと対処

| 症状 | 主な原因 | 対処 |
|---|---|---|
| CM3に接続できない | 社外、プロキシ経由 | 社内LANで、プロキシを通さず実行 |
| HTTP 404 | URL表記 | `cmschview//init` で試す |
| HTTP 415 / 400 | Content-Type不足、JSON誤り | 4.3 のJSONをそのまま使う |
| `meisaiList` が空 | 設備コード・termKbn誤り | CM3画面URLの値と比較 |
| 台帳「元ファイルが見つかりません」 | パス誤り、共有に未接続 | エクスプローラーで同じパスを開けるか確認 |
| 台帳「更新中のため前回のコピーを使用」 | 更新処理の最中 | 自動で次回（60秒後）再取得 |
| 台帳一致が少ない | キーの選び違い | 「台帳設定」→「一致を確認」で件数を見ながら調整 |
| 釜番が空欄 | 台帳に `設計_PIT炉番炉順` 列が無い、または列名が違う | `config.json` の `hot.kama_column` を実際の列名に合わせる |
| HOTコイル作業順が0件 | 台帳に `実績_設備ｺｰｽ` が無い／該当ロットなし | 画面上の「抽出条件」欄の注記を確認 |

---

## 9. 利用時のルール

- CM3への取得間隔は30秒以上（HOTコイル作業順は3設備をまとめて取得するが、各設備10秒のキャッシュを共用する）
- 共有の台帳は **SQLiteで直接開かず、手元コピーを参照** する（6.2）
- `init` 以外のCM3 APIを推測で呼ばない
- URL・パス・項目名は設定ファイルにまとめる

---

## 10. 未確認事項

| No | 内容 | 確認方法 |
|---|---|---|
| 1 | `ip` を省略・変更しても結果が変わらないか | ip を null にして比較 |
| 2 | `rowStyle` の白・緑の意味 | CM3画面と照合 |
| 3 | Cookie・セッションが必要になる条件 | CM3未使用のPCで取得 |
| 4 | 台帳の更新頻度・更新方法 | 台帳の管理者に確認 |
| 5 | 台帳の各区分コード（保留_区分・特急区分など）の意味 | 台帳の管理者に確認 |
| 6 | `点検板` 以外に点検扱いにすべき名称があるか | 現場で確認し「配色」画面に追加 |
| 7 | `設計_PIT炉番炉順` の列名・値の形式 | 最新の台帳で列を確認（列名が違えば `config.json` で変更） |
| 8 | 開始予定（前の行の完了予定）の妥当性 | 実績の開始時刻と比較 |

---

## 付録A. 実装例：CM3 Schedule Viewer

### 構成

```
cm3_schedule_viewer/
├─ start.bat                 起動（ダブルクリック）
└─ app/
   ├─ app.py                 Flask本体（HOTコイル作業順の抽出を含む）
   ├─ cm3_client.py          CM3取得と加工（5章）
   ├─ master.py              台帳のコピー・キー判定・結合（6章）
   ├─ xlsx_writer.py         Excelファイルの作成（標準ライブラリのみ。B.7）
   ├─ config.json            接続先・設備・台帳パス・HOTコイル作業順の設定
   ├─ master_settings.json   突合せキー（「台帳設定」で保存）
   ├─ view_settings.json     列の設定（「列の設定」で保存。無ければ既定）
   ├─ ui_settings.json       配色・点検キーワード（「配色」で保存）
   ├─ templates/index.html   画面
   ├─ static/expr.js         計算式の解釈（B.4）
   ├─ static/view.js         列の値の作り方（B.2）
   ├─ static/designer.js     「列の設定」画面
   ├─ static/app.js, style.css
   └─ data/                  台帳の手元コピー（初回起動時に自動作成）
```

動作条件：Python 3.9以上と Flask がインストール済みで、`python` コマンドが使えるPC。

### config.json（台帳・HOTコイル作業順）

```json
"master": {
  "enabled": true,
  "source_path": "\\\\Nlmsrvngy03\\Read\\【New】仕掛\\台帳\\LOTNOW.sqlite3",
  "local_copy": "data/LOTNOW.sqlite3",
  "check_interval_sec": 60
},
"hot": {
  "title": "HOTコイル作業順",
  "equipments": ["L-1", "L-2", "L-3"],
  "prev_value": "HOT",
  "result_course_column": "実績_設備ｺｰｽ",
  "remain_course_column": "残仕掛設備ｺｰｽ",
  "kama_column": "設計_PIT炉番炉順"
}
```

列名は半角カナ・全角のどちらで書いてもよい（表記ゆれを吸収して照合）。

### Flaskが提供するAPI

| メソッド | パス | 内容 |
|---|---|---|
| GET | `/api/cm3/schedule?setsubi=L-2` | 加工済みJSON＋台帳列（`force=1` で再取得） |
| GET | `/api/hot` | HOTコイル作業順（付録C）（`force=1` で再取得） |
| GET | `/api/cm3/schedule.csv?setsubi=L-2` | 元の値のCSV（読み替え・書式の前。他ツール向け） |
| GET | `/api/cm3/raw?setsubi=L-2` | CM3の応答そのまま（確認用） |
| POST | `/api/export/xlsx` | 画面が送った表示どおりの内容からExcelファイルを作る（B.7） |
| GET | `/api/master` | 台帳の状態、列一覧、ロット番号列の候補 |
| POST | `/api/master/reload` | 台帳を今すぐ再コピー・再読込 |
| POST | `/api/master/test` | キーの組を保存せずに試す |
| POST | `/api/master/settings` | キーを保存して反映 |
| GET / POST | `/api/ui/settings` | 配色・点検キーワード |
| GET / POST | `/api/view/settings` | 列の設定（付録B） |
| GET | `/api/view/default` | 列の設定の既定値 |
| GET | `/api/view/sample?setsubi=L-2` | 台帳の全列を付けた実データ（列の設定画面用） |

加工済みJSONの1件：

| 項目 | 内容 |
|---|---|
| seq | 配列順 |
| setsubi | 設備 |
| rowKind | `lot`（製造）／ `inspect`（点検）／ `stop`（設備停止） |
| kanryoYotei / kanryoYoteiIso | 完了予定（5.1） |
| startYotei / startYoteiIso | 開始予定（推定）（5.1） |
| passNo / passTotal | 同じロットの何回目か / 全回数 |
| reductionRate | 圧下率 |
| stopText | 停止内容 |
| mHit | 台帳に該当あり true / なし false / 照合対象外 null |
| m | 台帳の値（列名→値）。画面が必要とする列だけ |

---

## 付録B. 表示のカスタマイズ（列の設定）

画面の「列の設定」で一覧の見せ方を変えられる。設定は `view_settings.json` に保存され、全設備・HOTコイル作業順に共通。

### B.1 画面の構成

| 場所 | できること |
|---|---|
| 左：列の一覧 | 表示／非表示、並び替え（⠿ をドラッグ）、表示名の変更、実データ1件の見え方、絞り込み |
| 左上 | 「＋ 列を作る」（計算列）、「幅を内容に合わせる」 |
| 左：帯 | 変換表の一覧、色分けルールの設定 |
| 右：列の詳細 | 1 見せ方／式（計算列のみ）／2 値の整え方／3 読み替え／結果 |
| 下 | 設定ファイル（書き出し・読み込み）、既定に戻す、キャンセル、保存 |

編集中の内容は後ろの一覧にすぐ反映される。「保存」で確定、「キャンセル」で元に戻る。

### B.2 値ができるまで

```
元の値（スケジュールの項目／台帳の列／計算式の答え）
   ↓ 3 読み替え（変換表）
読み替え後の値   ← 並べ替え・色分けルールはこの値で判定
   ↓ 2 値の整え方（数値・日付・文字の書式、前後に付ける文字）
表示
```

### B.3 列の設定項目

| 区分 | 項目 | 内容 |
|---|---|---|
| 1 見せ方 | 表示名・表示・幅・寄せ・太字・順番 | 幅は自動または 30〜800px |
| 2 値の整え方 | 数値 | 小数の桁、3桁区切り、0 のとき空欄 |
| | 日付 | `yyyy/MM/dd` `MM/dd` `MM/dd HH:mm` など7種類 |
| | 文字 | 前後の空白を除く、半角カナを全角に、最大文字数 |
| | 前後に付ける | 例 後に ` kg` |
| 3 読み替え | 変換表 | 値を別の表示に置き換える |

### B.4 計算列（式）

| 書き方 | 意味 | 例 |
|---|---|---|
| `[列名]` | 列の値 | `[実績_設備ｺｰｽ]` |
| `"文字"` | 文字 | `"完了"` |
| `+ - * /` | 計算 | `[入側板厚]-[出側板厚]` |
| `&` | 文字をつなぐ | `[材質] & "-" & [形状]` |
| `= <> < <= > >=` | 比べる | `[特急区分]="2"` |

関数：`if` `and` `or` `not` `in` `isblank` `coalesce` `split`（番号 -1 で最後） `count` `left` `right` `mid` `len` `trim` `upper` `lower` `zen` `replace` `contains` `startswith` `endswith` `concat` `num` `round` `abs` `min` `max` `text` `today` `days` `datefmt`

| 列の名前 | 式 |
|---|---|
| 前工程（既定） | `split([実績_設備ｺｰｽ]," ",-1)` |
| 次の設備（既定・非表示） | `if(split([残仕掛設備ｺｰｽ]," ",1)="","完了",split([残仕掛設備ｺｰｽ]," ",1))` |
| 圧下量 | `[入側板厚]-[出側板厚]` |
| 納期まで | `days(today(),[工場納期])` |

### B.5 読み替え（変換表）

元の値 → 表示 の表。表に無い値は「そのまま／空欄／指定の文字」。「実データから値を集める」で値を一括追加。1つの変換表を複数の列で使える。

### B.6 色分けルール

条件（列の値で／式で）に合う製造行（または指定した列）に背景色・文字色・太字を付ける。下のルールほど優先。

### B.7 CSV出力・Excel出力

どちらも **今の表示のまま** 出力する。HOTコイル作業順でも同様（ファイル名 `cm3_HOT_yyyymmdd_hhmm`、シート名「HOTコイル作業順」）。

| 項目 | CSV | Excel（.xlsx） |
|---|---|---|
| 値 | 画面の表示文字 | 数値は数値（画面と同じ小数桁の表示形式）。単位付きの値やコード類は文字 |
| 色 | なし | 行の背景（通常・縞・点検・設備停止）、色分けルール、設備の色（HOT一覧）、釜番の色（HOT一覧） |
| 設備停止行 | 停止内容は最後の列 | 横に結合したセル |
| その他 | UTF-8（BOM付き） | 見出し固定、オートフィルター、A4横印刷、フッターに取得日時 |

### B.8 設定ファイル（view_settings.json）

| 項目 | 内容 |
|---|---|
| version | 2（版が古い設定は起動時に前工程・釜番の列を追加して 2 にする） |
| columns[].id | `s:項目` / `m:台帳の列名` / `c:番号`（並び順＝表示順） |
| columns[].source | `schedule` / `master` / `calc` |
| columns[].format.type | `none` / `number` / `date` / `text` |
| columns[].map | 使う変換表の名前 |
| maps / rules | 変換表 / 色分けルール |

---

## 付録C. HOTコイル作業順

タブ「HOTコイル作業順」で、L-1・L-2・L-3 のうち **前工程が HOT（熱延）のロット** を1つの一覧にまとめ、作業予定の順に並べる。どの設備で、どの順番で、どの釜番のコイルが作業されるかを確認するための一覧。

### C.1 抽出条件

各設備（L-1・L-2・L-3）のスケジュールの製造行のうち、次をすべて満たすもの。

| No | 条件 | 理由 |
|---|---|---|
| 1 | 台帳の `実績_設備ｺｰｽ` を半角空白で区切った **最後** が `HOT` | 前工程が熱延 |
| 2 | 台帳の `残仕掛設備ｺｰｽ` の **最初** がその設備（空欄なら条件なし） | 同じロットが複数の設備のスケジュールに載ることがあるため、HOTの直後に作業する設備だけを残す |
| 3 | その設備での **1パス目** | 2パス目以降の前工程はその設備自身 |

台帳に該当しないロットは前工程が分からないため除外し、件数を「抽出条件」欄に表示する。

### C.2 並び順

完了予定の早い順（同じ時刻は L-1 → L-2 → L-3）。列見出しを押せば並べ替えられる。

### C.3 画面

| 場所 | 内容 |
|---|---|
| 件数カード | 対象ロット数、設備ごとの本数と時間帯（押すとその設備の表示／非表示）、釜番の数、抽出条件と除外件数 |
| 作業の流れ（図） | 設備ごとの横帯。帯の位置と長さは「開始予定（推定）～完了予定」、帯の色と文字は釜番。現在時刻に赤線。帯を押すとその釜番だけを表示（他は薄く） |
| 釜番の一覧 | 作業が早い順に、釜番・本数・設備の内訳（例 `L-3×2`）。押すとその釜番に絞り込み（複数選択可）。マウスを乗せると時間帯 |
| 一覧 | 先頭に 順・設備・開始予定(推定)・完了予定、続けて「列の設定」で表示中の列。設備は色付きの印、釜番は色の印付き |

設備の色：L-1 青、L-2 緑、L-3 オレンジ。釜番の色は作業が早い順に16色を割り当てる。

### C.4 応答（/api/hot）

一覧の各行は付録Aの加工済みJSONに次を加えたもの。

| 項目 | 内容 |
|---|---|
| hotSeq | 一覧での順番 |
| prevProcess | 前工程（`HOT`） |
| kama | 釜番（`設計_PIT炉番炉順` の値） |

`hot` 項目：`equipments`、`prevValue`、`resultColumn` / `remainColumn` / `kamaColumn`（実際に使った台帳の列名。無ければ空）、`perEquipment`（設備ごとの本数）、`counts`（`noMaster` 台帳に無い、`otherNext` 次の設備が別、`laterPass` 2パス目以降）、`errors`（取得できなかった設備）。

---

## 改訂履歴

| 版 | 日付 | 内容 |
|---|---|---|
| 2.8 | 2026-10-19 | 既定の列に「前工程」（実績_設備ｺｰｽの最後）と「釜番」（設計_PIT炉番炉順）を追加（6.7）。開始予定（推定）を追加（5.1）。「HOTコイル作業順」タブを追加（付録C） |
| 2.7 | 2026-10-19 | Excel出力を追加。台帳から結合した列の専用色をやめ、他の列と同じ見た目で結合 |
| 2.6 | 2026-10-19 | 表示のカスタマイズ（付録B）：列の並び替え・計算列・読み替え・書式・色分けルール |
| 2.5 | 2026-10-19 | 点検行（点検板）の区分。突合せキーの複数組対応。配色設定 |
| 2.4 | 2026-10-19 | 仕掛台帳（LOTNOW.sqlite3）との結合 |
| 2.3 | 2026-10-19 | 停止内容を応答JSONの項目順・空白なしで連結 |
| 2.2 | 2026-10-19 | 設備停止行を行の情報量で判定 |
| 2.1 | 2026-10-19 | 設備停止行、文字化け対策、raw API |
| 2.0 | 2026-10-19 | 汎用仕様書に再構成 |
| 1.1 | 2026-10-19 | L-1・L-3切替 |
| 1.0 | 2026-10-19 | 初版 |
