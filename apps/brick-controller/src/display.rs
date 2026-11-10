// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! The Brick's panel backlight, driven through the vendor display node.
//!
//! NextUI and spruce use the same two ioctls: `0x103` returns the current raw
//! brightness, `0x102` sets it. Writing zero is the only way to switch the
//! panel off; the framebuffer stays usable, so the app can keep serving HID
//! reports while the screen is dark.
//!
//! A raw value is remembered on disk before the panel is switched off. If the
//! process is killed, `restore` can put the panel back even when no `Backlight`
//! value is left to drop. On a normal exit the in-memory value is enough.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

const DISP: &str = "/dev/disp";
const DISP_LCD_SET_BRIGHTNESS: libc::c_ulong = 0x102;
const DISP_LCD_GET_BRIGHTNESS: libc::c_ulong = 0x103;
/// A safe visible level when the panel reports nothing usable. It matches
/// NextUI's mid brightness on the Brick.
const DEFAULT_BRIGHTNESS: u16 = 96;
const RESTORE_FILE: &str = "screen-off";

fn restore_path() -> PathBuf {
    crate::data_dir().join(RESTORE_FILE)
}

fn ioctl(fd: libc::c_int, request: libc::c_ulong, args: &mut [libc::c_ulong; 4]) -> libc::c_int {
    // The ioctl request is an `int` on Linux and an `unsigned long` on the
    // host used for tests; `as _` lets each target pick its own type.
    unsafe { libc::ioctl(fd, request as _, args.as_mut_ptr()) }
}

fn get_raw(file: &File) -> Option<u16> {
    let mut args = [0; 4];
    let value = ioctl(file.as_raw_fd(), DISP_LCD_GET_BRIGHTNESS, &mut args);
    (0..=u8::MAX as libc::c_int)
        .contains(&value)
        .then_some(value as u16)
}

fn set_raw(file: &File, value: u16) {
    let mut args = [0, value as libc::c_ulong, 0, 0];
    let _ = ioctl(file.as_raw_fd(), DISP_LCD_SET_BRIGHTNESS, &mut args);
}

fn read_restore() -> Option<u16> {
    fs::read_to_string(restore_path())
        .ok()?
        .trim()
        .parse::<u16>()
        .ok()
        .filter(|value| *value <= u8::MAX as u16)
}

fn save_restore(value: u16) -> io::Result<()> {
    use std::io::Write;
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(restore_path())?;
    write!(file, "{value}")?;
    // The marker is only useful if it survives the process; sync it before
    // the panel goes dark.
    file.sync_all()
}

/// Switch the panel off and back on, remembering the brightness it had.
pub struct Backlight {
    file: Option<File>,
    saved: u16,
    off: bool,
}

impl Backlight {
    pub fn open() -> Self {
        let file = OpenOptions::new().read(true).write(true).open(DISP).ok();
        let current = file.as_ref().and_then(get_raw);
        // A marker left by a crash says what to restore; a usable current
        // value is fresher, so it wins and the stale marker is discarded.
        let saved = match (current, read_restore()) {
            (Some(value), _) if value > 0 => {
                let _ = fs::remove_file(restore_path());
                value
            }
            (_, Some(value)) => value,
            _ => DEFAULT_BRIGHTNESS,
        };
        if file.is_none() {
            eprintln!("display: {DISP} not available; the panel is left alone");
        }
        Self {
            file,
            saved,
            off: false,
        }
    }

    /// Darken the panel, leaving the framebuffer and the Bluetooth link up.
    pub fn off(&mut self) {
        if self.off {
            return;
        }
        let Some(file) = &self.file else {
            return;
        };
        if let Err(e) = save_restore(self.saved) {
            // Restoring after a crash would be nicer, but a missing marker is
            // no reason to keep the panel lit.
            eprintln!("display: could not save brightness: {e}");
        }
        set_raw(file, 0);
        self.off = true;
        eprintln!("display: panel off");
    }

    /// Bring the panel back at the brightness it had before.
    pub fn on(&mut self) {
        if !self.off {
            return;
        }
        if let Some(file) = &self.file {
            set_raw(file, self.saved);
        }
        let _ = fs::remove_file(restore_path());
        self.off = false;
        eprintln!("display: panel on");
    }
}

impl Drop for Backlight {
    fn drop(&mut self) {
        self.on();
    }
}

/// Put the panel back after the app was killed while it was dark. Called by
/// the `restore` subcommand from `launch.sh`; a no-op when no marker exists.
pub fn restore() -> io::Result<()> {
    let Some(saved) = read_restore() else {
        return Ok(());
    };
    let file = OpenOptions::new().read(true).write(true).open(DISP)?;
    set_raw(&file, saved);
    fs::remove_file(restore_path())?;
    Ok(())
}
