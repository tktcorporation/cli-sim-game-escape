#!/usr/bin/env bash
set -euo pipefail

# Install Rust (if not present)
if ! command -v rustup &> /dev/null; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  source "$CARGO_HOME/env"
fi

# Ensure a default toolchain is installed
rustup default stable

# Add WASM target
rustup target add wasm32-unknown-unknown

# Install Trunk from pre-built binary (much faster than cargo install)
TRUNK_VERSION="${TRUNK_VERSION:-0.21.14}"
TRUNK_BIN="$CARGO_HOME/bin/trunk"
if [ -x "$TRUNK_BIN" ] && "$TRUNK_BIN" --version 2>/dev/null | grep -q "$TRUNK_VERSION"; then
  echo "Trunk $TRUNK_VERSION already installed, skipping download"
else
  echo "Installing Trunk $TRUNK_VERSION from pre-built binary..."
  TRUNK_URL="https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}/trunk-x86_64-unknown-linux-gnu.tar.gz"
  curl -sSfL "$TRUNK_URL" | tar xzf - -C "$CARGO_HOME/bin"
  chmod +x "$TRUNK_BIN"
fi

# Build
trunk build --release
