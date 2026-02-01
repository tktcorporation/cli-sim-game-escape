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

# Install Trunk
cargo install trunk --version "${TRUNK_VERSION:-0.21.14}" --locked

# Build
trunk build --release
