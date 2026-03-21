#!/bin/bash
# build.sh — Build Wasm + JS bindings
set -euo pipefail

echo "=== Step 1: Build Wasm ==="
cargo build \
  --target wasm32-unknown-unknown \
  --release \
  -p citeme-engine-wasm

echo "=== Step 2: Generate JS bindings ==="
wasm-bindgen \
  target/wasm32-unknown-unknown/release/citeme_engine_wasm.wasm \
  --out-dir js/pkg \
  --target web \
  --typescript

echo "=== Step 3: Optimize Wasm (optional) ==="
if command -v wasm-opt &> /dev/null; then
  wasm-opt js/pkg/citeme_engine_wasm_bg.wasm \
    -Oz \
    --enable-bulk-memory \
    --enable-nontrapping-float-to-int \
    -o js/pkg/citeme_engine_wasm_bg.wasm
  echo "wasm-opt applied"
else
  echo "wasm-opt not found, skipping optimization"
fi

echo "=== Done ==="
ls -lh js/pkg/citeme_engine_wasm_bg.wasm
gzip -c js/pkg/citeme_engine_wasm_bg.wasm | wc -c | awk '{printf "Gzipped: %.1f KB\n", $1/1024}'
