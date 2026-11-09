# Brick Controller

Turn a **TrimUI Brick running NextUI** into a Bluetooth gamepad for a PC, Mac,
Linux/SteamOS machine, Android device or iPad.

The Brick shows up as a standard Bluetooth HID controller. Nothing has to be
installed on the host.

> **Status: Phase 1.** Bluetooth only, PC and Xbox identities on the Brick
> standard (`tg5040`). Brick Pro, Smart Pro and `tg5050` are planned next. USB
> HID is not part of the current release.

## Features

- Standard Bluetooth HID gamepad: no host driver.
- D-pad, A/B/X/Y, L1/L2/R1/R2, Start, Select and Menu.
- Start and Select are passed to the host as normal gamepad buttons.
- **F1** opens the settings screen.
- **F2** opens a real-time input monitor. Every press and release, D-pad
  direction, trigger value and stick movement is listed; the list scrolls so
  the newest entry is always at the bottom. Only **F2** returns to the
  controller view.
- **F1+F2** held for three seconds quits the app; the footer says
  "keep holding to quit…" while both are held.
- The settings screen lists paired hosts. **A** connects a disconnected host or
  disconnects the connected one; **F2** switches the selected host between the
  PC and Xbox identities. With no paired host, a **Next pairing** row chooses
  the identity the next host will see.
- **MENU** tap sends Home/Guide.
- Rumble in Xbox mode when the host sends output reports; the PC profile has
  no rumble output report.
- The controller view is shown only while a host is connected. Without a
  connection, the app stays on the settings screen.
- Reconnects to the last host with the identity chosen for it.
- Clean restore of NextUI's Bluetooth stack when the app exits.

## Requirements

- TrimUI Brick with NextUI (`tg5040`).
- Bluetooth enabled in NextUI before launching the app.
- A data-capable USB-C cable only if you want to use ADB/SSH for development;
  normal use is wireless.

## Install

1. Build or download `Brick Controller.pak`.
2. Copy the `Brick Controller.pak` folder to:

   ```text
   /mnt/SDCARD/Tools/tg5040/
   ```

3. Eject the SD card safely and put it back in the Brick.
4. Open **Tools → Brick Controller**.

For development, ADB can push the pak directly:

```sh
VERSION=0.1.0 ./apps/brick-controller/deploy/deploy.sh
```

Use the **bottom USB-C port** for ADB. The top port is a USB host port.

## Use

1. Turn Bluetooth on in NextUI.
2. Open **Brick Controller** from Tools.
3. On the host, open Bluetooth settings and pair with the name shown on the
   Brick (`Brick Controller` by default).
4. Press **F1** for settings; **F2** shows the input monitor.
5. In settings, use the D-pad to select a host, **A** to connect/disconnect,
   and **F2** to switch that host between PC and Xbox. When a host is
   connected, **F1** returns to the controller view; without a host the
   settings screen stays up.
6. Tap **MENU** to send Home/Guide. Hold **F1+F2** for three seconds to quit.

Start and Select are not used by the app; they reach the host as normal gamepad
buttons.

## Xbox and PC profiles

Each host has its own identity, chosen with **F2** on the settings screen:

- **PC** is a generic HID gamepad. It works with Windows, macOS,
  Linux/SteamOS, Android and iPadOS, and is the recommended default.
- **Xbox** advertises the Brick as an Xbox Wireless Controller
  (`045E:02FD`), for XInput on Windows and native Xbox support on macOS,
  iPadOS and Android. It also reads the host's rumble output report and drives
  the handheld motor.

A host remembers the controller identity it first paired with. After changing a
host from PC to Xbox or back, remove the old pairing on the host and pair again.
Changing the profile of the host currently in use also forgets that pairing on
the Brick, restarts Bluetooth with the new identity, keeps the settings screen
open and waits for a fresh pairing. Hosts that are not currently connected keep
their entry; forget them on the host before connecting them with their new
profile.

## Limitations

- **Bluetooth only**. USB HID is not available.
- Rumble is available in Xbox mode. The PC profile's descriptor has no output
  report, so PC hosts do not vibrate the Brick.
- The Brick standard has no analog sticks, gyro, trackpads or back buttons.
  Sticks are reported centred for host compatibility.
- Bluetooth audio and the controller cannot share the radio cleanly. `bluealsa`
  is stopped while the controller runs and restored on exit.
- Wi-Fi and Bluetooth share one antenna on the Brick; expect reduced Wi-Fi
  throughput during a session.
- Windows compatibility is less tested than macOS, in both profiles.
- A host remembers the controller identity it first paired. Switching between
  PC and Xbox therefore requires removing the old pairing on that host and
  pairing again.

## Troubleshooting

Logs are written to:

```text
/mnt/SDCARD/.userdata/tg5040/logs/BrickController.txt
```

If Bluetooth remains stopped after a crash or a forced exit, reboot the Brick
or run:

```sh
/mnt/SDCARD/.system/tg5040/etc/bluetooth/bt_init.sh start
```

## Build

Requires Rust and the `aarch64-unknown-linux-musl` target:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
rustup target add aarch64-unknown-linux-musl
cargo fmt --all
cargo test --workspace
cargo build --release --target aarch64-unknown-linux-musl -p brick-controller
VERSION=0.1.0 ./apps/brick-controller/deploy/package.sh
```

The binary is static; no Rust runtime is needed on the Brick.

## Credits and license

This project is based on **Padpod**, the TrimUI Brick Pro Bluetooth controller
by its authors: <https://github.com/duyquang6/padpod>. Padpod is licensed under
the GNU GPL v3 with additional terms. The license and credits travel with the
pak (`LICENSE`, `ADDITIONAL-TERMS.md`, `CREDITS.md`, `OFL.txt`).

This modified version is released under the same GPL-3.0 terms and additional
terms as Padpod. It is not an official Padpod release.
