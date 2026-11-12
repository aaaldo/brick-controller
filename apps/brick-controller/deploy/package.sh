#!/bin/sh
# Copyright 2026 the Brick Controller contributors
# SPDX-License-Identifier: GPL-3.0-only
# Build the static binary and stage the NextUI .pak archive.
#
#   VERSION=v1.1.0 ./apps/brick-controller/deploy/package.sh
#       -> dist/Brick.Controller.pak.zip
#
# The Pak Store extracts the archive into Tools/tg5040/<name>.pak, so the
# entries must stay at the archive root and must not contain the pak folder.
set -eu

cd "$(dirname "$0")/.."
ROOT=$(cd ../.. && pwd)
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
cp deploy/launch.sh deploy/icon.png deploy/README.txt "$STAGE/"
cp "$ROOT/LICENSE" "$ROOT/ADDITIONAL-TERMS.md" "$ROOT/CREDITS.md" \
    "$ROOT/crates/brick/assets/OFL.txt" "$STAGE/"

chmod +x "$STAGE/brick-controller" "$STAGE/launch.sh"

for file in launch.sh brick-controller icon.png README.txt LICENSE \
    ADDITIONAL-TERMS.md CREDITS.md OFL.txt; do
    [ -f "$STAGE/$file" ] || {
        echo "missing $STAGE/$file" >&2
        exit 1
    }
done

STORE_ARCHIVE="$DIST/Brick.Controller.pak.zip"

# The Pak Store extracts into Tools/tg5040/<name>.pak, so these entries must
# stay at the archive root. A top-level pak folder would be nested twice.
( cd "$STAGE" && zip -qr "$STORE_ARCHIVE" \
    launch.sh brick-controller icon.png README.txt \
    LICENSE ADDITIONAL-TERMS.md CREDITS.md OFL.txt )

echo "built $STORE_ARCHIVE"
ls -l "$STORE_ARCHIVE"
