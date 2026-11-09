#!/bin/sh
# Copyright 2026 the Brick Controller contributors
# SPDX-License-Identifier: GPL-3.0-only
# Build the static binary and stage a NextUI .pak archive.
#
#   VERSION=0.1.0 ./apps/brick-controller/deploy/package.sh
#       -> dist/Brick Controller.pak.zip
#
# The archive contains the contents of the pak folder, as the Pak Store
# expects. Installation is a copy into Tools/tg5040 on the SD card.
set -eu

cd "$(dirname "$0")/.."
ROOT=../..
TARGET=aarch64-unknown-linux-musl
VERSION="${VERSION:-dev}"
APP="Brick Controller"
PAK="$APP.pak"
DIST="$ROOT/dist"
CARGO="${CARGO:-cargo}"

if ! command -v "$CARGO" >/dev/null 2>&1; then
    echo "cargo not found; add the Rust toolchain to PATH first" >&2
    exit 1
fi

echo "building $APP $VERSION"
BRICK_CONTROLLER_VERSION="$VERSION" "$CARGO" build --release --target "$TARGET" -p brick-controller

BINARY="$ROOT/target/$TARGET/release/brick-controller"
[ -x "$BINARY" ] || { echo "missing $BINARY" >&2; exit 1; }

STAMP="$(date +%Y%m%d-%H%M%S)"
STAGE="$DIST/stage-$STAMP/$PAK"
mkdir -p "$STAGE"

cp "$BINARY" "$STAGE/brick-controller"
cp deploy/launch.sh deploy/icon.png "$STAGE/"
cp "$ROOT/LICENSE" "$ROOT/ADDITIONAL-TERMS.md" "$ROOT/CREDITS.md" \
    "$ROOT/crates/brick/assets/OFL.txt" "$STAGE/"
[ -f "$ROOT/README.md" ] && cp "$ROOT/README.md" "$STAGE/README.txt"

chmod +x "$STAGE/brick-controller" "$STAGE/launch.sh"

ARCHIVE="$DIST/$PAK.zip"
( cd "$DIST/stage-$STAMP" && zip -qr "$ARCHIVE" "$PAK" )

echo "built $ARCHIVE"
ls -l "$ARCHIVE"
