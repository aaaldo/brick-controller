#!/bin/sh
# Copyright 2026 the Brick Controller contributors
# SPDX-License-Identifier: GPL-3.0-only
# NextUI entry point for Brick Controller.
#
# The binary owns the framebuffer and takes over Bluetooth for the session.
# The wired PC mode also borrows the ADB USB gadget; this wrapper is responsible
# for restoring both on every exit path.

DIR=$(dirname "$0")
DIR=$(cd "$DIR" && pwd)
LOG="${LOGS_PATH:-/tmp}/BrickController.txt"

case "${PLATFORM:-}:${DEVICE:-}" in
    h700:rg40xxh|h700:rg40xx-h|'h700:rg40xx h') ;;
    h700:*)
        echo "Brick Controller supports only the RG40XX H on H700 (${DEVICE:-unknown} detected)." >>"$LOG"
        exit 1
        ;;
esac
if [ "${PLATFORM:-}" != h700 ]; then
    case "${DEVICE:-}" in
        rg40xxh|rg40xx-h|'rg40xx h')
            echo "Brick Controller: ${DEVICE} requires PLATFORM=h700 (got ${PLATFORM:-unknown})." >>"$LOG"
            exit 1
            ;;
    esac
fi

export HOME="$DIR"
export BRICK_CONTROLLER_DATA="${USERDATA_PATH:-/tmp}/brick-controller"
mkdir -p "$BRICK_CONTROLLER_DATA"

# One instance only: the app owns the framebuffer, the HID PSMs and its own
# bluetoothd. A leftover instance from a crash or a development launch would
# make the new one fail with "Address in use" and show a misleading Bluetooth
# error. Stop it first and let its wrapper restore the panel and Bluetooth.
PIDFILE="/tmp/brick-controller-launch.pid"
OLD_PID=""
if [ -f "$PIDFILE" ]; then
    OLD_PID=$(cat "$PIDFILE" 2>/dev/null)
    case "$OLD_PID" in
        ''|*[!0-9]*) OLD_PID="" ;;
    esac
fi
OLD_APP=$(pidof brick-controller)
if [ -n "$OLD_APP" ]; then
    echo "launch: stopping previous instance ($OLD_APP)" >>"$LOG" 2>&1
    kill $OLD_APP 2>/dev/null
fi
if [ -n "$OLD_PID" ] && [ "$OLD_PID" != "$$" ] && [ -d "/proc/$OLD_PID" ]; then
    i=0
    while [ -d "/proc/$OLD_PID" ] && [ "$i" -lt 10 ]; do
        sleep 1
        i=$((i + 1))
    done
    if [ -d "/proc/$OLD_PID" ]; then
        kill -9 "$OLD_PID" 2>/dev/null
    fi
elif [ -n "$OLD_APP" ]; then
    # Older builds did not write a PID file; leave time for their cleanup.
    sleep 2
fi
echo "$$" >"$PIDFILE"

# A bug report needs the firmware and model behind the log, and one previous
# run is enough context. Rotate after the previous wrapper has finished so the
# new run starts with its own header.
if [ -f "$LOG" ] && [ "$(wc -c <"$LOG" | tr -d ' ')" -gt 262144 ]; then
    mv -f "$LOG" "$LOG.1"
fi
NEXTUI_VERSION=$(head -n 1 "${SDCARD_PATH:-/mnt/SDCARD}/.system/version.txt" 2>/dev/null | tr -d '\r')
echo "launch: $(date '+%Y-%m-%d %H:%M:%S') platform=${PLATFORM:-unknown} device=${DEVICE:-unknown} nextui=${NEXTUI_VERSION:-unknown}" >>"$LOG"

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

# H700 keeps its daemon outside /usr/bin. The binary validates this path
# before executing it, so a bad environment cannot launch an arbitrary binary.
if [ "${PLATFORM:-}" = h700 ]; then
    export NEXTUI_BLUETOOTHD=/usr/libexec/bluetooth/bluetoothd
fi

# NextUI puts the CPU on `performance` before every Pak. This app only draws a
# small status screen and reads input, so the lower capped clock is enough.
# NextUI restores its own governor when the Pak exits.
GOVERNOR="${SYSTEM_PATH:-}/bin/governor.sh"
if [ -f "$GOVERNOR" ]; then
    sh "$GOVERNOR" powersave >>"$LOG" 2>&1 || \
        echo "governor.sh failed, continuing" >>"$LOG" 2>&1
fi

# Restores brightness, volume and display calibration from NextUI's shared
# settings after the app had the panel dark or changed the mixer.
SYNC_SETTINGS="${SYSTEM_PATH:-}/bin/syncsettings.elf"

# Remember whether Bluetooth was already up: a user who launched the Pak with
# Bluetooth off must not get it switched on behind their back at exit.
BT_WAS_ON=0
BT_STARTED=0

# Stop only the daemons, never `bt_init.sh stop`: the full stop can tear down
# `hciattach` and make the adapter unavailable until a reboot.
stop_bluetooth_daemons() {
    killall bluealsa 2>/dev/null
    killall bluetoothd 2>/dev/null
}

cleanup() {
    # Order matters. The binary restores the backlight, its own Bluetooth
    # daemon and the g1 USB gadget first; syncsettings then puts NextUI's shared
    # settings back over it. Starting bt_init last avoids fighting the binary's
    # own restore.
    "$DIR/brick-controller" restore >>"$LOG" 2>&1
    if [ -x "$SYNC_SETTINGS" ]; then
        # syncsettings links libmsettings from the NextUI system folder.
        LD_LIBRARY_PATH="${SYSTEM_PATH:-}/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
            "$SYNC_SETTINGS" >>"$LOG" 2>&1
    fi
    if [ "$BT_STARTED" = 1 ]; then
        stop_bluetooth_daemons
    elif [ "$BT_WAS_ON" = 1 ] && [ -n "$NEXTUI_BT_INIT" ]; then
        # Always run the NextUI script here: it restarts both bluetoothd and
        # bluealsa. Checking only bluetoothd left bluealsa stopped after a
        # session on the Brick (observed in hardware testing).
        sh "$NEXTUI_BT_INIT" start >>"$LOG" 2>&1
    fi
    sync
    if [ "$(cat "$PIDFILE" 2>/dev/null)" = "$$" ]; then
        rm -f "$PIDFILE"
    fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# A killed launcher may have left the dedicated daemon running, or no daemon
# at all if it died during a profile switch. Recover before sampling NextUI.
if ! "$DIR/brick-controller" restore >>"$LOG" 2>&1; then
    echo "launch: could not recover the previous session" >>"$LOG"
    exit 1
fi
pidof bluetoothd >/dev/null 2>&1 && BT_WAS_ON=1

if [ "${PLATFORM:-}" = h700 ] && [ "$BT_WAS_ON" = 0 ] && [ -n "$NEXTUI_BT_INIT" ]; then
    BT_STARTED=1
    # Tell the binary that this daemon belongs to the wrapper: at exit it must
    # stop it, not restore NextUI's stack.
    export BRICK_CONTROLLER_BT_STARTED=1
    if ! sh "$NEXTUI_BT_INIT" start >>"$LOG" 2>&1; then
        echo "launch: could not initialize H700 Bluetooth" >>"$LOG"
        exit 1
    fi
fi

cd "$DIR"
"$DIR/brick-controller" >>"$LOG" 2>&1
