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
// The transport itself is Linux-only; the host build compiles the pure pieces
// for their unit tests.
#[cfg(any(target_os = "linux", test))]
mod usb;

use bluetooth::bluez;
use platform::{display, rumble};
use std::process::ExitCode;

fn main() -> ExitCode {
    let restore_only = std::env::args().nth(1).as_deref() == Some("restore");
    // The launcher calls `restore` before sampling Bluetooth and at exit.
    // Direct launches also recover crashes before taking a fresh snapshot.
    // Restore the wired gadget first so ADB is available again.
    #[cfg(target_os = "linux")]
    usb::restore_after_crash();
    let bluetooth = bluez::restore_system_daemon();
    if restore_only {
        rumble::stop();
    }
    if let Err(e) = display::restore() {
        eprintln!("display: could not restore the previous session: {e}");
    }
    if let Err(e) = bluetooth {
        eprintln!("bluez: could not restore the previous session: {e}");
        return ExitCode::FAILURE;
    }
    if restore_only {
        return ExitCode::SUCCESS;
    }
    if let Err(e) = app::run() {
        eprintln!("brick-controller: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
