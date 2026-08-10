#!/usr/bin/env bash
# Cloud Agent 用の環境セットアップ。冪等に書く（build スナップショット生成時に
# 一度走り、その後も再実行され得る）。ローカルの devcontainer / netlify build.sh
# とは別に、Cursor Cloud Agent の VM を対象にした最小構成。
set -euo pipefail

CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"
mkdir -p "$CARGO_BIN"

# ── Rust toolchain ─────────────────────────────────────────
# 推移的依存 (beamterm-renderer 0.10) が Cargo の `edition2024` feature を要求する
# ため stable >= 1.85 が必須。ベースイメージが古い stable を default にしている
# ことがあるので、明示的に更新して wasm ターゲットと clippy を揃える。
rustup update stable
rustup default stable
rustup component add clippy
rustup target add wasm32-unknown-unknown

# ── Trunk (WASM ビルドシステム) ────────────────────────────
# netlify build.sh / CI と同じ 0.21.14 に固定。prebuilt バイナリを直接展開する
# (cargo install より速く、trunk の推移的依存の非互換更新にも影響されない)。
TRUNK_VERSION="0.21.14"
if [ -x "$CARGO_BIN/trunk" ] && "$CARGO_BIN/trunk" --version 2>/dev/null | grep -q "$TRUNK_VERSION"; then
  echo "trunk $TRUNK_VERSION already installed"
else
  echo "installing trunk $TRUNK_VERSION"
  curl -sSfL "https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}/trunk-x86_64-unknown-linux-gnu.tar.gz" \
    | tar xzf - -C "$CARGO_BIN"
  chmod +x "$CARGO_BIN/trunk"
fi

# ── cargo-nextest (CI の `--profile ci` が使うテストランナー) ─
if [ -x "$CARGO_BIN/cargo-nextest" ]; then
  echo "cargo-nextest already installed"
else
  echo "installing cargo-nextest"
  curl -sSfL https://get.nexte.st/latest/linux | tar zxf - -C "$CARGO_BIN"
fi

# ── Node devtool (metropolis AI worker の Playwright E2E) ───
npm install
# chromium 本体 + 必要 OS ライブラリ (libnss3 等) を一括導入。
npx playwright install --with-deps chromium

# ── WASM ビルドを一度通してキャッシュを温める ──────────────
# NO_COLOR=true は Cloud Agent VM が設定する NO_COLOR=1 を無害化する。
# trunk 0.21 の --no-color フラグは bool を期待し "1" を弾くため、そのままだと
# `trunk build`/`trunk serve` が起動時に失敗する。
NO_COLOR=true trunk build
