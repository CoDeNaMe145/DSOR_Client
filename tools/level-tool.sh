#!/bin/sh
# Build/run the headless Rust level tool from any working directory.
set -eu
workspace=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
exec cargo run --quiet --locked --release --manifest-path "$workspace/Cargo.toml" -p dsor-level-data --bin dsor-levels -- "$@"
