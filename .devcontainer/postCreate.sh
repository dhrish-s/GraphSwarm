#!/usr/bin/env bash
# Runs once the dev container is created: builds and verifies the binary.
# Exits non-zero (surfacing a visible failure in the container log) if either step fails.
set -euo pipefail

echo "==> Active toolchain:"
rustup show

echo "==> Building graphswarm (release)..."
cargo build --release

BIN_PATH="target/release/graphswarm"
if [ ! -f "$BIN_PATH" ]; then
  echo "ERROR: build did not produce $BIN_PATH" >&2
  exit 1
fi

echo "==> Binary built at: $(pwd)/$BIN_PATH"
echo "==> Verifying binary runs:"
"$BIN_PATH" --version

echo "==> Dev container setup complete."
