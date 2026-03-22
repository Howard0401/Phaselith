#!/bin/bash
# Usage: ./scripts/release.sh 0.2.0 "Release message"

set -e

VERSION="$1"
MESSAGE="${2:-Release v$VERSION}"

if [ -z "$VERSION" ]; then
    echo "Usage: ./scripts/release.sh <version> [message]"
    echo "Example: ./scripts/release.sh 0.2.0 \"License integration release\""
    exit 1
fi

echo "==> Updating version to $VERSION across all targets..."

# 1. Root Cargo.toml
sed -i "s/^version = \".*\"/version = \"$VERSION\"/" Cargo.toml
echo "    ✓ Cargo.toml"

# 2. Tauri conf
sed -i "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" crates/tauri-app/tauri.conf.json
echo "    ✓ tauri.conf.json"

# 3. Chrome Extension manifest
sed -i "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" chrome-ext/manifest.json
echo "    ✓ chrome-ext/manifest.json"

# 4. Update Cargo.lock
cargo update --workspace 2>/dev/null || true
echo "    ✓ Cargo.lock"

echo ""
echo "==> Committing..."
git add Cargo.toml Cargo.lock \
    crates/tauri-app/tauri.conf.json \
    chrome-ext/manifest.json

git commit -m "release: v$VERSION — $MESSAGE

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"

echo ""
echo "==> Tagging v$VERSION..."
git tag -a "v$VERSION" -m "$MESSAGE"

echo ""
echo "==> Pushing..."
git push && git push origin "v$VERSION"

echo ""
echo "==> Done! Released v$VERSION"
echo "    Cargo.toml:       $VERSION"
echo "    tauri.conf.json:  $VERSION"
echo "    manifest.json:    $VERSION"
echo "    Git tag:          v$VERSION"
