#!/usr/bin/env bash
#
# Claude Code on the web 用のセッション開始フック。
# Rust + WASM (ratzilla / trunk) ビルドができる状態に環境を整える。
#
# やること:
#   1. wasm32-unknown-unknown ターゲットを追加 (idempotent)
#   2. trunk を pre-built binary でインストール (既にあれば skip)
#
# ローカル環境では何もしない (CLAUDE_CODE_REMOTE != true)。
# 既にツール類が入っている開発機を汚染しないため。

set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

CARGO_HOME_DIR="${CARGO_HOME:-$HOME/.cargo}"
CARGO_BIN_DIR="$CARGO_HOME_DIR/bin"
mkdir -p "$CARGO_BIN_DIR"

# 後続の Claude セッションコマンドからも cargo / trunk が見えるよう、
# $CLAUDE_ENV_FILE に PATH 追記する (skill ドキュメント推奨パターン)。
if [ -n "${CLAUDE_ENV_FILE:-}" ]; then
  echo "export PATH=\"$CARGO_BIN_DIR:\$PATH\"" >> "$CLAUDE_ENV_FILE"
fi
export PATH="$CARGO_BIN_DIR:$PATH"

# ── rustup (初回コンテナで未インストールの場合の保険) ─────
if ! command -v rustup >/dev/null 2>&1; then
  echo "▶ Installing rustup (stable toolchain)..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain stable --no-modify-path
  # shellcheck disable=SC1091
  . "$CARGO_HOME_DIR/env"
fi

# ── wasm32 ターゲット ──────────────────────────────────────
# 既に入っていても rustup target add は no-op で抜ける (idempotent)。
echo "▶ Ensuring wasm32-unknown-unknown target..."
rustup target add wasm32-unknown-unknown

# ── trunk (build.sh と揃える) ─────────────────────────────
TRUNK_VERSION="${TRUNK_VERSION:-0.21.14}"
TRUNK_BIN="$CARGO_BIN_DIR/trunk"

if [ -x "$TRUNK_BIN" ] && "$TRUNK_BIN" --version 2>/dev/null | grep -q "$TRUNK_VERSION"; then
  echo "✓ Trunk $TRUNK_VERSION already installed"
else
  echo "▶ Installing Trunk $TRUNK_VERSION..."
  TRUNK_URL="https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}/trunk-x86_64-unknown-linux-gnu.tar.gz"
  curl -sSfL "$TRUNK_URL" | tar xzf - -C "$CARGO_BIN_DIR"
  chmod +x "$TRUNK_BIN"
  "$TRUNK_BIN" --version
fi

echo "✓ session-start hook complete"
