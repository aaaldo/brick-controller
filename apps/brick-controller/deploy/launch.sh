#!/bin/sh
# Copyright 2026 the Brick Controller contributors
# SPDX-License-Identifier: GPL-3.0-only
# NextUI entry point for Brick Controller.
#
# The binary owns the framebuffer and takes over Bluetooth for the session.
# This wrapper is responsible for restoring both on every exit path.

DIR=$(dirname "$0")
DIR=$(cd "$DIR" && pwd)
LOG="${LOGS_PATH:-/tmp}/BrickController.txt"

export HOME="$DIR"
export BRICK_CONTROLLER_DATA="${USERDATA_PATH:-/tmp}/brick-controller"
mkdir -p "$BRICK_CONTROLLER_DATA"

# FAT refuses to replace a running executable. deploy.sh leaves the new build
# beside the old one; promote it now that nothing is running.
if [ -x "$DIR/brick-controller.next" ]; then
    mv -f "$DIR/brick-controller.next" "$DIR/brick-controller"
fi

# NextUI's own Bluetooth script restarts bluetoothd and bluealsa and restores
# the adapter alias. The stock init script alone would leave bluealsa stopped.
NEXTUI_BT_INIT="${SYSTEM_PATH:-}/etc/bluetooth/bt_init.sh"
[ -f "$NEXTUI_BT_INIT" ] || NEXTUI_BT_INIT=""
export NEXTUI_BT_INIT

BT_WAS_ON=0
pidof bluetoothd >/dev/null 2>&1 && BT_WAS_ON=1

cleanup() {
    "$DIR/brick-controller" restore >>"$LOG" 2>&1
    if [ "$BT_WAS_ON" = 1 ] && [ -n "$NEXTUI_BT_INIT" ]; then
        sh "$NEXTUI_BT_INIT" start >>"$LOG" 2>&1
    fi
    sync
}
trap cleanup EXIT INT TERM

cd "$DIR"
"$DIR/brick-controller" >>"$LOG" 2>&1
