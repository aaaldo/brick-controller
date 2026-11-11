// Copyright 2026 ligt (https://github.com/duyquang6/padpod)
// SPDX-License-Identifier: GPL-3.0-only

//! The generic PC gamepad: a plain HID report with no report ID.
//!
//! The report descriptor is what most desktop operating systems and SDL
//! already understand. Its axes follow Linux's own Xbox driver, which is the
//! layout SDL and browsers expect of a gamepad they do not know: sticks on
//! X/Y and Rx/Ry, triggers on Z/Rz.
//!
//! The report has no report ID, so the interrupt packet is the DATA header
//! `0xA1` followed by the nine bytes described below:
//!
//! | byte | contents |
//! |---|---|
//! | 0-1 | buttons 1-16, one bit each |
//! | 2 | hat switch (low nibble, 8 = centred), high nibble unused |
//! | 3-6 | left stick X, Y; right stick X, Y; 0-255, centre 128 |
//! | 7-8 | left and right trigger; 0-255 |

use super::codec::{Codec as CodecTrait, Output, Reports};
use super::spec::{ProfileSpec, Schedule};
use crate::hid::{self, Control, DATA_INPUT};
use crate::input::PadState;
use std::time::Duration;

pub const NAME: &str = "Brick Controller (PC)";

pub const DESCRIPTOR: &[u8] = &[
    0x05, 0x01, //       Usage Page (Generic Desktop)
    0x09, 0x05, //       Usage (Game Pad)
    0xA1, 0x01, //       Collection (Application)
    0x05, 0x09, //         Usage Page (Button)
    0x19, 0x01, //         Usage Minimum (1)
    0x29, 0x10, //         Usage Maximum (16)
    0x15, 0x00, //         Logical Minimum (0)
    0x25, 0x01, //         Logical Maximum (1)
    0x75, 0x01, //         Report Size (1)
    0x95, 0x10, //         Report Count (16)
    0x81, 0x02, //         Input (Data, Variable, Absolute)
    0x05, 0x01, //         Usage Page (Generic Desktop)
    0x09, 0x39, //         Usage (Hat Switch)
    0x15, 0x00, //         Logical Minimum (0)
    0x25, 0x07, //         Logical Maximum (7)
    0x35, 0x00, //         Physical Minimum (0)
    0x46, 0x3B, 0x01, //   Physical Maximum (315)
    0x65, 0x14, //         Unit (degrees)
    0x75, 0x04, //         Report Size (4)
    0x95, 0x01, //         Report Count (1)
    0x81, 0x42, //         Input (Data, Variable, Absolute, Null State)
    0x65, 0x00, //         Unit (none)
    0x75, 0x04, //         Report Size (4)
    0x95, 0x01, //         Report Count (1)
    0x81, 0x03, //         Input (Constant) - padding
    0x09, 0x30, //         Usage (X)  - left stick
    0x09, 0x31, //         Usage (Y)
    0x09, 0x33, //         Usage (Rx) - right stick
    0x09, 0x34, //         Usage (Ry)
    0x09, 0x32, //         Usage (Z)  - left trigger
    0x09, 0x35, //         Usage (Rz) - right trigger
    0x15, 0x00, //         Logical Minimum (0)
    0x26, 0xFF, 0x00, //   Logical Maximum (255)
    0x75, 0x08, //         Report Size (8)
    0x95, 0x06, //         Report Count (6)
    0x81, 0x02, //         Input (Data, Variable, Absolute)
    0xC0, //             End Collection
];

pub const REPORT_LEN: usize = 9;

pub const SPEC: ProfileSpec = ProfileSpec {
    key: "pc",
    label: "PC",
    bluetooth_name: NAME,
    usb: None,
    descriptor: DESCRIPTOR,
    schedule: Schedule::OnChange {
        min_gap: Duration::from_millis(10),
    },
    rumble: false,
};

/// Serialise the generic report without its DATA header.
pub fn report(state: &PadState) -> [u8; REPORT_LEN] {
    let [b0, b1] = state.buttons_bits().to_le_bytes();
    let [lx, ly, rx, ry] = state.sticks;
    let [lt, rt] = state.triggers;
    [b0, b1, state.hat.raw() & 0x0F, lx, ly, rx, ry, lt, rt]
}

/// The interrupt-channel packet: DATA header, then the report.
pub fn packet(state: &PadState) -> [u8; REPORT_LEN + 1] {
    let mut packet = [0u8; REPORT_LEN + 1];
    packet[0] = DATA_INPUT;
    packet[1..].copy_from_slice(&report(state));
    packet
}

#[derive(Default)]
pub struct Codec;

impl CodecTrait for Codec {
    fn packets(&mut self, state: &PadState, _elapsed: Duration) -> Reports {
        Reports::one(&packet(state))
    }

    fn control(
        &mut self,
        message: &[u8],
        _link: &super::codec::LinkContext,
        state: &PadState,
    ) -> Control {
        hid::control(message, &report(state))
    }

    fn output(&mut self, _report: &[u8], _link: &super::codec::LinkContext) -> Output {
        Output::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Button, Hat};

    #[test]
    fn the_descriptor_declares_exactly_the_bytes_the_report_carries() {
        assert_eq!(
            crate::test_support::input_bits(DESCRIPTOR, None),
            REPORT_LEN as u32 * 8
        );
    }

    #[test]
    fn a_resting_pad_reports_centred_sticks_and_no_buttons() {
        assert_eq!(
            report(&PadState::default()),
            [0, 0, Hat::Centred.raw(), 128, 128, 128, 128, 0, 0]
        );
    }

    #[test]
    fn buttons_land_on_their_own_bits() {
        let mut s = PadState::default();
        s.set_button(Button::South, true);
        s.set_button(Button::Home, true);
        assert_eq!(report(&s)[..2], [0b0000_0001, 0b0001_0000]);
        s.set_button(Button::South, false);
        assert_eq!(report(&s)[0], 0);
    }

    #[test]
    fn the_interrupt_packet_is_a_data_input_header_and_the_report() {
        let p = packet(&PadState::default());
        assert_eq!(p[0], DATA_INPUT);
        assert_eq!(p.len(), 10);
    }

    #[test]
    fn the_codec_answers_a_get_report_with_the_generic_report() {
        let mut codec = Codec;
        let Control::Reply(reply) = codec.control(
            &[0x41],
            &super::super::codec::LinkContext {
                local: crate::bluetooth::Address([0; 6]),
                peer: crate::bluetooth::Address([0; 6]),
            },
            &PadState::default(),
        ) else {
            panic!()
        };
        assert_eq!(reply[0], DATA_INPUT);
        assert_eq!(&reply[1..], &report(&PadState::default()));
    }
}
