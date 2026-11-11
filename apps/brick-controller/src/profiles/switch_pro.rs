// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only
//
// Portions ported from deck-as-controller's switch_pro.py.
// Copyright (c) 2026 jmedina21
// SPDX-License-Identifier: MIT

//! Nintendo Switch Pro Controller over Bluetooth Classic.
//!
//! Unlike the other profiles, the host talks to the controller: it sends
//! subcommands in output report 0x01 and waits for an input report 0x21
//! (device info, SPI calibration reads, input mode, IMU and rumble enables).
//! The controller starts in simple mode 0x3F until the host asks for the full
//! 0x30 reports.
//!
//! The protocol follows dekuNukem's Nintendo_Switch_Reverse_Engineering, SDL's
//! HIDAPI Switch driver and Linux's `hid-nintendo.c`. No IMU, NFC or IR data is
//! sent: the full report's IMU samples stay zero, which hosts treat as "no
//! motion". The single Brick motor receives the strongest rumble band.

use super::codec::{Codec as CodecTrait, LinkContext, Output, Reports};
use super::spec::{ProfileSpec, Schedule};
use crate::bluetooth::Address;
use crate::hid::{self, Control};
use crate::input::{Button, Hat, PadState};
use std::collections::VecDeque;
use std::time::Duration;

pub const NAME: &str = "Pro Controller";

pub const DESCRIPTOR: &[u8] = &[
    0x05, 0x01, 0x09, 0x05, 0xA1, 0x01, 0x06, 0x01, 0xFF, 0x85, 0x21, 0x09, 0x21, 0x75, 0x08, 0x95,
    0x30, 0x81, 0x02, 0x85, 0x30, 0x09, 0x30, 0x75, 0x08, 0x95, 0x30, 0x81, 0x02, 0x85, 0x31, 0x09,
    0x31, 0x75, 0x08, 0x96, 0x69, 0x01, 0x81, 0x02, 0x85, 0x3F, 0x05, 0x09, 0x19, 0x01, 0x29, 0x10,
    0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x10, 0x81, 0x02, 0x05, 0x01, 0x09, 0x39, 0x15, 0x00,
    0x25, 0x07, 0x75, 0x04, 0x95, 0x01, 0x81, 0x42, 0x05, 0x09, 0x75, 0x04, 0x95, 0x01, 0x81, 0x01,
    0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x09, 0x33, 0x09, 0x34, 0x16, 0x00, 0x00, 0x27, 0xFF, 0xFF,
    0x00, 0x00, 0x75, 0x10, 0x95, 0x04, 0x81, 0x02, 0x06, 0x01, 0xFF, 0x85, 0x01, 0x09, 0x01, 0x75,
    0x08, 0x95, 0x30, 0x91, 0x02, 0x85, 0x10, 0x09, 0x10, 0x75, 0x08, 0x95, 0x30, 0x91, 0x02, 0x85,
    0x11, 0x09, 0x11, 0x75, 0x08, 0x95, 0x30, 0x91, 0x02, 0x85, 0x12, 0x09, 0x12, 0x75, 0x08, 0x95,
    0x30, 0x91, 0x02, 0xC0,
];

pub const SPEC: ProfileSpec = ProfileSpec {
    key: "switch",
    label: "Switch",
    bluetooth_name: NAME,
    usb: None,
    descriptor: DESCRIPTOR,
    schedule: Schedule::Periodic {
        every: Duration::from_millis(15),
    },
    rumble: true,
};

const REPORT_LEN: usize = 49;
const STICK_CENTER: u16 = 0x800;
const STICK_RANGE: i32 = 0x700;

/// Stick and IMU parameters hosts read from the emulated SPI flash.
const STICK_PARAMS: [u8; 18] = [
    0x0F, 0x30, 0x61, 0x96, 0x30, 0xF3, 0xD4, 0x14, 0x54, 0x41, 0x15, 0x54, 0xC7, 0x79, 0x9C, 0x33,
    0x36, 0x63,
];
const COLOURS: [u8; 12] = [
    0x32, 0x32, 0x32, 0xFF, 0xFF, 0xFF, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32,
];

struct Reply {
    ack: u8,
    subcommand: u8,
    data: Vec<u8>,
}

/// The Switch encoder has more state than the others: a report counter, the
/// current reporting mode, a queue of 0x21 replies, and the emulated SPI flash
/// the host reads during its handshake.
pub struct Codec {
    timer: u8,
    full: bool,
    replies: VecDeque<Reply>,
    spi: Vec<u8>,
}

impl Default for Codec {
    fn default() -> Self {
        Self {
            timer: 0,
            full: false,
            replies: VecDeque::new(),
            spi: spi_flash(),
        }
    }
}

impl Codec {
    fn push_reply(&mut self, ack: u8, subcommand: u8, data: Vec<u8>) {
        // A host that floods subcommands must not make this grow without
        // bound; the oldest reply is the one a late host no longer needs.
        if self.replies.len() == 8 {
            self.replies.pop_front();
        }
        self.replies.push_back(Reply {
            ack,
            subcommand,
            data,
        });
    }

    fn handle_subcommand(&mut self, subcommand: u8, args: &[u8], local: Address) {
        let (ack, data) = match subcommand {
            0x02 => {
                // Firmware 4.33, Pro Controller, our address, then colours.
                let mut data = vec![0x04, 0x33, 0x03, 0x02];
                data.extend(local.0.iter().rev());
                data.extend([0x01, 0x01]);
                (0x82, data)
            }
            0x03 => {
                self.full = args.first().is_some_and(|mode| *mode != 0x3F);
                (0x80, Vec::new())
            }
            0x04 => (0x83, Vec::new()),
            0x10 if args.len() >= 5 => {
                let address = u32::from_le_bytes([args[0], args[1], args[2], args[3]]) as usize;
                let size = (args[4] as usize).min(0x1D);
                let mut data = args[..5].to_vec();
                data.extend(self.spi_slice(address, size));
                (0x90, data)
            }
            // Player lights, rumble enable, low power and other harmless
            // subcommands are simply acknowledged.
            _ => (0x80, Vec::new()),
        };
        self.push_reply(ack, subcommand, data);
    }

    fn spi_slice(&self, address: usize, size: usize) -> Vec<u8> {
        let end = address.saturating_add(size);
        if end <= self.spi.len() {
            self.spi[address..end].to_vec()
        } else {
            vec![0xFF; size]
        }
    }

    fn current_report(&self, state: &PadState) -> Vec<u8> {
        if let Some(reply) = self.replies.front() {
            let mut report = vec![0u8; REPORT_LEN];
            report[0] = 0x21;
            report[1] = self.timer;
            report[2] = 0x80;
            self.fill_controls(&mut report, state);
            report[12] = 0x80;
            report[13] = reply.ack;
            report[14] = reply.subcommand;
            let end = (15 + reply.data.len()).min(REPORT_LEN);
            report[15..end].copy_from_slice(&reply.data[..end - 15]);
            report
        } else if self.full {
            let mut report = vec![0u8; REPORT_LEN];
            report[0] = 0x30;
            report[1] = self.timer;
            report[2] = 0x80;
            self.fill_controls(&mut report, state);
            report[12] = 0x80;
            report
        } else {
            simple_report(state)
        }
    }

    fn fill_controls(&self, report: &mut [u8], state: &PadState) {
        let bits = button_bits(state);
        report[3] = bits as u8;
        report[4] = (bits >> 8) as u8;
        report[5] = (bits >> 16) as u8;
        report[6..9].copy_from_slice(&pack_stick(
            axis_full(state.sticks[0], false),
            axis_full(state.sticks[1], true),
        ));
        report[9..12].copy_from_slice(&pack_stick(
            axis_full(state.sticks[2], false),
            axis_full(state.sticks[3], true),
        ));
    }
}

impl CodecTrait for Codec {
    fn packets(&mut self, state: &PadState, _elapsed: Duration) -> Reports {
        self.timer = self.timer.wrapping_add(1);
        let report = self.current_report(state);
        if !self.replies.is_empty() {
            self.replies.pop_front();
        }
        Reports::one(&report)
    }

    fn control(&mut self, message: &[u8], _link: &LinkContext, state: &PadState) -> Control {
        // GET_REPORT(input) is answered with the current 0x30/0x3F packet.
        // Feature reports are not declared by the descriptor.
        hid::control(message, &self.current_report(state))
    }

    fn output(&mut self, report: &[u8], link: &LinkContext) -> Output {
        let mut output = Output::default();
        match report.first().copied() {
            Some(0x01) if report.len() >= 11 => {
                output.rumble = rumble(&report[2..10]);
                let subcommand = report[10];
                let args = &report[11..];
                self.handle_subcommand(subcommand, args, link.local);
                if subcommand == 0x06 && args.first() == Some(&0x00) {
                    output.disconnect = true;
                }
            }
            Some(0x10) if report.len() >= 10 => {
                output.rumble = rumble(&report[2..10]);
            }
            _ => {}
        }
        output
    }
}

fn simple_report(state: &PadState) -> Vec<u8> {
    let mut bits = 0u16;
    let mut set = |button: Button, bit: u16| {
        if state.pressed(button) {
            bits |= 1 << bit;
        }
    };
    set(Button::East, 0); // A
    set(Button::South, 1); // B
    set(Button::North, 2); // X
    set(Button::West, 3); // Y
    set(Button::L1, 4);
    set(Button::R1, 5);
    set(Button::L2, 6); // ZL
    set(Button::R2, 7); // ZR
    set(Button::Select, 8); // Minus
    set(Button::Start, 9); // Plus
    set(Button::L3, 10);
    set(Button::R3, 11);
    set(Button::Home, 12);
    if state.triggers[0] > 30 {
        bits |= 1 << 6;
    }
    if state.triggers[1] > 30 {
        bits |= 1 << 7;
    }
    let mut report = vec![0u8; 12];
    report[0] = 0x3F;
    report[1..3].copy_from_slice(&bits.to_le_bytes());
    report[3] = state.hat.raw();
    report[4..6].copy_from_slice(&axis_simple(state.sticks[0]).to_le_bytes());
    report[6..8].copy_from_slice(&axis_simple(state.sticks[1]).to_le_bytes());
    report[8..10].copy_from_slice(&axis_simple(state.sticks[2]).to_le_bytes());
    report[10..12].copy_from_slice(&axis_simple(state.sticks[3]).to_le_bytes());
    report
}

/// Full-report buttons: Y X B A, then R/ZR, Minus/Plus, sticks, Home, and the
/// D-pad and L/ZL in the third byte. Bits are numbered as the real report.
fn button_bits(state: &PadState) -> u32 {
    let mut bits = 0u32;
    let mut set = |button: Button, bit: u32| {
        if state.pressed(button) {
            bits |= 1 << bit;
        }
    };
    set(Button::West, 0); // Y
    set(Button::North, 1); // X
    set(Button::South, 2); // B
    set(Button::East, 3); // A
    set(Button::R1, 6); // R
    set(Button::R2, 7); // ZR
    set(Button::Select, 8); // Minus
    set(Button::Start, 9); // Plus
    set(Button::R3, 10);
    set(Button::L3, 11);
    set(Button::Home, 12);
    set(Button::L1, 22); // L
    set(Button::L2, 23); // ZL
    if state.triggers[0] > 30 {
        bits |= 1 << 23;
    }
    if state.triggers[1] > 30 {
        bits |= 1 << 7;
    }
    match state.hat {
        Hat::Up => bits |= 1 << 17,
        Hat::UpRight => bits |= (1 << 17) | (1 << 18),
        Hat::Right => bits |= 1 << 18,
        Hat::DownRight => bits |= (1 << 16) | (1 << 18),
        Hat::Down => bits |= 1 << 16,
        Hat::DownLeft => bits |= (1 << 16) | (1 << 19),
        Hat::Left => bits |= 1 << 19,
        Hat::UpLeft => bits |= (1 << 17) | (1 << 19),
        Hat::Centred => {}
    }
    bits
}

/// 12-bit full-report axis; Y is inverted because the report counts up, while
/// the pad state counts down from the top.
fn axis_full(value: u8, invert: bool) -> u16 {
    let offset = axis_offset(value, STICK_RANGE);
    let raw = if invert {
        STICK_CENTER as i32 - offset
    } else {
        STICK_CENTER as i32 + offset
    };
    raw.clamp(0, 0xFFF) as u16
}

/// Simple-report axis is 16 bit with 0x8000 at rest; 0 is up/left.
fn axis_simple(value: u8) -> u16 {
    let scaled = 32768 + axis_offset(value, 32767);
    scaled.clamp(0, 0xFFFF) as u16
}

/// Map a 0-255 axis onto `-range..=range`.
///
/// The two halves are scaled separately so 0 and 255 reach the exact ends;
/// dividing both by 128 would leave the positive half one step short.
fn axis_offset(value: u8, range: i32) -> i32 {
    let delta = value as i32 - 128;
    if delta >= 0 {
        delta * range / 127
    } else {
        delta * range / 128
    }
}

fn pack_stick(x: u16, y: u16) -> [u8; 3] {
    [
        x as u8,
        ((x >> 8) as u8) | (((y & 0x0F) as u8) << 4),
        (y >> 4) as u8,
    ]
}

/// The strongest of the two rumble bands, on either side, converted to the
/// 0-255 strength the Brick motor takes.
fn rumble(bytes: &[u8]) -> Option<u8> {
    if bytes.len() < 8 {
        return None;
    }
    let (left_low, left_high) = amplitude(&bytes[..4]);
    let (right_low, right_high) = amplitude(&bytes[4..8]);
    let level = left_low.max(right_low).max(left_high).max(right_high);
    Some((level as u16 * 255 / 100) as u8)
}

fn amplitude(side: &[u8]) -> (u8, u8) {
    let high = ((side[1] & 0xFE) >> 1).min(100);
    let low = if side[3] >= 0x40 {
        (((side[3] - 0x40) << 1) | (side[2] >> 7)).min(100)
    } else {
        0
    };
    (low, high)
}

/// The parts of the controller's SPI flash hosts read during the handshake.
/// Unset bytes are 0xFF, which means "no user calibration"; the factory values
/// below are plausible defaults.
fn spi_flash() -> Vec<u8> {
    let mut flash = vec![0xFFu8; 0x10000];
    flash[0x6012] = 0x03; // device type: Pro Controller
    flash[0x601B] = 0x01; // colours below are set
    let imu: [i16; 12] = [0, 0, 0, 16384, 16384, 16384, 0, 0, 0, 13371, 13371, 13371];
    for (i, value) in imu.iter().enumerate() {
        flash[0x6020 + i * 2..0x6022 + i * 2].copy_from_slice(&value.to_le_bytes());
    }
    let center = STICK_CENTER;
    let range = STICK_RANGE as u16;
    flash[0x603D..0x6046].copy_from_slice(
        &[
            pack_stick(range, range),
            pack_stick(center, center),
            pack_stick(range, range),
        ]
        .concat(),
    );
    flash[0x6046..0x604F].copy_from_slice(
        &[
            pack_stick(center, center),
            pack_stick(range, range),
            pack_stick(range, range),
        ]
        .concat(),
    );
    flash[0x6050..0x605C].copy_from_slice(&COLOURS);
    flash[0x6080..0x6086].copy_from_slice(&[0x50, 0xFD, 0x00, 0x00, 0xC6, 0x0F]);
    flash[0x6086..0x6098].copy_from_slice(&STICK_PARAMS);
    flash[0x6098..0x60AA].copy_from_slice(&STICK_PARAMS);
    flash
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN: Address = Address([0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
    const HOST: Address = Address([0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC]);

    fn link() -> LinkContext {
        LinkContext {
            local: OWN,
            peer: HOST,
        }
    }

    #[test]
    fn the_descriptor_declares_report_0x30_at_the_size_sent() {
        assert_eq!(
            crate::test_support::input_bits(DESCRIPTOR, Some(0x30)),
            48 * 8
        );
    }

    #[test]
    fn simple_reports_are_twelve_bytes_with_a_centred_pad() {
        let report = simple_report(&PadState::default());
        assert_eq!(report.len(), 12);
        assert_eq!(report[0], 0x3F);
        assert_eq!(report[3], Hat::Centred.raw());
        assert_eq!(u16::from_le_bytes([report[4], report[5]]), 32768);
    }

    #[test]
    fn the_host_switches_the_codec_to_full_reports() {
        let mut codec = Codec::default();
        assert_eq!(
            codec
                .packets(&PadState::default(), Duration::ZERO)
                .iter()
                .next()
                .unwrap()[0],
            0x3F
        );
        codec.output(&[0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x03, 0x30], &link());
        // The acknowledgement is 0x21, then the next packet is 0x30.
        let ack = codec.packets(&PadState::default(), Duration::ZERO);
        assert_eq!(ack.iter().next().unwrap()[0], 0x21);
        let full = codec.packets(&PadState::default(), Duration::ZERO);
        assert_eq!(full.iter().next().unwrap()[0], 0x30);
        assert_eq!(full.iter().next().unwrap().len(), REPORT_LEN);
    }

    #[test]
    fn device_info_reverses_the_local_address() {
        let mut codec = Codec::default();
        codec.output(&[0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02], &link());
        let report = codec.packets(&PadState::default(), Duration::ZERO);
        let report = report.iter().next().unwrap();
        assert_eq!(&report[13..15], &[0x82, 0x02]);
        assert_eq!(&report[19..25], &[0x55, 0x44, 0x33, 0x22, 0x11, 0x02]);
    }

    #[test]
    fn an_spi_read_returns_the_requested_flash_bytes() {
        let mut codec = Codec::default();
        codec.output(
            &[0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10, 0x12, 0x60, 0, 0, 1],
            &link(),
        );
        let report = codec.packets(&PadState::default(), Duration::ZERO);
        let report = report.iter().next().unwrap();
        assert_eq!(&report[13..15], &[0x90, 0x10]);
        assert_eq!(report[20], 0x03, "device type in the emulated flash");
    }

    #[test]
    fn a_sleep_subcommand_asks_the_session_to_disconnect() {
        let mut codec = Codec::default();
        let output = codec.output(&[0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x06, 0x00], &link());
        assert!(output.disconnect);
    }

    #[test]
    fn rumble_is_the_strongest_band_on_either_side() {
        // Left high amplitude 80, right low amplitude 40.
        let bytes = [0, 0xA0, 0, 0x40, 0, 0, 0, 0x40];
        assert_eq!(rumble(&bytes), Some((80u16 * 255 / 100) as u8));
    }

    #[test]
    fn full_report_sticks_center_and_invert_y() {
        assert_eq!(axis_full(128, false), 0x800);
        assert_eq!(axis_full(0, false), 0x100);
        assert_eq!(axis_full(255, false), 0xF00);
        assert_eq!(axis_full(0, true), 0xF00, "0 is up");
        assert_eq!(axis_full(255, true), 0x100);
    }
}
