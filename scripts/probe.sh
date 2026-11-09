#!/bin/sh
# Copyright 2026 the Brick Controller contributors
# SPDX-License-Identifier: GPL-3.0-only
# Read-only hardware probe for a NextUI device. Run over ADB or SSH:
#
#   adb push scripts/probe.sh /tmp/probe.sh
#   adb shell "sh /tmp/probe.sh 2>&1" > probe.txt
#
# It writes nothing to the device. It intentionally does not read Bluetooth
# addresses, serial numbers or other device identifiers.

say() { echo; echo "=== $* ==="; }

say "System"
cat /proc/version 2>/dev/null
uname -a 2>/dev/null
cat /etc/version 2>/dev/null
cat /etc/trimui_device.txt 2>/dev/null
echo "PLATFORM=${PLATFORM:-unset} DEVICE=${DEVICE:-unset}"
strings /usr/trimui/bin/MainUI 2>/dev/null | grep -m1 '^Trimui'
for file in /mnt/SDCARD/.system/version.txt /mnt/SDCARD/.system/tg5040/version.txt; do
    [ -f "$file" ] && { echo "--- $file"; cat "$file"; }
done

say "Framebuffer"
ls -l /dev/fb0 /dev/disp 2>/dev/null
for file in bits_per_pixel virtual_size stride modes; do
    [ -f "/sys/class/graphics/fb0/$file" ] && {
        echo "--- $file"
        cat "/sys/class/graphics/fb0/$file"
    }
done
fbset -i 2>/dev/null | head -40

say "Input devices"
cat /proc/bus/input/devices 2>/dev/null
for event in /dev/input/event*; do
    [ -e "$event" ] || continue
    name=$(basename "$event")
    echo "--- $name"
    cat "/sys/class/input/$name/device/name" 2>/dev/null
    echo -n "key: "; cat "/sys/class/input/$name/device/capabilities/key" 2>/dev/null
    echo -n "abs: "; cat "/sys/class/input/$name/device/capabilities/abs" 2>/dev/null
done
if command -v getevent >/dev/null 2>&1; then
    getevent -pl 2>/dev/null | head -160
fi

say "Bluetooth"
ls -l /sys/class/bluetooth 2>/dev/null
for hci in /sys/class/bluetooth/hci*; do
    [ -e "$hci" ] || continue
    echo "--- $hci"
    cat "$hci/name" 2>/dev/null
done
ps 2>/dev/null | grep -E 'bluetoothd|bluealsa|hciattach|rtk_hciattach|adbd|trimui_inputd|keymon' | grep -v grep
ls -l /etc/bluetooth 2>/dev/null
if [ -f /etc/bluetooth/bluetoothd ]; then
    echo "--- /etc/bluetooth/bluetoothd"
    head -50 /etc/bluetooth/bluetoothd
fi
if [ -f /mnt/SDCARD/.system/tg5040/etc/bluetooth/bt_init.sh ]; then
    echo "--- bt_init.sh"
    head -80 /mnt/SDCARD/.system/tg5040/etc/bluetooth/bt_init.sh
fi
for command in bluetoothd hciconfig bluetoothctl btmgmt systemctl pidof killall; do
    printf '%s: ' "$command"
    command -v "$command" 2>/dev/null || echo "absent"
done

say "Hardware"
ls -l /sys/class/backlight 2>/dev/null
for backlight in /sys/class/backlight/*; do
    [ -e "$backlight" ] || continue
    echo "--- $backlight"
    cat "$backlight/max_brightness" "$backlight/brightness" 2>/dev/null
done
ls -l /sys/class/motor 2>/dev/null
for motor in /sys/class/motor/*; do
    [ -e "$motor" ] || continue
    echo "--- $motor"
    cat "$motor" 2>/dev/null
done
ls -l /sys/class/power_supply 2>/dev/null
cat /sys/class/power_supply/axp2202-battery/capacity 2>/dev/null

say "USB gadget"
ls -l /sys/class/udc 2>/dev/null
for udc in /sys/class/udc/*; do
    [ -e "$udc" ] || continue
    echo "$udc: $(cat "$udc/state" 2>/dev/null)"
done
ls -l /sys/kernel/config/usb_gadget 2>/dev/null

say "Tools"
for command in fbset getevent evtest strace gdb; do
    printf '%s: ' "$command"
    command -v "$command" 2>/dev/null || echo "absent"
done

say "Done"
