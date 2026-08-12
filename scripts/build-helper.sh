#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG="$ROOT/sidecar/FennecHelper"
DEST_DIR="$ROOT/src-tauri/binaries"
DEST="$DEST_DIR/fennec-helper-aarch64-apple-darwin"

swift build -c release --package-path "$PKG" --arch arm64
BIN="$(swift build -c release --package-path "$PKG" --arch arm64 --show-bin-path)/fennec-helper"

mkdir -p "$DEST_DIR"
cp "$BIN" "$DEST"
codesign --force --sign - "$DEST" >/dev/null 2>&1 || true
echo "built $DEST"
