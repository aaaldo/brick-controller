// Copyright 2026 ligt (https://github.com/duyquang6/padpod)
// SPDX-License-Identifier: GPL-3.0-only

//! Xbox Wireless Controller (model 1708, 045E:02FD), as it speaks over
//! classic Bluetooth.
//!
//! What makes Windows hand it to games as an XInput pad, and what iPadOS and
//! Android list as an Xbox controller. The controller presents one of two
//! descriptors depending on the host it believes it is talking to; this is
//! the Windows one, whose axes - sticks on X/Y and Rx/Ry, triggers on Z/Rz -
//! are also what Linux and SDL expect of a gamepad. Descriptor as xpadneo's
//! documentation dumps it (docs/descriptors/xb1s_windows.md).
//!
//! Reports:
//! - 0x01: sticks (16 bit, 0-65535), triggers (10 bit), hat (1-8, 0 at
//!   rest), ten buttons - A B X Y LB RB View Menu LS RS.
//! - 0x02: the Xbox button, alone.
//! - 0x03 (from the host): rumble, four magnitudes of 0-100.

use super::codec::{Codec as CodecTrait, LinkContext, Output, Reports};
use super::spec::{ProfileSpec, Schedule, UsbId};
use crate::hid::{self, Control};
use crate::input::{Button, Hat, PadState};
use std::time::Duration;

pub const VENDOR: u16 = 0x045E;
pub const PRODUCT: u16 = 0x02FD;
pub const NAME: &str = "Xbox Wireless Controller";

pub const DESCRIPTOR: &[u8] = &[
    0x05, 0x01, 0x09, 0x05, 0xA1, 0x01, 0x85, 0x01, 0x09, 0x01, 0xA1, 0x00, 0x09, 0x30, 0x09, 0x31,
    0x15, 0x00, 0x27, 0xFF, 0xFF, 0x00, 0x00, 0x95, 0x02, 0x75, 0x10, 0x81, 0x02, 0xC0, 0x09, 0x01,
    0xA1, 0x00, 0x09, 0x33, 0x09, 0x34, 0x15, 0x00, 0x27, 0xFF, 0xFF, 0x00, 0x00, 0x95, 0x02, 0x75,
    0x10, 0x81, 0x02, 0xC0, 0x05, 0x01, 0x09, 0x32, 0x15, 0x00, 0x26, 0xFF, 0x03, 0x95, 0x01, 0x75,
    0x0A, 0x81, 0x02, 0x15, 0x00, 0x25, 0x00, 0x75, 0x06, 0x95, 0x01, 0x81, 0x03, 0x05, 0x01, 0x09,
    0x35, 0x15, 0x00, 0x26, 0xFF, 0x03, 0x95, 0x01, 0x75, 0x0A, 0x81, 0x02, 0x15, 0x00, 0x25, 0x00,
    0x75, 0x06, 0x95, 0x01, 0x81, 0x03, 0x05, 0x01, 0x09, 0x39, 0x15, 0x01, 0x25, 0x08, 0x35, 0x00,
    0x46, 0x3B, 0x01, 0x66, 0x14, 0x00, 0x75, 0x04, 0x95, 0x01, 0x81, 0x42, 0x75, 0x04, 0x95, 0x01,
    0x15, 0x00, 0x25, 0x00, 0x35, 0x00, 0x45, 0x00, 0x65, 0x00, 0x81, 0x03, 0x05, 0x09, 0x19, 0x01,
    0x29, 0x0A, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x0A, 0x81, 0x02, 0x15, 0x00, 0x25, 0x00,
    0x75, 0x06, 0x95, 0x01, 0x81, 0x03, 0x05, 0x01, 0x09, 0x80, 0x85, 0x02, 0xA1, 0x00, 0x09, 0x85,
    0x15, 0x00, 0x25, 0x01, 0x95, 0x01, 0x75, 0x01, 0x81, 0x02, 0x15, 0x00, 0x25, 0x00, 0x75, 0x07,
    0x95, 0x01, 0x81, 0x03, 0xC0, 0x05, 0x0F, 0x09, 0x21, 0x85, 0x03, 0xA1, 0x02, 0x09, 0x97, 0x15,
    0x00, 0x25, 0x01, 0x75, 0x04, 0x95, 0x01, 0x91, 0x02, 0x15, 0x00, 0x25, 0x00, 0x75, 0x04, 0x95,
    0x01, 0x91, 0x03, 0x09, 0x70, 0x15, 0x00, 0x25, 0x64, 0x75, 0x08, 0x95, 0x04, 0x91, 0x02, 0x09,
    0x50, 0x66, 0x01, 0x10, 0x55, 0x0E, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x01, 0x91,
    0x02, 0x09, 0xA7, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x01, 0x91, 0x02, 0x65, 0x00,
    0x55, 0x00, 0x09, 0x7C, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x01, 0x91, 0x02, 0xC0,
    0x85, 0x04, 0x05, 0x06, 0x09, 0x20, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x01, 0x81,
    0x02, 0xC0,
];

pub const SPEC: ProfileSpec = ProfileSpec {
    key: "xbox",
    label: "Xbox",
    bluetooth_name: NAME,
    usb: Some(UsbId {
        vendor: VENDOR,
        product: PRODUCT,
        version: 0x0408,
    }),
    descriptor: DESCRIPTOR,
    schedule: Schedule::OnChange {
        min_gap: Duration::from_millis(10),
    },
    rumble: true,
};

/// Report 0x01 behind its DATA header.
pub fn controls(s: &PadState) -> Vec<u8> {
    let mut r = vec![0xA1, 0x01];
    // 0 is up and left, as the handheld's own axes.
    for v in s.sticks {
        r.extend((v as u16 * 257).to_le_bytes());
    }
    for t in s.triggers {
        r.extend(((t as u32 * 1023 / 255) as u16).to_le_bytes());
    }
    // 1 is up, clockwise; 0 is nothing pressed.
    r.push(if s.hat == Hat::Centred {
        0
    } else {
        s.hat.raw() + 1
    });
    // By position, as Xbox games expect: bottom is A whatever the handheld
    // prints on it.
    let order = [
        Button::South,  // A
        Button::East,   // B
        Button::West,   // X
        Button::North,  // Y
        Button::L1,     // LB
        Button::R1,     // RB
        Button::Select, // View
        Button::Start,  // Menu
        Button::L3,
        Button::R3,
    ];
    let bits = order
        .iter()
        .enumerate()
        .fold(0u16, |b, (i, &n)| if s.pressed(n) { b | 1 << i } else { b });
    r.extend(bits.to_le_bytes());
    r
}

/// Report 0x02: the Xbox button.
pub fn guide(s: &PadState) -> Vec<u8> {
    vec![0xA1, 0x02, s.pressed(Button::Home) as u8]
}

/// The rumble a host asked for, 0-255, from report 0x03 (starting at its
/// ID): the strongest of the four motors it enabled. The handheld has one.
pub fn rumble(report: &[u8]) -> Option<u8> {
    if report.first() != Some(&0x03) || report.len() < 6 {
        return None;
    }
    let enabled = report[1] & 0x0F;
    let strongest = report[2..6]
        .iter()
        .enumerate()
        .filter(|(i, _)| enabled & (1 << (3 - i)) != 0)
        .map(|(_, &m)| m)
        .max();
    Some((strongest.unwrap_or(0).min(100) as u16 * 255 / 100) as u8)
}

/// The encoder keeps the last Guide state because report 0x02 is sent only
/// when that button changes, not with every report 0x01.
#[derive(Default)]
pub struct Codec {
    last_home: Option<bool>,
}

impl CodecTrait for Codec {
    fn packets(&mut self, state: &PadState, _elapsed: Duration) -> Reports {
        let home = state.pressed(Button::Home);
        let guide_changed = self.last_home != Some(home);
        self.last_home = Some(home);
        let controls = controls(state);
        if guide_changed {
            Reports::two(&controls, &guide(state))
        } else {
            Reports::one(&controls)
        }
    }

    fn control(&mut self, message: &[u8], _link: &LinkContext, state: &PadState) -> Control {
        // GET_REPORT(input) wants report 0x01 without the DATA header; the
        // generic handler adds that header back.
        let packet = controls(state);
        hid::control(message, &packet[1..])
    }

    fn output(&mut self, report: &[u8], _link: &LinkContext) -> Output {
        Output {
            rumble: rumble(report),
            disconnect: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_one_is_fifteen_bytes_as_the_descriptor_declares() {
        let r = controls(&PadState::default());
        assert_eq!(&r[..2], &[0xA1, 0x01]);
        assert_eq!(r.len(), 2 + 15);
        // Resting sticks sit at the middle, triggers and hat at zero.
        assert_eq!(u16::from_le_bytes([r[2], r[3]]), 128 * 257);
        assert_eq!(&r[10..15], &[0, 0, 0, 0, 0]);
    }

    #[test]
    fn controls_land_where_an_xbox_pad_keeps_them() {
        let mut s = PadState::default();
        s.sticks = [0, 255, 128, 128];
        s.triggers = [255, 0];
        s.hat = Hat::Right;
        s.set_button(Button::South, true); // A
        s.set_button(Button::North, true); // Y
        s.set_button(Button::Start, true); // Menu
        let r = controls(&s);
        assert_eq!(u16::from_le_bytes([r[2], r[3]]), 0);
        assert_eq!(u16::from_le_bytes([r[4], r[5]]), 65535);
        assert_eq!(u16::from_le_bytes([r[10], r[11]]), 1023);
        assert_eq!(r[14], 3, "right is 3 counting from up = 1");
        assert_eq!(u16::from_le_bytes([r[15], r[16]]), 0b1000_1001);
    }

    #[test]
    fn the_xbox_button_has_a_report_of_its_own() {
        let mut s = PadState::default();
        assert_eq!(guide(&s), vec![0xA1, 0x02, 0]);
        s.set_button(Button::Home, true);
        assert_eq!(guide(&s), vec![0xA1, 0x02, 1]);
    }

    #[test]
    fn the_codec_sends_guide_only_when_it_changes() {
        let mut codec = Codec::default();
        let mut s = PadState::default();
        assert_eq!(codec.packets(&s, Duration::ZERO).iter().count(), 2);
        assert_eq!(codec.packets(&s, Duration::ZERO).iter().count(), 1);
        s.set_button(Button::Home, true);
        let reports = codec.packets(&s, Duration::ZERO);
        assert_eq!(reports.iter().count(), 2);
        assert_eq!(reports.iter().last().unwrap()[1], 0x02);
    }

    #[test]
    fn rumble_is_the_strongest_enabled_motor() {
        // Enable strong and weak (low bits), 60 and 20.
        assert_eq!(
            rumble(&[0x03, 0x03, 0, 0, 60, 20, 0xFF, 0, 0xEB]),
            Some(153)
        );
        // A disabled motor does not count.
        assert_eq!(rumble(&[0x03, 0x01, 0, 0, 100, 20, 0, 0, 0]), Some(51));
        assert_eq!(rumble(&[0x03, 0x00, 0, 0, 100, 100, 0, 0, 0]), Some(0));
        assert_eq!(rumble(&[0x01, 0x0F]), None);
    }

    #[test]
    fn the_descriptor_declares_report_one_at_the_size_sent() {
        assert_eq!(crate::test_support::input_bits(DESCRIPTOR, Some(1)), 15 * 8);
    }
}
