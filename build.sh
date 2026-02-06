#!/usr/bin/env bash
set -euo pipefail

NETLIFY_CACHE="/opt/build/cache"

# ── Restore cache ──────────────────────────────────────────
restore_cache() {
  local name=$1 src="$NETLIFY_CACHE/$1" dest=$2
  if [ -d "$src" ]; then
    echo "♻ Restoring $name cache..."
    mkdir -p "$dest"
    rsync -a "$src/" "$dest/"
  fi
}

restore_cache "cargo-registry" "$CARGO_HOME/registry"
restore_cache "cargo-bin"      "$CARGO_HOME/bin"
restore_cache "target"         "target"

# ── Install Rust (if not present) ──────────────────────────
if ! command -v rustup &> /dev/null; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  source "$CARGO_HOME/env"
fi

rustup default stable
rustup target add wasm32-unknown-unknown

# ── Install Trunk from pre-built binary ────────────────────
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

# ── Build ──────────────────────────────────────────────────
trunk build --release

# ── Save cache ─────────────────────────────────────────────
save_cache() {
  local name=$1 src=$2 dest="$NETLIFY_CACHE/$1"
  if [ -d "$src" ]; then
    echo "💾 Saving $name cache..."
    mkdir -p "$dest"
    rsync -a "$src/" "$dest/"
  fi
}

save_cache "cargo-registry" "$CARGO_HOME/registry"
save_cache "cargo-bin"      "$CARGO_HOME/bin"
save_cache "target"         "target"
