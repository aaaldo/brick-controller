// Copyright 2026 ligt (https://github.com/duyquang6/padpod)
// SPDX-License-Identifier: GPL-3.0-only

//! The handheld's own controls, read as a whole state rather than as presses.
//!
//! The gamepad usages are numbered in Linux's positional order (BTN_SOUTH,
//! BTN_EAST, ...), so most keys map onto a button by their offset from
//! BTN_SOUTH. Two do not, because the handheld does not report positionally:
//! spruce's BrickPro.cfg (measured on hardware) has X - the *top* button - on
//! 308, which Linux calls BTN_WEST, and Y - the left one - on 307, BTN_NORTH.
//!
//! The same file lists the right stick's axes crossed (ABS_RY as X), but on
//! the device they are not: tested on a PC, pushing right moved ABS_RX.
//!
//! This is the platform input reader, not a controller profile. The pure pad
//! state it fills lives in `input.rs`; profiles translate that state into
//! their own reports.

use crate::input::{scale, Button, Hat, PadState};
use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::{AsRawFd, RawFd};

const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const EV_ABS: u16 = 3;

const BTN_SOUTH: u16 = 304;
/// The Brick's front function keys, later mapped to L3 and R3.
///
/// `BTN_THUMBR` is the last code in the contiguous gamepad range, so the
/// mapper uses `BTN_SOUTH..=BTN_THUMBR` to recognise every button without
/// listing each one; the two exceptions are handled by name below.
const BTN_THUMBR: u16 = 318;
/// The handheld's X (top) and Y (left) buttons.
const KEY_X: u16 = 308;
const KEY_Y: u16 = 307;

const ABS_X: u16 = 0;
const ABS_Y: u16 = 1;
const ABS_Z: u16 = 2;
const ABS_RX: u16 = 3;
const ABS_RY: u16 = 4;
const ABS_RZ: u16 = 5;
const ABS_HAT0X: u16 = 16;
const ABS_HAT0Y: u16 = 17;

/// A trigger counts as a button press past this share of its travel.
///
/// The host gets both the analogue axis and a digital L2/R2 bit, because some
/// games only read the button. A quarter of the travel is early enough to feel
/// immediate without making a light resting touch register.
const TRIGGER_BUTTON_AT: u8 = 64;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct InputEvent {
    tv_sec: i64,
    tv_usec: i64,
    kind: u16,
    code: u16,
    value: i32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct AbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

const EVENT_SIZE: usize = std::mem::size_of::<InputEvent>();

/// EVIOCGABS(abs): _IOR('E', 0x40 + abs, struct input_absinfo).
fn eviocgabs(abs: u16) -> libc::c_ulong {
    (2 << 30)
        | ((std::mem::size_of::<AbsInfo>() as libc::c_ulong) << 16)
        | (b'E' as libc::c_ulong) << 8
        | (0x40 + abs as libc::c_ulong)
}

pub use brick::input::pad_path;

/// Turns evdev events into gamepad state. Separate from the file so the
/// mapping can be tested without a device.
#[derive(Clone)]
pub struct Mapper {
    pub state: PadState,
    /// Set when Menu and Start were down together, even if both were released
    /// before the next poll. Cleared by `take_combo_tap`.
    combo_tap: bool,
    /// (min, max) for ABS_X..=ABS_RZ.
    ranges: [(i32, i32); 6],
    hat: (i32, i32),
}

impl Default for Mapper {
    fn default() -> Self {
        Self {
            state: PadState::default(),
            combo_tap: false,
            ranges: [
                (-32768, 32767),
                (-32768, 32767),
                (0, 255),
                (-32768, 32767),
                (-32768, 32767),
                (0, 255),
            ],
            hat: (0, 0),
        }
    }
}

impl Mapper {
    pub fn with_ranges(ranges: [(i32, i32); 6]) -> Self {
        Self {
            ranges,
            ..Self::default()
        }
    }

    /// Whether Menu+Start was seen down together since the last call.
    pub fn take_combo_tap(&mut self) -> bool {
        std::mem::take(&mut self.combo_tap)
    }

    /// Apply one event. True when it completes a frame (SYN_REPORT), which is
    /// when a report should go out.
    ///
    /// The mapper deliberately works on the whole state instead of emitting
    /// press/release events: the HID report always carries every button and
    /// axis, so intermediate events never need to reach the host.
    fn apply(&mut self, kind: u16, code: u16, value: i32) -> bool {
        match kind {
            EV_SYN => return code == 0,
            EV_KEY if (BTN_SOUTH..=BTN_THUMBR).contains(&code) && value != 2 => {
                // Most Brick keys follow Linux's positional numbering, but X
                // (top) and Y (left) are reported by letter instead. Mapping
                // by position keeps the host's face buttons where the player
                // expects them, whatever is printed on the shell.
                let button = if code == KEY_X {
                    Button::North
                } else if code == KEY_Y {
                    Button::West
                } else {
                    let Some(button) = Button::from_number((code - BTN_SOUTH + 1) as u8) else {
                        return false;
                    };
                    button
                };
                self.state.set_button(button, value != 0);
                // Menu alone is Home; Menu+Start is the disconnect/wake
                // chord. The tap is remembered even if both are released
                // before the next poll, so a quick tap is not lost.
                if self.state.pressed(Button::Home) && self.state.pressed(Button::Start) {
                    self.combo_tap = true;
                }
            }
            EV_ABS => {
                let scaled = |i: usize| {
                    let (min, max) = self.ranges[i];
                    scale(value, min, max)
                };
                match code {
                    ABS_X => self.state.sticks[0] = scaled(0),
                    ABS_Y => self.state.sticks[1] = scaled(1),
                    ABS_RX => self.state.sticks[2] = scaled(3),
                    ABS_RY => self.state.sticks[3] = scaled(4),
                    ABS_Z => {
                        let v = scaled(2);
                        self.state.triggers[0] = v;
                        self.state.set_button(Button::L2, v >= TRIGGER_BUTTON_AT);
                    }
                    ABS_RZ => {
                        let v = scaled(5);
                        self.state.triggers[1] = v;
                        self.state.set_button(Button::R2, v >= TRIGGER_BUTTON_AT);
                    }
                    ABS_HAT0X => self.hat.0 = value,
                    ABS_HAT0Y => self.hat.1 = value,
                    _ => {}
                }
                self.state.hat = Hat::from_axes(self.hat.0, self.hat.1);
            }
            _ => {}
        }
        false
    }
}

/// The evdev reader and its current mapped state.
///
/// `mapper` is public because the session reads the state to build a report;
/// a future refinement is a `PadState` accessor plus a small event type, so
/// callers cannot mutate the mapper by accident.
pub struct Pad {
    file: File,
    pub mapper: Mapper,
    /// Each (type, code) is logged the first time it arrives, so the log says
    /// what the hardware actually sends without a line per event.
    seen: std::collections::HashSet<(u16, u16)>,
}

impl Pad {
    /// Opened non-exclusively, as Truepod and Anipod do: spruce's own
    /// watchdogs read the same node for volume and brightness.
    pub fn open(path: &str) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)?;
        let mut ranges = Mapper::default().ranges;
        for (i, slot) in ranges.iter_mut().enumerate() {
            let mut info = AbsInfo::default();
            if unsafe { libc::ioctl(file.as_raw_fd(), eviocgabs(i as u16) as _, &mut info) } == 0
                && info.maximum > info.minimum
            {
                *slot = (info.minimum, info.maximum);
            }
        }
        eprintln!("pad: {path}, axis ranges {ranges:?}");
        let mut pad = Self {
            file,
            mapper: Mapper::with_ranges(ranges),
            seen: Default::default(),
        };
        // Nothing else reads the gamepad node while the app is away, so events
        // from the launcher can still be queued here. Replaying them could
        // look like a fresh Menu+Start chord; start from what happens after
        // launch instead.
        pad.drain()?;
        Ok(pad)
    }

    fn drain(&mut self) -> io::Result<()> {
        let mut buf = [0u8; EVENT_SIZE * 64];
        loop {
            match self.file.read(&mut buf) {
                Ok(0) => return Ok(()),
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    }

    /// Read whatever is waiting. True if a frame completed and the state may
    /// have changed. Call only after poll() says the node is readable.
    ///
    /// The read is batched: one `read` can return several `input_event`
    /// records, and only the final SYN_REPORT means the state is consistent
    /// enough to send. The first unknown (type, code) pair is logged so a new
    /// firmware revision can be adapted from the log.
    pub fn read(&mut self) -> io::Result<bool> {
        let mut buf = [0u8; EVENT_SIZE * 16];
        let n = match self.file.read(&mut buf) {
            Ok(n) => n,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) =>
            {
                return Ok(false)
            }
            Err(e) => return Err(e),
        };
        let mut frame = false;
        for chunk in buf[..n].as_chunks::<EVENT_SIZE>().0 {
            let ev: InputEvent =
                unsafe { std::ptr::read_unaligned(chunk.as_ptr() as *const InputEvent) };
            if ev.kind != EV_SYN && self.seen.insert((ev.kind, ev.code)) {
                eprintln!(
                    "pad: first event type {} code {} value {}",
                    ev.kind, ev.code, ev.value
                );
            }
            frame |= self.mapper.apply(ev.kind, ev.code, ev.value);
        }
        Ok(frame)
    }
}

impl AsRawFd for Pad {
    fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(m: &mut Mapper, events: &[(u16, u16, i32)]) -> bool {
        events
            .iter()
            .fold(false, |frame, &(k, c, v)| m.apply(k, c, v) | frame)
    }

    #[test]
    fn keys_map_by_position_onto_the_gamepad_buttons() {
        let mut m = Mapper::default();
        assert!(feed(
            &mut m,
            &[(EV_KEY, 304, 1), (EV_KEY, 316, 1), (EV_SYN, 0, 0)]
        ));
        assert!(m.state.pressed(Button::South));
        assert!(m.state.pressed(Button::Home));
        feed(&mut m, &[(EV_KEY, 304, 0)]);
        assert!(!m.state.pressed(Button::South));
    }

    #[test]
    fn menu_and_start_together_set_a_tap_even_when_released_before_the_poll() {
        let mut m = Mapper::default();
        feed(&mut m, &[(EV_KEY, 316, 1)]);
        assert!(!m.take_combo_tap());
        feed(
            &mut m,
            &[(EV_KEY, 315, 1), (EV_KEY, 315, 0), (EV_KEY, 316, 0)],
        );
        assert!(m.take_combo_tap());
        assert!(!m.take_combo_tap(), "the tap is consumed once");
    }

    #[test]
    fn x_and_y_go_by_where_they_sit_not_by_their_codes() {
        let mut m = Mapper::default();
        feed(&mut m, &[(EV_KEY, KEY_X, 1)]);
        assert!(m.state.pressed(Button::North), "X is the top button");
        assert!(!m.state.pressed(Button::West));
        feed(&mut m, &[(EV_KEY, KEY_X, 0), (EV_KEY, KEY_Y, 1)]);
        assert!(m.state.pressed(Button::West), "Y is the left one");
    }

    #[test]
    fn the_right_stick_is_rx_across_and_ry_down() {
        let mut m = Mapper::default();
        feed(&mut m, &[(EV_ABS, ABS_RX, 32767), (EV_ABS, ABS_RY, -32768)]);
        assert_eq!(m.state.sticks[2], 255, "ABS_RX is right-stick X");
        assert_eq!(m.state.sticks[3], 0, "ABS_RY is right-stick Y");
    }

    #[test]
    fn autorepeat_does_not_change_anything() {
        let mut m = Mapper::default();
        feed(&mut m, &[(EV_KEY, 305, 1), (EV_KEY, 305, 2)]);
        assert!(m.state.pressed(Button::East));
    }

    #[test]
    fn keys_outside_the_gamepad_range_are_ignored() {
        let mut m = Mapper::default();
        feed(&mut m, &[(EV_KEY, 115, 1), (EV_KEY, 61, 1)]);
        assert_eq!(m.state, PadState::default());
    }

    #[test]
    fn front_function_keys_map_to_l3_and_r3() {
        let mut m = Mapper::default();
        feed(&mut m, &[(EV_KEY, 317, 1)]);
        assert!(m.state.pressed(Button::L3));
        feed(&mut m, &[(EV_KEY, 317, 0), (EV_KEY, 318, 1)]);
        assert!(!m.state.pressed(Button::L3));
        assert!(m.state.pressed(Button::R3));
        feed(&mut m, &[(EV_KEY, 318, 0)]);
        assert!(!m.state.pressed(Button::R3));
    }

    #[test]
    fn the_dpad_hat_follows_both_axes() {
        let mut m = Mapper::default();
        feed(&mut m, &[(EV_ABS, ABS_HAT0Y, -1)]);
        assert_eq!(m.state.hat, Hat::Up);
        feed(&mut m, &[(EV_ABS, ABS_HAT0X, 1)]);
        assert_eq!(m.state.hat, Hat::UpRight, "up-right");
        feed(&mut m, &[(EV_ABS, ABS_HAT0Y, 0), (EV_ABS, ABS_HAT0X, 0)]);
        assert_eq!(m.state.hat, Hat::Centred);
    }

    #[test]
    fn triggers_report_travel_and_a_button_past_a_quarter() {
        let mut m = Mapper::default();
        feed(&mut m, &[(EV_ABS, ABS_Z, 255)]);
        assert_eq!(m.state.triggers[0], 255);
        assert!(m.state.pressed(Button::L2));
        feed(&mut m, &[(EV_ABS, ABS_Z, 0), (EV_ABS, ABS_RZ, 30)]);
        assert!(!m.state.pressed(Button::L2));
        assert!(!m.state.pressed(Button::R2), "a light touch is not a press");
    }

    #[test]
    fn sticks_scale_from_the_device_range() {
        let mut m = Mapper::with_ranges([
            (0, 4095),
            (0, 4095),
            (0, 255),
            (0, 4095),
            (0, 4095),
            (0, 255),
        ]);
        feed(
            &mut m,
            &[
                (EV_ABS, ABS_X, 0),
                (EV_ABS, ABS_Y, 4095),
                (EV_ABS, ABS_RX, 2048),
            ],
        );
        assert_eq!(m.state.sticks[..3], [0, 255, 127]);
    }

    #[test]
    fn eviocgabs_matches_the_kernel_header() {
        // EVIOCGABS(ABS_X) on 64-bit Linux.
        assert_eq!(eviocgabs(0), 0x8018_4540);
    }
}
