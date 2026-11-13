#!/usr/bin/env sh
# Builds the wasm module and its bindings, and copies the scenes it draws.
#
#   ./build.sh [out-dir]     default www/pkg
#
# wasm-bindgen's CLI has to be the exact version of the crate mln-rs pins:
#   cargo install wasm-bindgen-cli --version 0.2.130 --locked
set -eu
cd "$(dirname "$0")"
out=${1:-www/pkg}

cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir "$out" \
  target/wasm32-unknown-unknown/release/visgl_web_mln.wasm

# The scenes are the repository's own, so the browser draws what json_render draws.
mkdir -p www/scenes
for scene in san-francisco vancouver-blocks maplibre-arcs heathrow-flights \
             shadows data-filter osm-tiles terrain; do
  cp "../json/$scene.json" www/scenes/
done
echo "built into $out"
