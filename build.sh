#!/bin/bash
# build.sh — Build Wasm + JS bindings
set -euo pipefail

# The wasm-bindgen CLI must be exactly the version of the wasm-bindgen crate
# in Cargo.lock, or it refuses the module — after the whole cargo build.
lock_version=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/"/, "", $3); print $3 }' Cargo.lock)
cli_version=$(wasm-bindgen --version | awk '{ print $2 }')
if [[ "$cli_version" != "$lock_version" ]]; then
  echo "error: wasm-bindgen CLI is $cli_version, Cargo.lock has $lock_version" >&2
  echo "       cargo install wasm-bindgen-cli --version $lock_version --locked" >&2
  exit 1
fi

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

echo "=== Step 3: Stage README for the npm package ==="
# npm only publishes files under the package root (js/), so the repo README
# has to be copied in — without it the package page on npmjs.com renders no
# documentation at all. Copied at build time, gitignored, listed in `files`.
cp README.md js/README.md

echo "=== Step 4: Optimize Wasm (optional) ==="
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
