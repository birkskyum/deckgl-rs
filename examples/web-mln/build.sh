#!/usr/bin/env sh
# Builds the wasm module, its bindings and the scenes it draws.
#
#   ./build.sh                            into www/, for the local page
#   ./build.sh ../../site/public/demos    into the site, for GitHub Pages
#
# wasm-bindgen's CLI has to be the exact version of the crate mln-rs pins:
#   cargo install wasm-bindgen-cli --version 0.2.130 --locked
# wasm-opt (binaryen) is used when it is on PATH: it takes the module from about
# 12 MB to 8.6 MB, 3.4 MB over the wire once the server compresses it.
set -eu
cd "$(dirname "$0")"
dest=${1:-www}

cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir "$dest/pkg" \
  target/wasm32-unknown-unknown/release/visgl_web_mln.wasm

module="$dest/pkg/visgl_web_mln_bg.wasm"
if command -v wasm-opt >/dev/null 2>&1; then
  wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
    "$module" -o "$module.opt"
  mv "$module.opt" "$module"
else
  echo "wasm-opt not found, leaving the module unoptimised" >&2
fi

# The scenes are the repository's own, so the browser draws what json_render draws.
mkdir -p "$dest/scenes"
for scene in san-francisco vancouver-blocks maplibre-arcs heathrow-flights \
             shadows data-filter osm-tiles terrain; do
  cp "../json/$scene.json" "$dest/scenes/"
done

# The driver sits beside them, so the page can import it from one place.
[ "$dest" = www ] || cp www/demo.js "$dest/demo.js"

echo "built into $dest ($(du -h "$module" | cut -f1))"
