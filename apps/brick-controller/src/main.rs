// Copyright 2026 ligt (https://github.com/duyquang6/padpod)
// SPDX-License-Identifier: GPL-3.0-only

//! Brick Controller: the TrimUI Brick as a standard Bluetooth HID gamepad.
//!
//! The binary is only a bootstrap and a crash-recovery entry point. Profiles
//! live under `profiles/`, the session under `app/session.rs`, hardware access
//! under `platform/` and the Bluetooth transport under `bluetooth/`. See the
//! **Architecture** section of `AGENTS.md` before adding a controller type.

mod app;
mod bluetooth;
mod hid;
mod i18n;
mod input;
mod paths;
mod platform;
mod profiles;
#[cfg(test)]
mod test_support;
mod ui;

use bluetooth::bluez;
use platform::{display, rumble};

fn main() {
    // `launch.sh` calls this after a crash, when no Rust value got the chance
    // to run its `Drop`. Every step is idempotent and safe when the app exited
    // normally: a missing marker file means there is nothing to restore.
    if std::env::args().nth(1).as_deref() == Some("restore") {
        bluez::restore_system_daemon();
        rumble::stop();
        if let Err(e) = display::restore() {
            eprintln!("display: could not restore the panel: {e}");
        }
        return;
    }
    if let Err(e) = app::run() {
        eprintln!("brick-controller: {e}");
        std::process::exit(1);
    }
}
