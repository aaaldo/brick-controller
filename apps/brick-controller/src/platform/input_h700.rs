// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! H700 (Anbernic RG40XX H) evdev translation.
//!
//! The H700 kernel does not use the Brick's positional layout: the face
//! buttons are swapped, the sticks report X/Y on `ABS_Z`/`ABS_RX` and
//! `ABS_RY`/`ABS_RZ`, and L2/R2 are digital buttons. This module translates
//! the codes measured on an RG40XX H into the normalized Brick layout, so the
//! shared `Mapper` in `input_trimui` and every controller profile stay
//! unchanged.
//!
//! The raw constants are named after the physical labels, not Linux's
//! positional names, because the two disagree on this shell. They are the
//! values observed in the device log; a firmware change is expected to show
//! up as an unknown first event in the log.

use super::input_trimui::{
    ABS_RX, ABS_RY, ABS_RZ, ABS_X, ABS_Y, ABS_Z, BTN_EAST, BTN_MODE, BTN_SELECT, BTN_SOUTH,
    BTN_START, BTN_THUMBL, BTN_THUMBR, BTN_TL, BTN_TR, EV_ABS, EV_KEY, KEY_X, KEY_Y,
};

// RG40XX H raw codes, labelled by what is printed on the shell.
const H700_A: u16 = 304;
const H700_B: u16 = 305;
const H700_Y: u16 = 306;
const H700_X: u16 = 307;
const H700_L1: u16 = 308;
const H700_R1: u16 = 309;
const H700_SELECT: u16 = 310;
const H700_START: u16 = 311;
const H700_MENU: u16 = 312;
const H700_L3: u16 = 313;
const H700_L2: u16 = 314;
const H700_R2: u16 = 315;
const H700_R3: u16 = 316;

const H700_LEFT_X: u16 = ABS_Z;
const H700_LEFT_Y: u16 = ABS_RX;
const H700_RIGHT_X: u16 = ABS_RY;
const H700_RIGHT_Y: u16 = ABS_RZ;

/// Find the `ANBERNIC-keys` event node.
///
/// The name was measured on the RG40XX H. Unlike the Brick reader, there is no
/// hard-coded event-number fallback: opening the wrong node would silently
/// feed unrelated events into the mapper.
pub(crate) fn pad_path() -> Result<String, String> {
    let devices = std::fs::read_to_string("/proc/bus/input/devices")
        .map_err(|e| format!("could not read /proc/bus/input/devices: {e}"))?;
    brick::input::parse_pad_by_name(&devices, "ANBERNIC-keys")
        .map(|event| format!("/dev/input/{event}"))
        .ok_or_else(|| "no ANBERNIC-keys input device found".into())
}

/// Translate one raw H700 event into the normalized Brick layout.
///
/// The digital L2/R2 buttons represent the full travel of the trigger axes,
/// which is what the Brick reports from its analogue triggers.
pub(crate) fn normalize(kind: u16, code: u16, value: i32) -> (u16, u16, i32) {
    match (kind, code) {
        (EV_KEY, H700_A) => (EV_KEY, BTN_EAST, value),
        (EV_KEY, H700_B) => (EV_KEY, BTN_SOUTH, value),
        (EV_KEY, H700_Y) => (EV_KEY, KEY_Y, value),
        (EV_KEY, H700_X) => (EV_KEY, KEY_X, value),
        (EV_KEY, H700_L1) => (EV_KEY, BTN_TL, value),
        (EV_KEY, H700_R1) => (EV_KEY, BTN_TR, value),
        (EV_KEY, H700_SELECT) => (EV_KEY, BTN_SELECT, value),
        (EV_KEY, H700_START) => (EV_KEY, BTN_START, value),
        (EV_KEY, H700_MENU) => (EV_KEY, BTN_MODE, value),
        (EV_KEY, H700_L3) => (EV_KEY, BTN_THUMBL, value),
        (EV_KEY, H700_L2) => (EV_ABS, ABS_Z, if value == 0 { 0 } else { 255 }),
        (EV_KEY, H700_R2) => (EV_ABS, ABS_RZ, if value == 0 { 0 } else { 255 }),
        (EV_KEY, H700_R3) => (EV_KEY, BTN_THUMBR, value),
        (EV_ABS, H700_LEFT_X) => (EV_ABS, ABS_X, value),
        (EV_ABS, H700_LEFT_Y) => (EV_ABS, ABS_Y, value),
        (EV_ABS, H700_RIGHT_X) => (EV_ABS, ABS_RX, value),
        (EV_ABS, H700_RIGHT_Y) => (EV_ABS, ABS_RY, value),
        _ => (kind, code, value),
    }
}

/// The evdev axis whose range fills `ranges[index]` on H700.
///
/// Each stick slot reads its range from the same physical axis that feeds it.
/// Slots 2 and 5 are digital triggers and keep the default 0..=255 range.
pub(crate) fn axis_for_slot(index: usize) -> Option<u16> {
    match index {
        0 => Some(H700_LEFT_X),
        1 => Some(H700_LEFT_Y),
        3 => Some(H700_RIGHT_X),
        4 => Some(H700_RIGHT_Y),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_buttons_follow_the_shell_labels() {
        assert_eq!(normalize(EV_KEY, H700_A, 1), (EV_KEY, BTN_EAST, 1));
        assert_eq!(normalize(EV_KEY, H700_B, 1), (EV_KEY, BTN_SOUTH, 1));
        assert_eq!(normalize(EV_KEY, H700_X, 1), (EV_KEY, KEY_X, 1));
        assert_eq!(normalize(EV_KEY, H700_Y, 1), (EV_KEY, KEY_Y, 1));
    }

    #[test]
    fn shoulders_menu_and_click_buttons_map_to_the_normalized_range() {
        assert_eq!(normalize(EV_KEY, H700_L1, 1), (EV_KEY, BTN_TL, 1));
        assert_eq!(normalize(EV_KEY, H700_R1, 1), (EV_KEY, BTN_TR, 1));
        assert_eq!(normalize(EV_KEY, H700_SELECT, 1), (EV_KEY, BTN_SELECT, 1));
        assert_eq!(normalize(EV_KEY, H700_START, 1), (EV_KEY, BTN_START, 1));
        assert_eq!(normalize(EV_KEY, H700_MENU, 1), (EV_KEY, BTN_MODE, 1));
        assert_eq!(normalize(EV_KEY, H700_L3, 1), (EV_KEY, BTN_THUMBL, 1));
        assert_eq!(normalize(EV_KEY, H700_R3, 1), (EV_KEY, BTN_THUMBR, 1));
    }

    #[test]
    fn digital_triggers_report_full_travel() {
        assert_eq!(normalize(EV_KEY, H700_L2, 1), (EV_ABS, ABS_Z, 255));
        assert_eq!(normalize(EV_KEY, H700_L2, 0), (EV_ABS, ABS_Z, 0));
        assert_eq!(normalize(EV_KEY, H700_R2, 1), (EV_ABS, ABS_RZ, 255));
        assert_eq!(normalize(EV_KEY, H700_R2, 0), (EV_ABS, ABS_RZ, 0));
    }

    #[test]
    fn physical_stick_axes_map_to_their_normalized_axes() {
        assert_eq!(normalize(EV_ABS, ABS_Z, -4096), (EV_ABS, ABS_X, -4096));
        assert_eq!(normalize(EV_ABS, ABS_RX, -4096), (EV_ABS, ABS_Y, -4096));
        assert_eq!(normalize(EV_ABS, ABS_RY, 4096), (EV_ABS, ABS_RX, 4096));
        assert_eq!(normalize(EV_ABS, ABS_RZ, -4096), (EV_ABS, ABS_RY, -4096));
    }

    #[test]
    fn unknown_events_pass_through() {
        assert_eq!(normalize(EV_KEY, 115, 1), (EV_KEY, 115, 1));
        assert_eq!(normalize(0, 0, 0), (0, 0, 0));
    }

    #[test]
    fn stick_ranges_come_from_the_physical_axes() {
        assert_eq!(axis_for_slot(0), Some(ABS_Z));
        assert_eq!(axis_for_slot(1), Some(ABS_RX));
        assert_eq!(axis_for_slot(3), Some(ABS_RY));
        assert_eq!(axis_for_slot(4), Some(ABS_RZ));
        assert_eq!(axis_for_slot(2), None, "the left trigger keeps 0..=255");
        assert_eq!(axis_for_slot(5), None, "the right trigger keeps 0..=255");
    }
}
