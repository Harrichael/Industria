#!/usr/bin/env bash
# Build the browser version into ./web (serve that folder with any static server).
set -euo pipefail
cd "$(dirname "$0")"
rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true
cargo build --release --target wasm32-unknown-unknown --bin ice_slide
cp target/wasm32-unknown-unknown/release/ice_slide.wasm web/
echo "Built web/ice_slide.wasm — try: python3 -m http.server -d web 8000"
