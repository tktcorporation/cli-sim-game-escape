#!/usr/bin/env bash
# PostToolUse framework hook: 鮮度チェックの対象ディレクトリに新しい .md を作った
# 直後に、SSOT の判断と消費タイミングを Claude へ問い直す。
#
# なぜ lint ではなく hook か: 「その記述の SSOT はコードではないか」は正規表現で
# 判定できない。lint（pnpm lint:docs）は鮮度とリンク切れという機械的に判定できる
# 部分だけを担い、書く前の判断はここで促す。
#
# 発火条件は `.config/docs-lifecycle.json` の scan から導出するため、対象ディレクトリを
# このスクリプトに書かない（設定ファイルが SSOT）。既に追跡済みのファイルの編集では
# 発火しない（新規作成のときだけ問う）。設定が無いリポジトリでは no-op。
set -euo pipefail

cd "${CLAUDE_PROJECT_DIR:-.}"

config=".config/docs-lifecycle.json"
[[ -f "$config" ]] || exit 0
command -v jq >/dev/null 2>&1 || exit 0

input="$(cat)"
[[ -z "$input" ]] && exit 0

file="$(echo "$input" | jq -r '.tool_input.file_path // .tool_input.path // empty' 2>/dev/null || true)"
[[ -z "$file" || ! -f "$file" ]] && exit 0

# CLAUDE_PROJECT_DIR 起点の相対パスに正規化する（Write は絶対パスを渡してくる）
file="${file#"$PWD"/}"

case "$file" in
*.md | *.mdx) ;;
*) exit 0 ;;
esac

# scan の glob からワイルドカード以降を落とし、対象ルート（例: `docs/`）を導出する
matched=0
while IFS= read -r root; do
  [[ -z "$root" ]] && continue
  case "$file" in "$root"*) matched=1 ;; esac
done < <(jq -r '.scan[]' "$config" 2>/dev/null | sed 's#\*.*##' | sort -u)
[[ "$matched" -eq 1 ]] || exit 0

# 追跡済みファイルの編集は対象外。新規作成のときだけ問う。
git ls-files --error-unmatch "$file" >/dev/null 2>&1 && exit 0

jq -Rn --arg file "$file" '{
  hookSpecificOutput: {
    hookEventName: "PostToolUse",
    additionalContext: ("新しい doc を作成した: " + $file + "\n" +
      ".claude/rules/doc-placement.md に沿って次を確認すること:\n" +
      "1. この内容の SSOT はコード側ではないか（スキーマ表・カラム表・config 値の一覧・関数の挙動はコードの JSDoc が SSOT。.md からは 1 行で参照する）\n" +
      "2. .md を作らずに済まないか（1 PR で終わるなら PR description、段階実装なら issue、確定した設計判断ならコードコメント、作業計画なら .claude/plans/）\n" +
      "3. 使い捨ての plan / spec なら、どの PR で消費して削除するかを決めたか\n" +
      "4. 長期保持する WHY 集約 doc なら frontmatter に lifecycle: durable を宣言したか\n" +
      "鮮度チェックの閾値を超えると pnpm lint:docs が fail する。")
  }
}'
