// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! HID class control requests for the generic PC gamepad.
//!
//! The kernel passes every unhandled setup packet to FunctionFS `ep0`. The
//! host's HID driver asks for the report descriptor, then may read the current
//! report, set the idle rate and select the report protocol. This module
//! decides the answer without touching a file, so the protocol is tested on the
//! host.

use super::ffs::hid_descriptor;

/// One USB control request, as FunctionFS reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Setup {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}

impl Setup {
    /// The eight-byte setup packet is little-endian on the wire.
    pub fn from_bytes(bytes: &[u8; 8]) -> Self {
        Self {
            request_type: bytes[0],
            request: bytes[1],
            value: u16::from_le_bytes([bytes[2], bytes[3]]),
            index: u16::from_le_bytes([bytes[4], bytes[5]]),
            length: u16::from_le_bytes([bytes[6], bytes[7]]),
        }
    }

    pub fn dir_in(&self) -> bool {
        self.request_type & 0x80 != 0
    }

    /// `true` for a class request, `false` for a standard one.
    fn is_class(&self) -> bool {
        self.request_type & 0x60 == 0x20
    }
}

/// What `ep0` must do to complete the request.
#[derive(Debug, PartialEq, Eq)]
pub enum Reply {
    /// Write at most the setup length, then complete the IN transfer.
    Data(Vec<u8>),
    /// Read the OUT data stage, then acknowledge.
    ReadOut,
    /// Acknowledge without data.
    Ack,
    /// The request is not supported.
    Stall,
}

/// Answer a setup request for the PC HID profile.
pub fn respond(setup: &Setup, report_descriptor: &[u8], input_report: &[u8]) -> Reply {
    let dir_in = setup.dir_in();

    if !setup.is_class() {
        return match (setup.request, dir_in) {
            // GET_DESCRIPTOR: the host asks for the HID class descriptor or the
            // report descriptor while enumerating the interface.
            (0x06, true) => match (setup.value >> 8) as u8 {
                0x21 => Reply::Data(hid_descriptor(report_descriptor.len() as u16).to_vec()),
                0x22 => Reply::Data(report_descriptor.to_vec()),
                _ => Reply::Stall,
            },
            // GET_STATUS: no remote wakeup, bus powered.
            (0x00, true) => Reply::Data(vec![0, 0]),
            // SET_INTERFACE and SET_CONFIGURATION carry no data stage.
            (0x0B, false) | (0x09, false) => Reply::Ack,
            _ => Reply::Stall,
        };
    }

    match (setup.request, dir_in) {
        // GET_REPORT: only the input report exists in the PC profile.
        (0x01, true) => {
            if (setup.value >> 8) as u8 == 1 {
                Reply::Data(input_report.to_vec())
            } else {
                Reply::Stall
            }
        }
        // GET_IDLE: the profile never sends unsolicited reports, so the idle
        // rate is zero.
        (0x02, true) => Reply::Data(vec![0]),
        // GET_PROTOCOL: report protocol, not boot protocol.
        (0x03, true) => Reply::Data(vec![1]),
        // SET_REPORT carries an output report even though the PC profile
        // declares none; read it so the host is not left waiting.
        (0x09, false) => Reply::ReadOut,
        // SET_IDLE.
        (0x0A, false) => Reply::ReadOut,
        // SET_PROTOCOL.
        (0x0B, false) => Reply::ReadOut,
        _ => Reply::Stall,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT: [u8; 9] = [0, 0, 8, 128, 128, 128, 128, 0, 0];
    const DESCRIPTOR: &[u8] = &[0x05, 0x01, 0x09, 0x05];

    fn setup(request_type: u8, request: u8, value: u16, length: u16) -> Setup {
        Setup {
            request_type,
            request,
            value,
            index: 0,
            length,
        }
    }

    #[test]
    fn get_descriptor_returns_the_report_descriptor() {
        let reply = respond(&setup(0x81, 0x06, 0x2200, 75), DESCRIPTOR, &REPORT);
        assert_eq!(reply, Reply::Data(DESCRIPTOR.to_vec()));
    }

    #[test]
    fn get_descriptor_returns_the_hid_descriptor() {
        let reply = respond(&setup(0x81, 0x06, 0x2100, 9), DESCRIPTOR, &REPORT);
        let Reply::Data(data) = reply else {
            panic!("expected data");
        };
        assert_eq!(data[0], 9);
        assert_eq!(data[1], 0x21);
        assert_eq!(
            u16::from_le_bytes([data[7], data[8]]),
            DESCRIPTOR.len() as u16
        );
    }

    #[test]
    fn get_report_returns_the_current_input_report() {
        let reply = respond(&setup(0xA1, 0x01, 0x0100, 9), DESCRIPTOR, &REPORT);
        assert_eq!(reply, Reply::Data(REPORT.to_vec()));
    }

    #[test]
    fn set_report_reads_the_out_data_stage() {
        let reply = respond(&setup(0x21, 0x09, 0x0200, 8), DESCRIPTOR, &REPORT);
        assert_eq!(reply, Reply::ReadOut);
    }

    #[test]
    fn the_protocol_and_idle_requests_are_answered() {
        assert_eq!(
            respond(&setup(0xA1, 0x03, 0, 1), DESCRIPTOR, &REPORT),
            Reply::Data(vec![1])
        );
        assert_eq!(
            respond(&setup(0xA1, 0x02, 0, 1), DESCRIPTOR, &REPORT),
            Reply::Data(vec![0])
        );
    }

    #[test]
    fn an_unknown_request_is_stalled() {
        assert_eq!(
            respond(&setup(0x21, 0x42, 0, 0), DESCRIPTOR, &REPORT),
            Reply::Stall
        );
    }

    #[test]
    fn the_setup_packet_is_little_endian() {
        let parsed = Setup::from_bytes(&[0x81, 0x06, 0x00, 0x22, 0, 0, 75, 0]);
        assert_eq!(parsed.value, 0x2200);
        assert_eq!(parsed.length, 75);
        assert!(parsed.dir_in());
    }
}
