#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(python3 -c "import json; print(json.load(open('src-tauri/tauri.conf.json'))['version'])")
OUT="dist-release"
TAG="${1:-}"

pnpm install --frozen-lockfile
pnpm tauri build

mkdir -p "$OUT"
rm -f "$OUT"/*

case "$(uname -s)" in
Darwin)
    DMG=$(ls target/release/bundle/dmg/*.dmg | head -1)
    APP=target/release/bundle/macos/Fennec.app
    cp "$DMG" "$OUT/Fennec_${VERSION}_$(uname -m).dmg"
    ditto -c -k --sequesterRsrc --keepParent "$APP" "$OUT/Fennec_${VERSION}_$(uname -m).zip"
    ;;
Linux)
    cp target/release/bundle/deb/*.deb "$OUT/" 2>/dev/null || true
    cp target/release/bundle/appimage/*.AppImage "$OUT/" 2>/dev/null || true
    cp target/release/bundle/rpm/*.rpm "$OUT/" 2>/dev/null || true
    ;;
*)
    cp target/release/bundle/nsis/*.exe "$OUT/" 2>/dev/null || true
    cp target/release/bundle/msi/*.msi "$OUT/" 2>/dev/null || true
    ;;
esac

(cd "$OUT" && shasum -a 256 * > SHA256SUMS.txt)
echo "release artifacts:"
ls -la "$OUT"

if [ -n "$TAG" ]; then
    gh release upload "$TAG" "$OUT"/* --clobber
    echo "uploaded to release $TAG"
fi
