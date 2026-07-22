#!/usr/bin/env bash
# Bootstraps a Rust toolchain (if needed) and builds GraphSwarm on Mac/Linux.
# Safe to re-run: every step checks whether it already succeeded before acting.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$REPO_ROOT"

echo "==> Checking for Rust toolchain..."
if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then
  echo "cargo/rustc not found. Installing Rust via rustup (non-interactive)..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
else
  echo "Found cargo: $(command -v cargo)"
fi

# Make sure this shell can see a freshly installed toolchain even if PATH
# hasn't been reloaded yet.
if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

if ! command -v rustup >/dev/null 2>&1; then
  echo "ERROR: rustup still not found on PATH after install." >&2
  echo "        Open a new terminal (so PATH picks up ~/.cargo/bin) and re-run this script." >&2
  exit 1
fi

echo "==> Active toolchain:"
rustup show

echo "==> Building graphswarm (release)..."
cargo build --release

BIN_PATH="$REPO_ROOT/target/release/graphswarm"
if [ ! -f "$BIN_PATH" ]; then
  echo "ERROR: build did not produce $BIN_PATH" >&2
  exit 1
fi

echo "==> Binary built at: $BIN_PATH"
echo "==> Verifying binary runs:"
"$BIN_PATH" --version

echo "==> Setup complete."
