#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(node -p "require('./src-tauri/tauri.conf.json').version")
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
    SUMS="SHA256SUMS-macos.txt"
    ;;
Linux)
    cp target/release/bundle/deb/*.deb "$OUT/" 2>/dev/null || true
    cp target/release/bundle/appimage/*.AppImage "$OUT/" 2>/dev/null || true
    cp target/release/bundle/rpm/*.rpm "$OUT/" 2>/dev/null || true
    SUMS="SHA256SUMS-linux.txt"
    ;;
*)
    cp target/release/bundle/nsis/*.exe "$OUT/" 2>/dev/null || true
    cp target/release/bundle/msi/*.msi "$OUT/" 2>/dev/null || true
    SUMS="SHA256SUMS-windows.txt"
    ;;
esac

# OS ごとに分けないと、各マシンが --clobber でアップロードして最後の1つしか残らない
(cd "$OUT" && shasum -a 256 * > "$SUMS")
echo "release artifacts:"
ls -la "$OUT"

if [ -n "$TAG" ]; then
    gh release upload "$TAG" "$OUT"/* --clobber
    echo "uploaded to release $TAG"

    if [ "$(uname -s)" = "Darwin" ]; then
        ZIP_NAME="Fennec_${VERSION}_$(uname -m).zip"
        SHA256=$(shasum -a 256 "$OUT/$ZIP_NAME" | awk '{print $1}')
        gh api repos/krgpi/homebrew-tap/dispatches \
          -f event_type=update-cask \
          -f "client_payload[cask]=fennec" \
          -f "client_payload[version]=${VERSION}" \
          -f "client_payload[sha256]=${SHA256}"
        echo "dispatched Homebrew tap update"
    fi
fi
