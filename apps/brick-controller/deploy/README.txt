Brick Controller
================

Turn a TrimUI Brick or Brick Pro into a Bluetooth or wired USB HID gamepad for
a PC, Mac, Linux/SteamOS machine, Android device or iPad. Nothing has to be
installed on the host.

Requirements
------------
- A TrimUI Brick or Brick Pro running NextUI (tg5040).
- Bluetooth enabled in NextUI. The app opens its Bluetooth session before
  showing Devices, so this is required even for the wired mode.
- A USB-C data cable for the wired PC mode. Use the bottom USB-C port; the top
  port is a host port.
- The Brick Pro uses the same package. Its support is implemented but has not
  yet been validated on Brick Pro hardware.
- The Smart Pro and Smart Pro S are not supported yet. The app refuses to
  start on them.

Installation
------------
Pak Store:
  Open Pak Store > Tools > Brick Controller > Install.

Manual:
  1. Create Tools/tg5040/Brick Controller.pak/ on the SD card.
  2. Extract the archive contents into that folder.
  3. Open Tools > Brick Controller on the handheld.

The launch.sh and brick-controller files must sit directly inside
Tools/tg5040/Brick Controller.pak/.

Pairing
-------
1. Turn Bluetooth on in NextUI.
2. Open Brick Controller > Pair a new device.
3. Press X to choose the controller type: PC, Xbox, PlayStation or Switch.
4. On the host, pair with the name shown on screen:
   - Brick Controller (PC)
   - Brick Controller (Xbox)
   - Brick Controller (PS4)
   - Brick Controller (Switch)
5. The Pairing screen completes when the host connects.

The type is chosen for the next pairing. To change the type of an already
paired host, forget it on both sides and pair again.

Wired PC mode
-------------
1. Open Brick Controller so Devices is visible.
2. Select Wired connection and press A.
3. Connect the bottom USB-C port to the PC with a data cable.
4. The screen switches to Connected and names the host PC (USB).
5. Hold Menu+Start for 1 second to return to Devices.

The wired mode is PC only. It publishes a standard HID gamepad; Windows,
Linux/SteamOS, Android and applications that read HID directly see it. macOS
does not expose a generic HID gamepad to GameController, so use a Bluetooth
profile on Apple hosts.

ADB is unavailable while the wired session runs: the app borrows the USB gadget
NextUI uses for it and restores it on exit, including after a crash.

Controls
--------
Devices screen:
  A  Connect the selected host, open Pair a new device, start Wired connection,
     or open Settings (the label changes to Open on the Settings row)
  X  Forget the selected host (confirmation required; host rows only)
  B  Quit the app

Wired waiting:
  B  Return to Devices

Settings:
  A          Open Settings from Devices
  Up/Down    Select a line
  Left/Right Change the value
  B          Return to Devices

D-Pad Mode sends the D-pad as a D-pad or as the left stick. Rumble strength
scales the host request: Off, Low (30%), Mid (60%), Strong (90%). Both settings
are saved on the SD card and survive a restart.

Connected:
  Menu             Send Home/Guide to the host
  Menu+Start       Hold for 1 second to disconnect and return to Devices
  Menu+Start tap   Wake the panel after it switched off

After 10 seconds, the panel switches off while the gamepad keeps working.
Start and Select are sent to the host as normal gamepad buttons. Input is kept
away from the host while an interface screen is open.

Profiles
--------
PC           Generic HID. Windows, Linux/SteamOS, Android, and Mac/iPad apps
             that read HID directly (Steam, for example).
Xbox         Xbox Wireless Controller identity. XInput on Windows; native
             support on macOS, iPadOS and Android.
PlayStation  DualShock 4 identity. Windows, macOS, Linux, Android, iPadOS.
Switch       Switch Pro Controller identity. macOS, iPadOS and Windows.

On Apple hosts, use Xbox, PlayStation or Switch. macOS does not expose a
generic HID gamepad to its GameController framework. This also applies to the
wired PC mode.

The wired mode is independent of the Bluetooth profile: it always publishes the
PC HID identity and does not change the type stored for a host.

Troubleshooting
---------------
Logs:
  /mnt/SDCARD/.userdata/tg5040/logs/BrickController.txt

If Bluetooth stays stopped after a crash:
  /mnt/SDCARD/.system/tg5040/etc/bluetooth/bt_init.sh start

If ADB does not come back after a wired session, quit and relaunch the Pak. The
launch wrapper runs the binary's restore command, which puts the NextUI USB
gadget back before restarting Bluetooth. A reboot is the fallback.

If the host stops seeing the Brick at all after several wired sessions, unplug
and replug the USB-C cable. Some hosts do not re-enumerate by themselves even
when the Brick side is restored.

If the panel stays dark, press Menu+Start once. To restore NextUI's brightness
and volume settings:
  /mnt/SDCARD/.system/tg5040/bin/syncsettings.elf

If pairing fails, remove the old pairing on both sides, choose the controller
type again in Pairing, and pair once more.

Reporting a problem
-------------------
Brick Controller is a community Pak; NextUI maintainers do not provide support
for it. Open an issue on the project repository and attach
/mnt/SDCARD/.userdata/tg5040/logs/BrickController.txt. The previous run is
kept as BrickController.txt.1 when the log rotates. Remove or mask the
Bluetooth names and addresses of your hosts if you do not want them public.

License and credits
-------------------
Brick Controller is a modified version of Padpod by ligt and its contributors,
released under the GNU GPL v3 with additional terms. See LICENSE,
ADDITIONAL-TERMS.md and CREDITS.md in this folder. The Switch Pro Controller
protocol is ported from deck-as-controller (MIT); its notice is in CREDITS.md.
