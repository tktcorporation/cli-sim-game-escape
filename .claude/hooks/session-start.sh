#!/usr/bin/env bash
# Claude Code on the web のセッション開始時に Rust + WASM ビルド環境を整える。
#
# - wasm32-unknown-unknown ターゲット (trunk build に必須)
# - trunk バイナリ (build.sh と同バージョンを pin)
#
# ローカル開発では何もしない (CLAUDE_CODE_REMOTE が "true" の時だけ実行)。
# 既にインストール済みなら skip するので safe to re-run。
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
PATH="$CARGO_HOME/bin:$PATH"

echo "🦀 Setting up Rust + WASM build environment..."

# ── rustup / wasm target ───────────────────────────────────
if ! command -v rustup >/dev/null 2>&1; then
  echo "rustup not found — installing via rustup-init..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
  # shellcheck disable=SC1091
  . "$CARGO_HOME/env"
fi

# `rustup target add` は既にあれば no-op。
rustup target add wasm32-unknown-unknown

# ── trunk (pre-built binary) ────────────────────────────────
# build.sh と同バージョンを使う。version 不一致や上書きを避けるため、
# 既存バイナリの `--version` 出力を grep で確認してから download する。
TRUNK_VERSION="${TRUNK_VERSION:-0.21.14}"
TRUNK_BIN="$CARGO_HOME/bin/trunk"

if [ -x "$TRUNK_BIN" ] && "$TRUNK_BIN" --version 2>/dev/null | grep -q "$TRUNK_VERSION"; then
  echo "✅ trunk $TRUNK_VERSION already installed"
else
  echo "⬇  Installing trunk $TRUNK_VERSION from pre-built binary..."
  TRUNK_URL="https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}/trunk-x86_64-unknown-linux-gnu.tar.gz"
  mkdir -p "$CARGO_HOME/bin"
  curl -sSfL "$TRUNK_URL" | tar xzf - -C "$CARGO_HOME/bin"
  chmod +x "$TRUNK_BIN"
fi

echo "✅ Environment ready: $(rustc --version), $("$TRUNK_BIN" --version)"
