#!/bin/bash
# Build WASM and copy to Chrome Extension directory
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
EXT_DIR="$ROOT_DIR/chrome-ext"

echo "Building WASM bridge (release)..."
cargo build -p phaselith-wasm-bridge --target wasm32-unknown-unknown --release

echo "Copying WASM to extension..."
cp "$ROOT_DIR/target/wasm32-unknown-unknown/release/phaselith_wasm_bridge.wasm" "$EXT_DIR/phaselith_wasm_bridge.wasm"

# Optional: optimize with wasm-opt if available
if command -v wasm-opt &> /dev/null; then
  echo "Optimizing WASM with wasm-opt..."
  wasm-opt -Oz "$EXT_DIR/phaselith_wasm_bridge.wasm" -o "$EXT_DIR/phaselith_wasm_bridge.wasm"
fi

echo "Extension ready at: $EXT_DIR"
echo "Load it in Chrome: chrome://extensions → Developer mode → Load unpacked"
