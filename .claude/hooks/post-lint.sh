#!/usr/bin/env bash
# PostToolUse framework hook: Edit/Write 後に project の mise task に委譲して
# format + lint を実行し、違反は additionalContext として Claude に注入する。
#
# 責務分離:
#   - このスクリプト（汎用、ziku 同期対象）: ファイルパス抽出 → mise 委譲 → JSON 整形
#   - project の `.mise.toml` の `[tasks.claude-postedit]`（プロジェクト固有）: 実コマンド
#
# 委譲先タスク仕様:
#   mise run claude-postedit -- <file>
#   - 違反なし: exit 0、stdout 空
#   - 違反あり: exit 非ゼロ、stdout に診断メッセージ
#   - 対象外ファイル: exit 0
#
# mise / task 不在ならサイレントに no-op。
set -euo pipefail

cd "${CLAUDE_PROJECT_DIR:-.}"

# Claude Code hook はイベント JSON を stdin で渡す（環境変数ではない）。
# 同一イベントに複数 hook が登録されても各プロセスが独立した stdin を受け取るので先に読み切る。
input="$(cat)"

command -v mise >/dev/null 2>&1 || exit 0
mise tasks ls --no-header 2>/dev/null | awk '{print $1}' | grep -qx 'claude-postedit' || exit 0

[[ -z "$input" ]] && exit 0

# 編集ファイルパスは tool_input.file_path 配下にある。
file="$(echo "$input" | jq -r '.tool_input.file_path // .tool_input.path // empty' 2>/dev/null || true)"
[[ -z "$file" || ! -f "$file" ]] && exit 0

# 外側 timeout は mise 機構自体の万一のハングに対する最終防御（task 内でも timeout 済み）。
# PostToolUse hook が編集をフリーズさせないことを保証する。
# timeout は GNU coreutils のため macOS host には無い（gtimeout があれば使い、無ければ素で実行）。
if command -v timeout >/dev/null 2>&1; then
  guard=(timeout -k 5 120)
elif command -v gtimeout >/dev/null 2>&1; then
  guard=(gtimeout -k 5 120)
else
  guard=()
fi

# timeout は対象プロセスがハングした場合 124（gtimeout の -k 経由 SIGKILL なら 137）を
# 出力なしで返す。空出力を「違反なし」と誤判定すると診断未完了のまま通過扱いになるため、
# 終了コードを明示的に見て区別する（set -e 下で終了コードを失わないよう一時的に無効化）。
set +e
diag="$("${guard[@]+"${guard[@]}"}" mise run --quiet claude-postedit -- "$file" 2>&1)"
status=$?
set -e
[[ "$status" -eq 0 ]] && exit 0

if [[ "$status" -eq 124 || "$status" -eq 137 ]]; then
  diag="claude-postedit がタイムアウトしました（120秒超過、exit ${status}）。lint/format が完了していません。"
fi

# mise が失敗時に "[task-name] ERROR task failed" を末尾に追記するので除去
diag="$(echo "$diag" | grep -vE '^\[claude-postedit\] ERROR task failed$' || true)"
[[ -z "$diag" ]] && exit 0

jq -Rn --arg msg "$diag" '{
  hookSpecificOutput: {
    hookEventName: "PostToolUse",
    additionalContext: ("⚠ post-edit check failed:\n" + $msg + "\nFix these issues before proceeding.")
  }
}'
