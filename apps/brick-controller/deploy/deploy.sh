#!/bin/sh
# Copyright 2026 the Brick Controller contributors
# SPDX-License-Identifier: GPL-3.0-only
# Build, package and push the pak to a NextUI device over ADB.
#
#   VERSION=v1.1.0 ./apps/brick-controller/deploy/deploy.sh
#
# The bottom USB-C port is the device/ADB port. The top port is host-only.
set -eu

cd "$(dirname "$0")/.."
ROOT=../..
ADB="${ADB:-adb}"
VERSION="${VERSION:-dev}"

if ! command -v "$ADB" >/dev/null 2>&1; then
    ADB="$HOME/Library/Android/sdk/platform-tools/adb"
fi
[ -x "$ADB" ] || { echo "adb not found; set ADB=/path/to/adb" >&2; exit 1; }

VERSION="$VERSION" ./deploy/package.sh

STAGE=$(ls -d "$ROOT"/dist/stage-*/"Brick Controller.pak" | tail -1)
[ -n "$STAGE" ] || { echo "no staged pak found" >&2; exit 1; }

PAK="Brick Controller.pak"
DEST="/mnt/SDCARD/Tools/tg5040/$PAK"
echo "pushing $STAGE to $DEST"
"$ADB" shell "mkdir -p \"$DEST\""

for file in launch.sh icon.png LICENSE ADDITIONAL-TERMS.md CREDITS.md OFL.txt README.txt; do
    [ -f "$STAGE/$file" ] && "$ADB" push "$STAGE/$file" "$DEST/$file"
done

if ! "$ADB" push "$STAGE/brick-controller" "$DEST/brick-controller" 2>/dev/null; then
    echo "the app is running; the new build takes effect next launch (.next)"
    "$ADB" push "$STAGE/brick-controller" "$DEST/brick-controller.next"
fi

"$ADB" shell "chmod +x \"$DEST/launch.sh\" \"$DEST/brick-controller\" 2>/dev/null || true"
echo "installed; launch Tools > Brick Controller on the device"
