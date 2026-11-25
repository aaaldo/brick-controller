// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! The panel backlight, driven through the vendor display node.
//!
//! NextUI and spruce use the same two ioctls: `0x103` returns the current raw
//! brightness, `0x102` sets it. H700 also needs the framebuffer blanked and
//! unblanked, as NextUI does when switching off the built-in panel.
//!
//! A raw value is remembered on disk before the panel is switched off. If the
//! process is killed, `restore` can put the panel back even when no `Backlight`
//! value is left to drop. On a normal exit the in-memory value is enough.

use super::model::Model;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

const DISP: &str = "/dev/disp";
const FB_BLANK: &str = "/sys/class/graphics/fb0/blank";
const DISP_LCD_SET_BRIGHTNESS: libc::c_ulong = 0x102;
const DISP_LCD_GET_BRIGHTNESS: libc::c_ulong = 0x103;
/// A safe visible level when the panel reports nothing usable. It matches
/// NextUI's mid brightness on the Brick.
const DEFAULT_BRIGHTNESS: u16 = 96;
const RESTORE_FILE: &str = "screen-off";

fn restore_path() -> PathBuf {
    crate::paths::data_dir().join(RESTORE_FILE)
}

/// The vendor ioctls take a four-word argument block; only the first word or
/// two are meaningful, but the driver expects the full array.
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

fn set_raw(file: &File, value: u16) -> io::Result<()> {
    let mut args = [0, value as libc::c_ulong, 0, 0];
    if ioctl(file.as_raw_fd(), DISP_LCD_SET_BRIGHTNESS, &mut args) < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Parse the two shapes sysfs uses for the HDMI cable state.
fn parse_hdmi_state(state: &str) -> bool {
    state.trim() == "1" || state.lines().any(|line| line.trim() == "HDMI=1")
}

fn hdmi_connected() -> bool {
    fs::read_to_string("/sys/class/extcon/hdmi/state").is_ok_and(|state| parse_hdmi_state(&state))
        || fs::read_to_string("/sys/class/extcon/hdmi/cable.0/state")
            .is_ok_and(|state| parse_hdmi_state(&state))
}

fn show_panel(file: &File, brightness: u16, h700: bool) -> io::Result<()> {
    // Unblank whenever the node exists, not only when `DEVICE` says H700: a
    // crash restore can run without `DEVICE` and must not leave fb0 dark.
    let unblank = if h700 || std::path::Path::new(FB_BLANK).exists() {
        fs::write(FB_BLANK, "0\n")
    } else {
        Ok(())
    };
    let restored = set_raw(file, brightness);
    if let (Err(unblank_error), Err(brightness_error)) = (&unblank, &restored) {
        eprintln!(
            "display: unblank failed: {unblank_error}; brightness failed: {brightness_error}"
        );
    }
    unblank.and(restored)
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
///
/// `saved` starts as the value captured at open time and is refreshed from the
/// panel just before each switch-off, so a brightness changed by NextUI during
/// the session is preserved. A dark panel at start uses the on-disk marker from
/// the previous run instead.
pub struct Backlight {
    file: Option<File>,
    h700: bool,
    saved: u16,
    off: bool,
}

impl Backlight {
    pub fn open() -> Self {
        let h700 = Model::current() == Model::Rg40xxH;
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
            h700,
            saved,
            off: false,
        }
    }

    /// Darken the panel. Bluetooth keeps running; on H700 the framebuffer is
    /// blanked as well.
    ///
    /// Returns false when the panel could not be switched off: no display
    /// node, HDMI connected on H700, or the crash-recovery marker could not be
    /// written. The caller keeps the ten-second cycle armed so a transient
    /// failure is retried.
    pub fn off(&mut self) -> bool {
        if self.off {
            return true;
        }
        let Some(file) = &self.file else {
            return false;
        };
        if self.h700 && hdmi_connected() {
            return false;
        }
        if let Some(current) = get_raw(file).filter(|value| *value > 0) {
            self.saved = current;
        }
        if let Err(e) = save_restore(self.saved) {
            eprintln!("display: could not save brightness: {e}");
            return false;
        }
        let result = set_raw(file, 0).and_then(|()| {
            if self.h700 {
                fs::write(FB_BLANK, "1\n")
            } else {
                Ok(())
            }
        });
        if let Err(e) = result {
            if show_panel(file, self.saved, self.h700).is_ok() {
                let _ = fs::remove_file(restore_path());
            }
            eprintln!("display: could not turn the panel off: {e}");
            return false;
        }
        self.off = true;
        eprintln!("display: panel off");
        true
    }

    /// Bring the panel back at the brightness it had before.
    ///
    /// Returns false when the panel could not be restored; the caller keeps
    /// its screen-off state and retries on the next wake.
    pub fn on(&mut self) -> bool {
        if !self.off {
            return true;
        }
        if let Some(file) = &self.file {
            if let Err(e) = show_panel(file, self.saved, self.h700) {
                eprintln!("display: could not turn the panel on: {e}");
                return false;
            }
        }
        let _ = fs::remove_file(restore_path());
        self.off = false;
        eprintln!("display: panel on");
        true
    }
}

impl Drop for Backlight {
    fn drop(&mut self) {
        // A panic or early return must not leave the panel dark; `on` is a
        // no-op when it was never switched off.
        let _ = self.on();
    }
}

/// Put the panel back after the app was killed while it was dark. Called by
/// the `restore` subcommand from `launch.sh`; a no-op when no marker exists.
pub fn restore() -> io::Result<()> {
    let Some(saved) = read_restore() else {
        return Ok(());
    };
    let file = OpenOptions::new().read(true).write(true).open(DISP)?;
    show_panel(&file, saved, Model::current() == Model::Rg40xxH)?;
    fs::remove_file(restore_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hdmi_state_accepts_both_sysfs_shapes() {
        assert!(parse_hdmi_state("1\n"));
        assert!(parse_hdmi_state("HDMI=1\n"));
        assert!(parse_hdmi_state(" HDMI=1 \n"));
        assert!(!parse_hdmi_state("0\n"));
        assert!(!parse_hdmi_state("HDMI=0\n"));
        assert!(!parse_hdmi_state(""));
    }

    #[test]
    #[ignore = "requires an RG40XX H panel, with HDMI unplugged"]
    fn rg40xxh_blanks_and_restores() {
        assert_eq!(Model::current(), Model::Rg40xxH);
        assert!(!hdmi_connected());
        let mut panel = Backlight::open();
        let original = get_raw(panel.file.as_ref().unwrap()).unwrap();
        assert!(original > 0);

        assert!(panel.off());
        assert_eq!(get_raw(panel.file.as_ref().unwrap()), Some(0));
        assert_eq!(read_restore(), Some(original));
        assert!(panel.on());
        assert_eq!(get_raw(panel.file.as_ref().unwrap()), Some(original));
        assert!(read_restore().is_none());

        assert!(panel.off());
        restore().unwrap();
        assert_eq!(get_raw(panel.file.as_ref().unwrap()), Some(original));
        assert!(read_restore().is_none());
    }
}
