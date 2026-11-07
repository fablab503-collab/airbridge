#!/bin/sh
# Builds the Graphite editor (Rust -> WebAssembly) with the Immich Studio patch.
# Needs: git, Rust (rustup), Node.js 22+. Takes 20-60 minutes the first time.
# Result: immich-studio/graphite/dist  (served by Studio at /studio/graphite/)
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
COMMIT=d9a80b5c94afb4e698fda95af96ae5cbff179f14   # Graphite version this patch was made for
SRC="$HERE/src"
[ -d "$SRC/.git" ] || git clone https://github.com/GraphiteEditor/Graphite "$SRC"
cd "$SRC"
git fetch --depth 1 origin "$COMMIT" && git checkout -f "$COMMIT"
git apply "$HERE/immich-integration.patch"
rustup target add wasm32-unknown-unknown
cargo run build web release
# Studio serves Graphite under /studio/graphite/, so rebuild the page with relative paths
(cd frontend && node node_modules/vite/bin/vite.js build --base ./)
rm -rf "$HERE/dist" && cp -r frontend/dist "$HERE/dist"
echo "Graphite built into $HERE/dist"
