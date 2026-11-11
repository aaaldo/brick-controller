# Credits

Padpod stands on other people's work: device documentation, the Linux
drivers that record what these controllers say about themselves, and
open-source tools. Thank you to everyone below.

## In this repository

| Work | License | What Padpod uses | Where |
|---|---|---|---|
| [Nunito](https://github.com/googlefonts/nunito), by The Nunito Project Authors | SIL Open Font License 1.1 | The interface font, subset | `crates/brick/assets/` (with `OFL.txt`) |
| [Apostrophe](https://github.com/Helaas/Apostrophe), by Helaas | MIT | NextUI layout metrics and widget behaviour used as the design reference | `apps/brick-controller/src/ui.rs` |
| [deck-as-controller](https://github.com/jmedina21/deck-as-controller), by jmedina21 | MIT | Switch Pro Controller protocol, descriptor and subcommand handling, ported to Rust | `apps/brick-controller/src/profiles/switch_pro.rs` |

The DualShock 4 and Xbox Wireless Controller descriptors in
`apps/brick-controller/src/profiles/dualshock4.rs` and
`apps/brick-controller/src/profiles/xbox.rs` are what those controllers report
about themselves, as recorded by the Linux kernel's `hid-sony.c` and
`hid-playstation.c` and by
[atar-axis/xpadneo](https://github.com/atar-axis/xpadneo)'s descriptor dumps.

### MIT notice for deck-as-controller

Copyright (c) 2026 jmedina21

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## References

- The Linux kernel's `hid-playstation.c` and `hid-sony.c` - DualShock 4 report
  layouts and calibration.
- [atar-axis/xpadneo](https://github.com/atar-axis/xpadneo) - the Xbox
  Wireless Controller over Bluetooth.
- [dekuNukem/Nintendo_Switch_Reverse_Engineering](https://github.com/dekuNukem/Nintendo_Switch_Reverse_Engineering) -
  Switch Pro Controller reports, subcommands and SPI layout.
- The Linux kernel's `hid-nintendo.c` and SDL's HIDAPI Switch driver - the
  input report mode and subcommand handshake.
- [matlo/GIMX](https://github.com/matlo/GIMX) (GPL-3.0) - DualShock 4
  emulation over Bluetooth. Read, not copied.
- The Bluetooth HID profile specification - the service record, and the
  control and interrupt channels.
- The SPRUCE theme of spruceOS - the colours the interface uses.

## Tools Padpod runs

Not part of this repository; Padpod uses them as the handheld provides them.

- [BlueZ](http://www.bluez.org/) - the Bluetooth stack, driven over D-Bus.
- [spruceOS](https://github.com/spruceUI/spruceOS) - the handheld's firmware
  the app runs on, and whose helper scripts it calls.
- The Rust crates in `Cargo.lock` (MIT, Apache-2.0, Zlib, Unicode-3.0), each
  under its own license.

The font's license is in `crates/brick/assets/OFL.txt`.
