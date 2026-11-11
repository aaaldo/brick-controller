# Brick Controller

Turn a **TrimUI Brick running NextUI** into a Bluetooth gamepad for a PC, Mac,
Linux/SteamOS machine, Android device or iPad.

The Brick shows up as a standard Bluetooth HID controller. Nothing has to be
installed on the host.

> **Status: Phase 1.** Bluetooth only, PC, Xbox, PlayStation and Switch
> identities on the Brick standard (`tg5040`). Brick Pro, Smart Pro and
> `tg5050` are planned next. USB HID is not part of the current release.

## Features

- Standard Bluetooth HID gamepad: no host driver.
- D-pad, A/B/X/Y, L1/L2/R1/R2, Start, Select, Menu, and F1/F2 as L3/R3.
- Start and Select are passed to the host as normal gamepad buttons.
- The **Devices** screen is the app's home and is shown at launch. It always
  lists the paired hosts, even while one is connected, followed by **Pair a new
  device**. **B** quits the app from the Devices screen.
- Holding **Menu+Start** for 1 s disconnects and opens the Devices screen;
  gamepad input is neutralised while the chord is held. F1 and F2 are not used
  for navigation: they reach the host as **L3** and **R3**.
- Opening Devices stops the active link or pairing attempt. Nothing connects
  automatically at launch or after returning to Devices: choose a host and
  press **Connect**, or open **Pair a new device**.
- Pressing **Connect** shows a blocking **Connecting to <device>** popup with an
  animated loading bar until the host answers. The popup closes after 15
  seconds if the host does not respond.
- Inside the Devices screen, D-pad moves, **A** connects the selected host (or
  opens **Pair a new device**) and **X** forgets it. In a sub-screen, B goes
  back, A selects; on the Devices screen, B quits.
- Forgetting a host asks for confirmation. The controller type is chosen only
  while pairing a new host, not for an already paired one.
- The controller view is titled **Connected** and shows the host and the
  connection profile.
- The status bar shows the time, Bluetooth, Wi-Fi and battery state, using the
  nextval colours and the NextUI font when they are available.
- After a host connects, the panel switches off after 10 seconds while the
  gamepad keeps working. Press **Menu+Start** once to wake it; each wake starts
  a new 10-second cycle. The chord is not sent to the host.
- **MENU** tap sends Home/Guide while the controller view is active.
- Rumble in Xbox, PlayStation and Switch modes when the host sends output
  reports; the PC profile has no rumble output report.
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
3. To pair a new host, select **Pair a new device**, press **X** to choose the
   controller type, then open Bluetooth settings on the host and pair with the
   name shown on the Brick: `Brick Controller`, `Xbox Wireless Controller`,
   `Wireless Controller` (PlayStation) or `Pro Controller` (Switch).
4. To use an already paired host, select it and press **Connect**. A blocking
   **Connecting to <device>** popup is shown while the link is established.
   After that, the controller view shows the host and the profile. The panel
   switches off after 10 seconds; press **Menu+Start** once to light it again.
5. Hold **Menu+Start** for 1 s to disconnect and open the Devices screen; the
   current link or pairing attempt is stopped. Press **A** to connect the
   selected host or **X** to forget it after confirming. Press **B** in the
   Devices screen to quit.
6. Tap **MENU** on the controller view to send Home/Guide.

Start is used with Menu for the disconnect and wake chord; otherwise Start and
Select reach the host as normal gamepad buttons. While the Devices screen or a
sub-screen is open, gamepad input is kept away from the host.

## Controller types

The identity is chosen when pairing a new host:

- **PC** is a generic HID gamepad. It works with Windows, macOS,
  Linux/SteamOS, Android and iPadOS, and is the recommended default.
- **Xbox** advertises the Brick as an Xbox Wireless Controller
  (`045E:02FD`), for XInput on Windows and native Xbox support on macOS,
  iPadOS and Android. It reads the host's rumble output report.
- **PlayStation** presents a DualShock 4 (`054C:09CC`, Bluetooth name
  `Wireless Controller`). Hosts that support the DualShock 4 recognise it
  natively, including Windows, macOS, Linux, Android and iPadOS. It reads
  rumble from the host.
- **Switch** presents a Switch Pro Controller (`057E:2009`, Bluetooth name
  `Pro Controller`). It implements the Switch handshake and the 0x30/0x21
  reports, so hosts that support the Pro Controller can use it. It reads
  rumble from the host.

Open **Pair a new device**, press **X** to choose the controller type, then pair
from the host. Choosing another type restarts the Bluetooth session with the
new identity and returns to the **Pairing** screen, ready for the host to pair.
Each host keeps the identity it first paired with, and the Brick uses it again
when that host is connected.

The controller type of an already paired host is not editable from Devices. To
change it, forget the host on both sides, choose the other type in the pairing
screen and pair again.

## Limitations

- **Bluetooth only**. USB HID is not available.
- Rumble is available in Xbox, PlayStation and Switch modes. The PC profile's
  descriptor has no output report, so PC hosts do not vibrate the Brick.
- The PlayStation profile uses the DualShock 4 identity. It does not present a
  DualSense, and does not emulate the lightbar, touchpad, adaptive triggers or
  motion sensors.
- The Switch profile does not send IMU, NFC or IR data, and its single motor
  receives the strongest rumble band rather than HD rumble. Compatibility with
  the Nintendo Switch console itself is not claimed; the profile targets the
  hosts listed above.
- The Brick standard has no analog sticks, gyro, trackpads or back buttons.
  Sticks are reported centred for host compatibility.
- Bluetooth audio and the controller cannot share the radio cleanly. `bluealsa`
  is stopped while the controller runs and restored on exit.
- Wi-Fi and Bluetooth share one antenna on the Brick; expect reduced Wi-Fi
  throughput during a session.
- Windows compatibility is less tested than macOS, across all profiles.
- A host remembers the controller identity it first paired. Switching between
  controller types therefore requires removing the old pairing on that host
  and pairing again.

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

If the panel remains dark after a crash or a forced exit, press
**Menu+Start** once to wake it. `launch.sh` also restores the saved brightness and runs
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

The Switch Pro Controller protocol implementation is ported from
[deck-as-controller](https://github.com/jmedina21/deck-as-controller) (MIT).
The MIT notice is kept in `profiles/switch_pro.rs` and in `CREDITS.md`.
