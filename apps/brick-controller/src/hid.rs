// Copyright 2026 ligt (https://github.com/duyquang6/padpod)
// SPDX-License-Identifier: GPL-3.0-only

//! HID mechanics shared by every controller profile: the DATA headers, the
//! control-channel transaction types, and the answers owed to the host.
//!
//! Profile-specific descriptors and report layouts live in `profiles/`. This
//! module deliberately knows no controller identity: a profile hands it the
//! input report to return for a `GET_REPORT` and handles its own feature
//! reports before falling back here.
//!
//! On the interrupt channel a report travels as a DATA header (`0xA1` for
//! input, `0xA2` for output) followed by the profile's bytes. On the control
//! channel the host uses the HID transaction headers documented in the match
//! below.

pub const DATA_INPUT: u8 = 0xA1;
pub const DATA_OUTPUT: u8 = 0xA2;

const HANDSHAKE_SUCCESSFUL: u8 = 0x00;
/// `HANDSHAKE_INVALID_REPORT_ID`, exported for profiles that own report IDs.
pub const HANDSHAKE_INVALID_REPORT_ID: u8 = 0x02;
const HANDSHAKE_UNSUPPORTED: u8 = 0x03;
const HANDSHAKE_INVALID_PARAMETER: u8 = 0x04;

/// What a control-channel message asks of us.
#[derive(Debug, PartialEq, Eq)]
pub enum Control {
    /// Send these bytes back on the control channel.
    Reply(Vec<u8>),
    /// The host has unpaired: forget it and disconnect.
    Unplug,
    /// Nothing to answer (a suspend notice, say).
    Ignore,
}

/// Answer one message from the host, per the HID profile's transaction types.
///
/// `input_report` is the profile's current report, without the DATA header.
/// It is used for `GET_REPORT(input)`, which most hosts only send while they
/// are probing the device. A profile that owns output or feature reports must
/// intercept those first and delegate the rest here.
pub fn control(message: &[u8], input_report: &[u8]) -> Control {
    let Some(&header) = message.first() else {
        return Control::Ignore;
    };
    let param = header & 0x0F;
    match header >> 4 {
        // HID_CONTROL: 3 suspend, 4 exit suspend, 5 virtual cable unplug.
        0x1 if param == 5 => Control::Unplug,
        0x1 => Control::Ignore,
        // GET_REPORT. The low two bits say which report type was asked for;
        // only an input report exists in the generic profile.
        0x4 if param & 0x03 == 1 => {
            let mut reply = vec![DATA_INPUT];
            reply.extend_from_slice(input_report);
            Control::Reply(reply)
        }
        0x4 => Control::Reply(vec![HANDSHAKE_INVALID_PARAMETER]),
        // SET_REPORT: the generic profile has nothing to set, but refusing it
        // makes some hosts give up on the device.
        0x5 => Control::Reply(vec![HANDSHAKE_SUCCESSFUL]),
        // GET_PROTOCOL: always the report protocol.
        0x6 => Control::Reply(vec![0xA0, 0x01]),
        // SET_PROTOCOL and SET_IDLE are acknowledged; the report never
        // changes shape and every change is sent anyway.
        0x7 | 0x9 => Control::Reply(vec![HANDSHAKE_SUCCESSFUL]),
        // GET_IDLE: infinite, which is to say only on change.
        0x8 => Control::Reply(vec![0xA0, 0x00]),
        _ => Control::Reply(vec![HANDSHAKE_UNSUPPORTED]),
    }
}

/// The output report inside a control-channel SET_REPORT, if this is one.
/// The message is the transaction header followed by the report ID and data.
///
/// Hosts use this path for rumble when they do not open the interrupt channel
/// for output, so both paths must feed the same profile parser.
pub fn set_report_output(message: &[u8]) -> Option<&[u8]> {
    let (&header, report) = message.split_first()?;
    (header >> 4 == 0x5 && header & 0x03 == 0x02).then_some(report)
}

/// The output report inside an interrupt-channel DATA transaction: the `0xA2`
/// header, then the report ID and data.
pub fn data_output(message: &[u8]) -> Option<&[u8]> {
    message.strip_prefix(&[DATA_OUTPUT])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_messages_get_the_answers_the_profile_requires() {
        let report = [0u8; 9];
        assert_eq!(control(&[0x15], &report), Control::Unplug);
        assert_eq!(control(&[0x13], &report), Control::Ignore);
        assert_eq!(
            control(&[0x71], &report),
            Control::Reply(vec![0x00]),
            "SET_PROTOCOL report"
        );
        assert_eq!(
            control(&[0x90, 0x00], &report),
            Control::Reply(vec![0x00]),
            "SET_IDLE"
        );
        assert_eq!(control(&[0x60], &report), Control::Reply(vec![0xA0, 0x01]));
        let Control::Reply(r) = control(&[0x41], &report) else {
            panic!()
        };
        assert_eq!(r[0], DATA_INPUT);
        assert_eq!(&r[1..], &report);
        assert_eq!(
            control(&[0x43], &report),
            Control::Reply(vec![0x04]),
            "no feature report"
        );
        assert_eq!(control(&[0x52, 0x01], &report), Control::Reply(vec![0x00]));
        assert_eq!(control(&[0xB0], &report), Control::Reply(vec![0x03]));
        assert_eq!(control(&[], &report), Control::Ignore);
    }

    #[test]
    fn output_reports_are_located_on_both_channels() {
        assert_eq!(
            set_report_output(&[0x52, 0x03, 1, 2]),
            Some(&[0x03, 1, 2][..]),
            "SET_REPORT with the output report type"
        );
        assert_eq!(set_report_output(&[0x51, 0x03, 1]), None, "input report");
        assert_eq!(set_report_output(&[0x42, 0x03]), None, "GET_REPORT");
        assert_eq!(set_report_output(&[]), None);
        assert_eq!(
            data_output(&[DATA_OUTPUT, 0x03, 1, 2]),
            Some(&[0x03, 1, 2][..]),
            "DATA output on the interrupt channel"
        );
        assert_eq!(data_output(&[DATA_INPUT, 0x03, 1]), None, "input data");
    }
}
