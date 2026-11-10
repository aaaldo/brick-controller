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
- The **Devices** screen is the app's home and is shown at launch. It always
  lists the paired hosts, even while one is connected, followed by **Pair a new
  device**. **B** quits the app from the Devices screen.
- **F1** opens the Devices screen from the controller view. **F1+F2** held for 1.5 s
  opens it too; gamepad input is neutralised while both keys are held.
- Opening Devices stops the active link or pairing attempt. Nothing connects
  automatically at launch or after returning to Devices: choose a host and
  press **Connect**, or open **Pair a new device**.
- Inside the Devices screen, D-pad moves, **A** connects the selected host (or
  opens **Pair a new device**) and **X** forgets it. In a sub-screen, B goes
  back, A selects; on the Devices screen, B quits. F1 is a Back alias and F2 a
  Select alias.
- Forgetting a host asks for confirmation. The PC/Xbox controller type is
  chosen only while pairing a new host, not for an already paired one.
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
- Connections are manual: the app never pages a saved host at launch, and the
  last host's identity is only used when it is explicitly connected.
- Runs the CPU governor in `powersave` and draws no frames while the panel is
  off, to keep battery use low.
- Clean restore of NextUI's Bluetooth stack, panel brightness and mixer
  settings when the app exits.
- Only one instance runs at a time: launching the Pak again stops the previous
  session and starts a fresh one.

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
2. Open **Brick Controller** from Tools. The **Devices** screen is the home
   screen and does not connect to a host on its own.
3. To pair a new host, select **Pair a new device**, then open Bluetooth
   settings on the host and pair with the name shown on the Brick (`Brick
   Controller` by default).
4. To use an already paired host, select it and press **Connect**. After the
   link is established, the controller view shows the host, the profile and the
   recent input events. The panel switches off after 10 seconds. Hold **F1** or
   **F2** to light it again.
5. Press **F1**, or hold **F1+F2** for 1.5 s, to open the Devices screen; the
   current link or pairing attempt is stopped. Press **A** to connect the
   selected host or **X** to forget it after confirming. Press **B** in the
   Devices screen to quit.
6. Tap **MENU** on the controller view to send Home/Guide.

Start and Select are not used by the app; they reach the host as normal gamepad
buttons. While the Devices screen or a sub-screen is open, gamepad input is kept away from the host.

## Xbox and PC controller types

The identity is chosen when pairing a new host:

- **PC** is a generic HID gamepad. It works with Windows, macOS,
  Linux/SteamOS, Android and iPadOS, and is the recommended default.
- **Xbox** advertises the Brick as an Xbox Wireless Controller
  (`045E:02FD`), for XInput on Windows and native Xbox support on macOS,
  iPadOS and Android. It also reads the host's rumble output report and drives
  the handheld motor.

Open **Pair a new device**, press **X** to choose the controller type, then pair
from the host. Each host keeps the identity it first paired with, and the Brick
uses it again when that host is connected.

The controller type of an already paired host is not editable from Devices. To
change it, forget the host on both sides, choose the other type in the pairing
screen and pair again.

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
