#!/bin/bash
# Build and package the Chrome extension for distribution.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
EXT_DIR="$ROOT_DIR/chrome-ext"
DIST_DIR="$ROOT_DIR/dist"

if ! command -v zip >/dev/null 2>&1; then
  echo "zip is required to package the extension." >&2
  exit 1
fi

if ! command -v node >/dev/null 2>&1; then
  echo "node is required to read the extension version." >&2
  exit 1
fi

bash "$SCRIPT_DIR/build-extension.sh"

VERSION="$(node -p "require('$EXT_DIR/manifest.json').version")"
ZIP_NAME="cirrus-extension-v${VERSION}.zip"
ZIP_PATH="$DIST_DIR/$ZIP_NAME"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

mkdir -p "$DIST_DIR"
rm -f "$ZIP_PATH"

# Copy only runtime files that should ship inside the extension package.
rsync -a \
  --exclude 'STORE-LISTING.md' \
  --exclude '.DS_Store' \
  --exclude '*/.DS_Store' \
  "$EXT_DIR/" "$TMP_DIR/package/"

(
  cd "$TMP_DIR/package"
  zip -qr "$ZIP_PATH" .
)

echo "Packaged extension at: $ZIP_PATH"
echo "Excluded from package: STORE-LISTING.md, .DS_Store"
