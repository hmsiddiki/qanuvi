#!/bin/sh
# Relay kit K5: builds the relay program for Linux x86_64 and packs the kit (program + setup.sh + README).
# Runs on a Linux x86_64 machine (the laptop): sh scripts/build-relay-kit.sh [out folder, default ./build/relay-kit]
# Then sign and put it on the channel from This PC: node scripts/publish-relay-kit.mjs <the .tar.gz>
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=${1:-$ROOT/build/relay-kit}
[ "$(uname -s)-$(uname -m)" = Linux-x86_64 ] || { echo "Build the kit on Linux x86_64."; exit 1; }

cd "$ROOT/src-tauri"
cargo build --release --no-default-features --bin qanuvi-relay
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)

STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
KIT=$STAGE/qanuvi-relay-kit
mkdir -p "$KIT"
install -m 0755 target/release/qanuvi-relay "$KIT/qanuvi-relay"
install -m 0755 "$ROOT/deploy/relay/kit/setup.sh" "$KIT/setup.sh"
install -m 0644 "$ROOT/deploy/relay/kit/README.md" "$KIT/README.md"
echo "$VERSION" > "$KIT/VERSION"

mkdir -p "$OUT"
TAR=$OUT/qanuvi-relay-kit-linux-x86_64.tar.gz
tar -C "$STAGE" --owner=0 --group=0 -czf "$TAR" qanuvi-relay-kit
(cd "$OUT" && sha256sum "$(basename "$TAR")" > "$(basename "$TAR").sha256")
echo "Kit $VERSION: $TAR ($(du -h "$TAR" | cut -f1)) + .sha256"
