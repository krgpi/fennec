#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

# 各OSのビルドマシンで古いコードのまま成果物を作らないよう、必ず最新を取り込んでから始める
git pull --ff-only

VERSION=$(node -p "require('./src-tauri/tauri.conf.json').version")
OUT="dist-release"
TAG="v${VERSION}"
SKIP_UPLOAD="${1:-}"

if [ -n "$SKIP_UPLOAD" ] && [ "$SKIP_UPLOAD" != "--no-upload" ]; then
    echo "usage: $(basename "$0") [--no-upload]   (tag is derived from tauri.conf.json: $TAG)" >&2
    exit 1
fi

# 配布物はCUDA Toolkitの無い環境でも動く必要がある。ビルドマシンにCUDAが入っていると
# build.rsが自動検出してcudart/cublasを動的リンクし、一般ユーザーの環境で起動できなくなる
if [ "$(uname -s)" != "Darwin" ] && [ "$(uname -s)" != "Linux" ]; then
    export FENNEC_NO_CUDA=1
fi

pnpm install --frozen-lockfile
pnpm tauri build

mkdir -p "$OUT"
rm -f "$OUT"/*

case "$(uname -s)" in
# bundle/ には過去のバージョンの成果物が残るため、必ず今回のVERSIONで絞る
Darwin)
    DMG=$(ls target/release/bundle/dmg/*_${VERSION}_*.dmg | head -1)
    APP=target/release/bundle/macos/Fennec.app
    cp "$DMG" "$OUT/Fennec_${VERSION}_$(uname -m).dmg"
    ditto -c -k --sequesterRsrc --keepParent "$APP" "$OUT/Fennec_${VERSION}_$(uname -m).zip"
    SUMS="SHA256SUMS-macos.txt"
    ;;
Linux)
    cp target/release/bundle/deb/*_${VERSION}_*.deb "$OUT/" 2>/dev/null || true
    cp target/release/bundle/appimage/*_${VERSION}_*.AppImage "$OUT/" 2>/dev/null || true
    cp target/release/bundle/rpm/*-${VERSION}-*.rpm "$OUT/" 2>/dev/null || true
    SUMS="SHA256SUMS-linux.txt"
    ;;
*)
    cp target/release/bundle/nsis/*_${VERSION}_*.exe "$OUT/" 2>/dev/null || true
    cp target/release/bundle/msi/*_${VERSION}_*.msi "$OUT/" 2>/dev/null || true
    SUMS="SHA256SUMS-windows.txt"
    ;;
esac

# OS ごとに分けないと、各マシンが --clobber でアップロードして最後の1つしか残らない
(cd "$OUT" && shasum -a 256 * > "$SUMS")
echo "release artifacts:"
ls -la "$OUT"

if [ "$SKIP_UPLOAD" != "--no-upload" ]; then
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
