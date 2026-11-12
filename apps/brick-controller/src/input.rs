// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Transport-neutral state of the physical pad.
//!
//! The evdev reader fills a [`PadState`]; each controller profile turns it into
//! the report its host expects. Nothing here knows about HID, Bluetooth or the
//! screen, which keeps the profiles testable on the host without a device.
//!
//! Button bits keep Linux's positional gamepad numbering. The gaps at 3 and 6
//! are deliberate: Linux maps HID button usage N to `BTN_GAMEPAD + N - 1`, and
//! `BTN_C` sits between East and North. Using explicit discriminants preserves
//! the exact bits the generic PC descriptor documents.

/// The state of every control the Brick exposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PadState {
    buttons: u16,
    pub hat: Hat,
    /// Left X, left Y, right X, right Y; 0-255, centre 128, 0 is up/left.
    pub sticks: [u8; 4],
    /// Left and right trigger; 0-255.
    pub triggers: [u8; 2],
}

impl Default for PadState {
    fn default() -> Self {
        Self {
            buttons: 0,
            hat: Hat::Centred,
            sticks: [128; 4],
            triggers: [0; 2],
        }
    }
}

impl PadState {
    pub fn set_button(&mut self, button: Button, down: bool) {
        if down {
            self.buttons |= button.bit();
        } else {
            self.buttons &= !button.bit();
        }
    }

    pub fn pressed(&self, button: Button) -> bool {
        self.buttons & button.bit() != 0
    }

    /// The button bitset as the generic HID report wants it.
    pub fn buttons_bits(&self) -> u16 {
        self.buttons
    }

    /// Return the state a host should see under `mode`.
    ///
    /// In `LeftStick`, a pressed hat drives the left stick and the hat reports
    /// centred. The physical stick stays in place while the hat is centred;
    /// the hat wins when both move, because a D-pad press is deliberate while
    /// a resting analog stick can drift. The app's own navigation always reads
    /// the untransformed hat.
    pub fn with_dpad_mode(self, mode: DpadMode) -> Self {
        if mode != DpadMode::LeftStick {
            return self;
        }
        let (x, y) = match self.hat {
            Hat::Up => (128, 0),
            Hat::UpRight => (255, 0),
            Hat::Right => (255, 128),
            Hat::DownRight => (255, 255),
            Hat::Down => (128, 255),
            Hat::DownLeft => (0, 255),
            Hat::Left => (0, 128),
            Hat::UpLeft => (0, 0),
            Hat::Centred => return self,
        };
        let mut state = self;
        state.sticks[0] = x;
        state.sticks[1] = y;
        state.hat = Hat::Centred;
        state
    }
}

/// How the D-pad reaches the host.
///
/// The physical hat remains the interface's navigation control; only the state
/// handed to the profiles changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DpadMode {
    /// Report the hat switch as a hat.
    Dpad,
    /// Report the hat as left-stick movement, with the hat centred.
    LeftStick,
}

impl DpadMode {
    pub const ALL: [Self; 2] = [Self::Dpad, Self::LeftStick];
}

/// A physical control, numbered as the generic HID descriptor counts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Button {
    South = 1,
    East = 2,
    North = 4,
    West = 5,
    L1 = 7,
    R1 = 8,
    L2 = 9,
    R2 = 10,
    Select = 11,
    Start = 12,
    Home = 13,
    L3 = 14,
    R3 = 15,
}

impl Button {
    pub const fn bit(self) -> u16 {
        1u16 << (self as u8 - 1)
    }

    /// Recover a button from its descriptor number, for evdev's positional
    /// range. The two gaps (3 and 6) are not buttons and return `None`.
    pub const fn from_number(number: u8) -> Option<Self> {
        match number {
            1 => Some(Self::South),
            2 => Some(Self::East),
            4 => Some(Self::North),
            5 => Some(Self::West),
            7 => Some(Self::L1),
            8 => Some(Self::R1),
            9 => Some(Self::L2),
            10 => Some(Self::R2),
            11 => Some(Self::Select),
            12 => Some(Self::Start),
            13 => Some(Self::Home),
            14 => Some(Self::L3),
            15 => Some(Self::R3),
            _ => None,
        }
    }
}

/// The hat switch's eight directions plus the centred state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Hat {
    Up = 0,
    UpRight = 1,
    Right = 2,
    DownRight = 3,
    Down = 4,
    DownLeft = 5,
    Left = 6,
    UpLeft = 7,
    /// Outside the logical range: the descriptor's Null State says "centred".
    Centred = 8,
}

impl Hat {
    pub const fn raw(self) -> u8 {
        self as u8
    }

    /// Build a hat value from the two evdev hat axes.
    ///
    /// Negative Y is up and negative X is left, as evdev reports them. A
    /// diagonal is kept as a diagonal here; profiles that only know four
    /// directions collapse it themselves.
    pub fn from_axes(x: i32, y: i32) -> Self {
        match (x.signum(), y.signum()) {
            (0, -1) => Self::Up,
            (1, -1) => Self::UpRight,
            (1, 0) => Self::Right,
            (1, 1) => Self::DownRight,
            (0, 1) => Self::Down,
            (-1, 1) => Self::DownLeft,
            (-1, 0) => Self::Left,
            (-1, -1) => Self::UpLeft,
            _ => Self::Centred,
        }
    }
}

/// Scale an axis reading from its device range onto 0-255.
///
/// Both ends are inclusive because evdev reports them that way: a stick at
/// `max` must reach 255, not 254. A malformed range yields the neutral 128
/// rather than a division by zero.
pub fn scale(value: i32, min: i32, max: i32) -> u8 {
    if max <= min {
        return 128;
    }
    let clamped = value.clamp(min, max) as i64;
    ((clamped - min as i64) * 255 / (max as i64 - min as i64)) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_keep_the_descriptor_numbering() {
        assert_eq!(Button::from_number(1), Some(Button::South));
        assert_eq!(Button::from_number(2), Some(Button::East));
        assert_eq!(
            Button::from_number(4),
            Some(Button::North),
            "BTN_C sits between East and North"
        );
        assert_eq!(Button::from_number(5), Some(Button::West));
        assert_eq!(Button::from_number(15), Some(Button::R3));
        assert_eq!(Button::from_number(3), None);
        assert_eq!(Button::South.bit(), 1);
        assert_eq!(Button::North.bit(), 0b1000);
    }

    #[test]
    fn setting_and_clearing_a_button_touches_only_its_bit() {
        let mut s = PadState::default();
        s.set_button(Button::South, true);
        s.set_button(Button::Home, true);
        assert!(s.pressed(Button::South));
        assert!(s.pressed(Button::Home));
        assert!(!s.pressed(Button::East));
        s.set_button(Button::South, false);
        assert!(!s.pressed(Button::South));
        assert_eq!(s.buttons_bits(), Button::Home.bit());
    }

    #[test]
    fn the_hat_counts_clockwise_from_up() {
        assert_eq!(Hat::from_axes(0, -1), Hat::Up);
        assert_eq!(Hat::from_axes(1, -1), Hat::UpRight);
        assert_eq!(Hat::from_axes(1, 0), Hat::Right);
        assert_eq!(Hat::from_axes(0, 1), Hat::Down);
        assert_eq!(Hat::from_axes(-1, 0), Hat::Left);
        assert_eq!(Hat::from_axes(-1, -1), Hat::UpLeft);
        assert_eq!(Hat::from_axes(0, 0), Hat::Centred);
    }

    #[test]
    fn axes_scale_onto_a_byte() {
        assert_eq!(scale(-32768, -32768, 32767), 0);
        assert_eq!(scale(32767, -32768, 32767), 255);
        assert_eq!(scale(0, -32768, 32767), 127);
        assert_eq!(scale(255, 0, 255), 255);
        assert_eq!(scale(999, 0, 255), 255, "clamped");
        assert_eq!(scale(5, 0, 0), 128, "a range that says nothing");
    }

    #[test]
    fn dpad_mode_keeps_the_hat_by_default() {
        let state = PadState {
            hat: Hat::Left,
            ..PadState::default()
        };
        assert_eq!(state.with_dpad_mode(DpadMode::Dpad), state);
    }

    #[test]
    fn left_stick_mode_turns_the_hat_into_the_left_stick() {
        let state = PadState {
            hat: Hat::Up,
            ..PadState::default()
        };
        let state = state.with_dpad_mode(DpadMode::LeftStick);
        assert_eq!(state.hat, Hat::Centred);
        assert_eq!(state.sticks[..2], [128, 0], "up is the top of the axis");

        let state = PadState {
            hat: Hat::DownRight,
            ..PadState::default()
        };
        assert_eq!(
            state.with_dpad_mode(DpadMode::LeftStick).sticks[..2],
            [255, 255]
        );
    }

    #[test]
    fn a_centred_hat_keeps_the_physical_stick() {
        let mut state = PadState::default();
        state.sticks[0] = 64;
        state.sticks[1] = 192;
        assert_eq!(state.with_dpad_mode(DpadMode::LeftStick), state);
    }

    #[test]
    fn the_hat_wins_when_the_stick_moves_too() {
        let mut state = PadState {
            hat: Hat::Down,
            ..PadState::default()
        };
        state.sticks[0] = 64;
        state.sticks[1] = 192;
        assert_eq!(
            state.with_dpad_mode(DpadMode::LeftStick).sticks[..2],
            [128, 255],
            "a deliberate D-pad press overrides the analog stick"
        );
    }
}
