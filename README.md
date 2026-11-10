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
- The **Devices** screen is the app's home when no host is connected. It lists
  paired hosts and the next pairing identity. **B** quits the app from the
  Devices screen.
- **F1** opens the Devices screen from the controller view. **F1+F2** held for 1.5 s
  opens it too; gamepad input is neutralised while both keys are held.
- Inside the Devices screen and its sub-screens, navigation follows NextUI: D-pad to move,
  A to select. In a sub-screen, B goes back; on the Devices screen, B quits the app.
  F1 is a Back alias and F2 a Select alias.
- A host has one row in the Devices screen. Selecting it opens its detail screen:
  connection, PC/Xbox controller type, and Forget pairing.
- The controller view shows the host, the profile, and the recent button and
  axis events. The input monitor is merged into this view.
- The status bar shows the time, Bluetooth, Wi-Fi and battery state, using the
  nextval colours and the NextUI font when they are available.
- After a host connects, the panel switches off after 10 seconds while the
  gamepad keeps working. Hold **F1** or **F2** to switch it back on; that hold
  does not trigger the key's action.
- **MENU** tap sends Home/Guide while the controller view is active.
- Rumble in Xbox mode when the host sends output reports; the PC profile has
  no rumble output report.
- Quit is immediate from the Devices screen with **B**. There is no quit row, no
  confirmation dialog and no destructive quit chord.
- Reconnects to the last host with the identity chosen for it.
- Runs the CPU governor in `powersave` and draws no frames while the panel is
  off, to keep battery use low.
- Clean restore of NextUI's Bluetooth stack, panel brightness and mixer
  settings when the app exits.

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
2. Open **Brick Controller** from Tools. With no host connected, the **Devices**
   is the home screen.
3. Select **Pair a new device**, then open Bluetooth settings on the host and
   pair with the name shown on the Brick (`Brick Controller` by default).
4. After pairing, the controller view shows the host, the profile and the
   recent input events. The panel switches off after 10 seconds. Hold **F1** or
   **F2** to light it again.
5. Press **F1**, or hold **F1+F2** for 1.5 s, to open the Devices screen. Select a host
   to connect or disconnect it, change its PC/Xbox controller type, or forget its
   pairing. Press **B** in the Devices screen to quit the app.
6. Tap **MENU** on the controller view to send Home/Guide.

Start and Select are not used by the app; they reach the host as normal gamepad
buttons. While the Devices screen or a sub-screen is open, gamepad input is kept away from the host.

## Xbox and PC controller types

Each host has its own identity, chosen from **Controller type** in the host detail:

- **PC** is a generic HID gamepad. It works with Windows, macOS,
  Linux/SteamOS, Android and iPadOS, and is the recommended default.
- **Xbox** advertises the Brick as an Xbox Wireless Controller
  (`045E:02FD`), for XInput on Windows and native Xbox support on macOS,
  iPadOS and Android. It also reads the host's rumble output report and drives
  the handheld motor.

A host remembers the controller identity it first paired with. After changing a
host from PC to Xbox or back, remove the old pairing on the host and pair again.
Changing the controller type of the host currently in use also forgets that pairing on
the Brick, restarts Bluetooth with the new identity, returns to the Devices screen and
waits for a fresh pairing. Hosts that are not currently connected keep their
entry; forget them on the host before connecting them with their new controller type.
The same controller type choice is available for the next pairing from **Pair a
controller → Change controller type**.

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

If the panel remains dark after a crash or a forced exit, hold **F1** or
**F2** to wake it. `launch.sh` also restores the saved brightness and runs
`syncsettings.elf` on exit; running

```sh
/mnt/SDCARD/.system/tg5040/bin/syncsettings.elf
```

restores NextUI's own brightness and volume settings.

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
