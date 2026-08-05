#!/bin/bash
set -euo pipefail

TAG="${1:?Usage: $0 <tag>  (e.g. v1.0.0)}"
VERSION="${TAG#v}"
APP_NAME="Fennec"
DMG_NAME="${APP_NAME}-${VERSION}.dmg"
ZIP_NAME="${APP_NAME}.zip"
DERIVED="build"
APP_PATH="${DERIVED}/Build/Products/Release/${APP_NAME}.app"

cd "$(dirname "$0")/.."

echo "==> Generating Xcode project..."
xcodegen generate

echo "==> Building ${APP_NAME} (Release)..."
xcodebuild build \
  -project "${APP_NAME}.xcodeproj" \
  -scheme "${APP_NAME}" \
  -configuration Release \
  -derivedDataPath "${DERIVED}" \
  CODE_SIGN_IDENTITY="-" \
  CODE_SIGNING_ALLOWED=NO

if [ ! -d "${APP_PATH}" ]; then
  echo "Error: ${APP_PATH} not found"
  exit 1
fi

echo "==> Creating DMG..."
TMP_DIR=$(mktemp -d)
cp -R "${APP_PATH}" "${TMP_DIR}/"
ln -s /Applications "${TMP_DIR}/Applications"
hdiutil create -volname "${APP_NAME}" -srcfolder "${TMP_DIR}" -ov -format UDZO "${DMG_NAME}"
rm -rf "${TMP_DIR}"

echo "==> Creating ZIP..."
ditto -c -k --keepParent "${APP_PATH}" "${ZIP_NAME}"

SHA256=$(shasum -a 256 "${ZIP_NAME}" | awk '{print $1}')
echo "==> SHA256: ${SHA256}"

echo "==> Creating GitHub Release ${TAG}..."
if ! git rev-parse "${TAG}" >/dev/null 2>&1; then
  git tag "${TAG}"
fi
git push origin "${TAG}"
gh release create "${TAG}" \
  "${DMG_NAME}" \
  "${ZIP_NAME}" \
  --title "${TAG}" \
  --generate-notes

echo "==> Updating Homebrew tap..."
gh api repos/krgpi/homebrew-tap/dispatches \
  -f event_type=update-cask \
  -f "client_payload[cask]=fennec" \
  -f "client_payload[version]=${VERSION}" \
  -f "client_payload[sha256]=${SHA256}"

echo "==> Done! Released ${TAG}"
