// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! The common contract every controller profile implements, and the facade the
//! session talks to.
//!
//! The profile set is closed and known at compile time, so dispatch is an
//! [`AnyCodec`] enum rather than a trait object. Each profile module owns its
//! descriptor and report layout; this module owns only the shared types and
//! the delegation.

use crate::bluetooth::Address;
use crate::hid::Control;
use crate::input::PadState;
use crate::profiles::spec::Profile;
use std::time::Duration;

/// The largest report any current profile sends (DualShock 4 is 79 bytes with
/// its DATA header). Keeping reports in a fixed buffer avoids an allocation
/// per report on the 10-15 ms path.
pub const MAX_REPORT: usize = 128;

/// One report, stored inline.
#[derive(Clone, Copy)]
pub struct Report {
    bytes: [u8; MAX_REPORT],
    len: u8,
}

impl Report {
    fn new(bytes: &[u8]) -> Self {
        assert!(bytes.len() <= MAX_REPORT, "report exceeds MAX_REPORT");
        let mut buf = [0u8; MAX_REPORT];
        buf[..bytes.len()].copy_from_slice(bytes);
        Self {
            bytes: buf,
            len: bytes.len() as u8,
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

/// The one or two reports a profile sends for a state change.
///
/// Xbox needs two when the Guide button changes, because its report 0x02 is
/// separate from the main report. Every other profile currently returns one.
#[derive(Clone, Copy)]
pub struct Reports {
    reports: [Report; 2],
    len: u8,
}

impl Reports {
    pub fn one(bytes: &[u8]) -> Self {
        Self {
            reports: [Report::new(bytes), Report::new(&[])],
            len: 1,
        }
    }

    pub fn two(first: &[u8], second: &[u8]) -> Self {
        Self {
            reports: [Report::new(first), Report::new(second)],
            len: 2,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &[u8]> {
        self.reports[..self.len as usize]
            .iter()
            .map(Report::as_slice)
    }
}

/// What a profile wants the session to do after a host output report.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Output {
    /// Motor strength, 0-255. `None` leaves the motor untouched.
    pub rumble: Option<u8>,
    /// The host asked the controller to disconnect (Switch sleep, for example).
    pub disconnect: bool,
}

/// The addressing information a profile may need for its protocol replies.
#[derive(Debug, Clone, Copy)]
pub struct LinkContext {
    pub local: Address,
    pub peer: Address,
}

/// The stateful protocol side of one profile.
pub trait Codec {
    /// Build the reports to send now. `elapsed` is the time since the previous
    /// send, for profiles that carry a timestamp or counter.
    fn packets(&mut self, state: &PadState, elapsed: Duration) -> Reports;

    /// Answer a HID control transaction. The profile must handle its feature
    /// reports before falling back to `hid::control`.
    fn control(&mut self, message: &[u8], link: &LinkContext, state: &PadState) -> Control;

    /// Interpret an output report from the host.
    fn output(&mut self, report: &[u8], link: &LinkContext) -> Output;
}

/// Static dispatch over the closed profile set.
pub enum AnyCodec {
    Pc(super::generic_hid::Codec),
    Xbox(super::xbox::Codec),
    PlayStation(super::dualshock4::Codec),
    Switch(super::switch_pro::Codec),
}

impl AnyCodec {
    fn new(profile: Profile) -> Self {
        match profile {
            Profile::Pc => Self::Pc(super::generic_hid::Codec),
            Profile::Xbox => Self::Xbox(super::xbox::Codec::default()),
            Profile::PlayStation => Self::PlayStation(super::dualshock4::Codec::default()),
            Profile::Switch => Self::Switch(super::switch_pro::Codec::default()),
        }
    }
}

impl Codec for AnyCodec {
    fn packets(&mut self, state: &PadState, elapsed: Duration) -> Reports {
        match self {
            Self::Pc(codec) => codec.packets(state, elapsed),
            Self::Xbox(codec) => codec.packets(state, elapsed),
            Self::PlayStation(codec) => codec.packets(state, elapsed),
            Self::Switch(codec) => codec.packets(state, elapsed),
        }
    }

    fn control(&mut self, message: &[u8], link: &LinkContext, state: &PadState) -> Control {
        match self {
            Self::Pc(codec) => codec.control(message, link, state),
            Self::Xbox(codec) => codec.control(message, link, state),
            Self::PlayStation(codec) => codec.control(message, link, state),
            Self::Switch(codec) => codec.control(message, link, state),
        }
    }

    fn output(&mut self, report: &[u8], link: &LinkContext) -> Output {
        match self {
            Self::Pc(codec) => codec.output(report, link),
            Self::Xbox(codec) => codec.output(report, link),
            Self::PlayStation(codec) => codec.output(report, link),
            Self::Switch(codec) => codec.output(report, link),
        }
    }
}

/// What the session uses: a profile identity plus its stateful encoder.
pub struct Controller {
    profile: Profile,
    codec: AnyCodec,
}

impl Controller {
    pub fn new(profile: Profile) -> Self {
        Self {
            profile,
            codec: AnyCodec::new(profile),
        }
    }

    pub fn profile(&self) -> Profile {
        self.profile
    }

    pub fn packets(&mut self, state: &PadState, elapsed: Duration) -> Reports {
        self.codec.packets(state, elapsed)
    }

    pub fn control(&mut self, message: &[u8], link: &LinkContext, state: &PadState) -> Control {
        self.codec.control(message, link, state)
    }

    pub fn output(&mut self, report: &[u8], link: &LinkContext) -> Output {
        self.codec.output(report, link)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_keep_their_bytes_and_respect_the_limit() {
        let one = Reports::one(&[1, 2, 3]);
        assert_eq!(one.iter().collect::<Vec<_>>(), vec![&[1, 2, 3][..]]);
        let two = Reports::two(&[1], &[2, 3]);
        assert_eq!(two.iter().collect::<Vec<_>>(), vec![&[1][..], &[2, 3][..]]);
    }

    #[test]
    fn every_profile_builds_a_neutral_packet() {
        for profile in Profile::ALL {
            let mut controller = Controller::new(profile);
            let packets = controller.packets(&PadState::default(), Duration::ZERO);
            let first = packets
                .iter()
                .next()
                .expect("a profile sends at least one packet");
            assert!(!first.is_empty(), "{} sent an empty packet", profile.key());
            assert!(
                first.len() <= MAX_REPORT,
                "{} exceeded MAX_REPORT",
                profile.key()
            );
        }
    }

    #[test]
    fn profile_keys_are_unique() {
        for (i, left) in Profile::ALL.iter().enumerate() {
            for right in &Profile::ALL[i + 1..] {
                assert_ne!(left.key(), right.key());
            }
        }
    }
}
