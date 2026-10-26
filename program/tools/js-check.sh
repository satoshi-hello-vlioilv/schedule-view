#!/usr/bin/env bash
# JavaScript の構文の確かめ（CLAUDE.md の「push 前の確かめ」と CI が同じ物を流す）。
#   bash program/tools/js-check.sh
# 拡張子が .js でも import / export を使うファイルは、モジュールとして確かめる（node --check はそのままだと古い形の
# スクリプトとして読み、モジュールの中の書き方の誤りを見逃して終了コード 0 を返す）。.mjs はモジュールとして読まれる。
set -u
bad=0
while IFS= read -r f; do
  if [[ "$f" == *.js ]] && grep -qE '^(import|export) ' "$f"; then
    node --input-type=module --check < "$f" || { echo "構文の誤り: $f"; bad=1; }
  else
    node --check "$f" || { echo "構文の誤り: $f"; bad=1; }
  fi
done < <(find program/app/web program/tools desktop/src -name '*.js' -o -name '*.mjs')
# 画面（index.html）の中の <script> も確かめる（差し込み印 {{APP}} は空の物に置き換える）
tmp="$(mktemp -d)/index-script.js"
py=$(command -v python3 || command -v python)
"$py" - "$tmp" <<'PY'
import re, sys
s = open("program/app/web/index.html", encoding="utf-8").read().replace("{{APP}}", "{}")
open(sys.argv[1], "w", encoding="utf-8").write("\n".join(re.findall(r"<script>(.*?)</script>", s, re.S)))
PY
node --check "$tmp" || { echo "構文の誤り: program/app/web/index.html の <script>"; bad=1; }
rm -rf "$(dirname "$tmp")"
exit $bad
