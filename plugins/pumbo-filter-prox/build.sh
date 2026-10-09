#!/usr/bin/env bash
# Builds PumboFilter for PumboProx into dist/pumbo-filter.wasm: one file to drop into
# the proxy's plugins/. The manifest (pumbo-filter.yml), the default config
# (assets/config.yml) and the messages (assets/lang/) are built in; the proxy writes
# plugins/pumbo-filter/config.yml and lang/*.yml at the first start.
set -euo pipefail
cd "$(dirname "$0")"
ROOT=$(cd ../.. && pwd)
TARGET=wasm32-wasip2
mkdir -p dist
cargo build --release --target "$TARGET" -p pumbo-filter-prox --target-dir "$ROOT/target/filter-prox"
cp "$ROOT/target/filter-prox/$TARGET/release/pumbo_filter_prox.wasm" dist/pumbo-filter.wasm
ls -l dist/pumbo-filter.wasm
