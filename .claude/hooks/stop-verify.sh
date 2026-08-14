#!/usr/bin/env bash
# Stop framework hook: 完了宣言の前に project の mise task に委譲して
# 全体検証（lint/test/型チェック等）を実行し、違反があればエージェントに差し戻す。
#
# 責務分離:
#   - このスクリプト（汎用、ziku 同期対象）: mise 委譲 → JSON 整形
#   - project の `.mise.toml` の `[tasks.claude-verify]`（プロジェクト固有）: 実コマンド
#
# 委譲先タスク仕様:
#   mise run claude-verify
#   - 違反なし: exit 0、stdout 空
#   - 違反あり: exit 非ゼロ、stdout に全違反まとめ
#
# mise / task 不在ならサイレントに no-op。
set -euo pipefail

cd "${CLAUDE_PROJECT_DIR:-.}"

command -v mise >/dev/null 2>&1 || exit 0
mise tasks ls --no-header 2>/dev/null | awk '{print $1}' | grep -qx 'claude-verify' || exit 0

# 外側 timeout は mise 機構自体の万一のハングに対する最終防御（個々のチェックは
# task 内 chk() でも timeout 済み）。Stop hook が完了をフリーズさせないことを保証する。
# timeout は GNU coreutils のため macOS host には無い（gtimeout があれば使い、無ければ素で実行）。
if command -v timeout >/dev/null 2>&1; then
  guard=(timeout -k 5 300)
elif command -v gtimeout >/dev/null 2>&1; then
  guard=(gtimeout -k 5 300)
else
  guard=()
fi

# timeout は対象プロセスがハングした場合 124（gtimeout の -k 経由 SIGKILL なら 137）を
# 出力なしで返す。空出力を「違反なし」と誤判定すると検証未完了のまま完了扱いになるため、
# 終了コードを明示的に見て区別する（set -e 下で終了コードを失わないよう一時的に無効化）。
set +e
errors="$("${guard[@]+"${guard[@]}"}" mise run --quiet claude-verify 2>&1)"
status=$?
set -e
[[ "$status" -eq 0 ]] && exit 0

if [[ "$status" -eq 124 || "$status" -eq 137 ]]; then
  errors="claude-verify がタイムアウトしました（300秒超過、exit ${status}）。検証が完了していません。"
fi

# mise が失敗時に "[task-name] ERROR task failed" を末尾に追記するので除去
errors="$(echo "$errors" | grep -vE '^\[claude-verify\] ERROR task failed$' || true)"
[[ -z "$errors" ]] && exit 0

# Stop hook では top-level の decision/reason を使う（hookSpecificOutput はスキーマ非対応）。
# decision: "block" で停止を阻止し、reason をシステムメッセージとしてエージェントに注入。
jq -Rn --arg msg "$errors" '{
  decision: "block",
  reason: ("🛑 Stop hook: 以下の問題が未解決です。修正してから完了してください:\n\n" + $msg)
}'
