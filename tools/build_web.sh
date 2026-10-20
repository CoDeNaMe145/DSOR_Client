#!/usr/bin/env bash
# Build the browser client twice -- WebGPU and WebGL2 -- under client/dist, with
# the loader page that picks one (client/web/index.html).
#
#   tools/build_web.sh            both builds
#   tools/build_web.sh webgl2     only one of them (webgl2 | webgpu)
#
# bevy specialises its renderer for one graphics API at compile time, so each
# build has its own feature (client/Cargo.toml) and its own target directory
# (switching features in one directory would rebuild bevy every time).
#   tools/build_web.sh --fast     both, quick to rebuild (profile wasm-fast: no LTO,
#                                 no wasm-opt, incremental) -- for iterating
set -euo pipefail
cd "$(dirname "$0")/../client"
export PATH="$HOME/.cargo/bin:$PATH"
if [ "${1:-}" = "--fast" ]; then
    # index.html names its profile (wasm-release, wasm-opt 3), and that wins over
    # --cargo-profile: the fast builds read a copy naming wasm-fast, no wasm-opt.
    # Both APIs: a browser without WebGPU (Firefox) loads the WebGL2 build, and
    # a stale one there hid every change ("rien n'a changé").
    sed -e 's/data-cargo-profile="wasm-release"/data-cargo-profile="wasm-fast"/' \
        -e 's/data-wasm-opt="3"/data-wasm-opt="0"/' index.html > index.fast.html
    for kind in webgpu webgl2; do
        echo "[web] $kind (fast)"
        CARGO_TARGET_DIR="../target/web-fast-$kind" trunk build index.fast.html \
            --no-default-features --features "$kind" \
            --dist "dist/$kind" --filehash false --public-url "./"
        [ -f "dist/$kind/index.fast.html" ] && mv "dist/$kind/index.fast.html" "dist/$kind/index.html"
    done
    cp web/index.html dist/index.html
    cp web/audio-unlock.js dist/audio-unlock.js
    ln -sfn ../../assets dist/assets
    echo "[web] done (fast): client/dist"
    exit 0
fi
kinds=("${@:-webgl2 webgpu}")
for kind in ${kinds[@]}; do
    echo "[web] $kind"
    CARGO_TARGET_DIR="../target/web-$kind" trunk build --release \
        --no-default-features --features "$kind" \
        --dist "dist/$kind" --filehash false --public-url "./"
done
cp web/index.html dist/index.html
cp web/audio-unlock.js dist/audio-unlock.js
# The converted game data is never copied: the page reads it through a symlink.
ln -sfn ../../assets dist/assets
echo "[web] done: client/dist (open index.html?map=a0200_kingscity)"
