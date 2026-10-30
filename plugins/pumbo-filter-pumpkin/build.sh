#!/usr/bin/env bash
# Builds PumboFilter for every supported Pumpkin release into dist/ at the
# workspace root:
#   dist/PumboFilter-26.3.wasm   -> Pumpkin 0.2.1+26.3-26.51     (Minecraft 26.3)
#   dist/PumboFilter-0.2.0.wasm  -> Pumpkin 0.2.0+26.3-26.51     (Minecraft 26.3)
#   dist/PumboFilter-26.2.wasm   -> Pumpkin 0.1.0-dev+26.2-26.45 (Minecraft 26.2)
# Pumpkin 0.2.0 and 0.2.1 reject each other's builds, so the 0.2.0 file is built
# with the plugin API pinned back to 0.2.0; Cargo.lock is restored afterwards.
set -euo pipefail
cd "$(dirname "$0")/../.."
TARGET=wasm32-wasip2
PKG=pumbo-filter-pumpkin
OUT=pumbo_filter_pumpkin.wasm
mkdir -p dist target

cargo build -p "$PKG" --release --target "$TARGET"
cp "target/$TARGET/release/$OUT" dist/PumboFilter-26.3.wasm

cp Cargo.lock target/Cargo.lock.keep
trap 'cp target/Cargo.lock.keep Cargo.lock' EXIT
cargo update -q -p pumpkin-plugin-api@0.2.1 --precise 0.2.0+26.3-26.51
PUMBO_PUMPKIN=0.2.0 cargo build -p "$PKG" --release --target "$TARGET" --locked --target-dir target/mc263-020
cp "target/mc263-020/$TARGET/release/$OUT" dist/PumboFilter-0.2.0.wasm
cp target/Cargo.lock.keep Cargo.lock

cargo build -p "$PKG" --release --target "$TARGET" --no-default-features --features mc262 --target-dir target/mc262
cp "target/mc262/$TARGET/release/$OUT" dist/PumboFilter-26.2.wasm

# Each file names the api it was built against in its panic locations; a file
# with the wrong api would not load on its Pumpkin.
for f in 26.3:0.2.1+26.3-26.51 0.2.0:0.2.0+26.3-26.51 26.2:0.1.0-dev+26.2-26.45; do
  if ! grep -qaF "pumpkin-plugin-api-${f#*:}/" "dist/PumboFilter-${f%%:*}.wasm"; then
    echo "dist/PumboFilter-${f%%:*}.wasm is not built against pumpkin-plugin-api ${f#*:}" >&2
    exit 1
  fi
done
ls -l dist/PumboFilter-*.wasm
