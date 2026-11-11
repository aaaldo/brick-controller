// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! The stable identity of a controller type and its immutable metadata.
//!
//! A host caches the identity it paired with, so [`Profile`] is the value
//! stored on disk and shown in the interface. Everything a host reads before
//! connecting - Bluetooth name, Device ID, report descriptor and report
//! cadence - lives in [`ProfileSpec`]. The stateful encoder lives in
//! [`super::codec::Codec`], not here.

use std::time::Duration;

/// The USB identity a host sees in the Device ID record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsbId {
    pub vendor: u16,
    pub product: u16,
    pub version: u16,
}

/// When a profile sends its input reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schedule {
    /// Send on change, but not more often than `min_gap`.
    OnChange { min_gap: Duration },
    /// Send at a fixed interval, even when the state did not change.
    Periodic { every: Duration },
}

/// Everything about an identity that is known at compile time.
#[derive(Debug, Clone, Copy)]
pub struct ProfileSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub bluetooth_name: &'static str,
    pub usb: Option<UsbId>,
    pub descriptor: &'static [u8],
    pub schedule: Schedule,
    pub rumble: bool,
}

/// The user-facing controller type, stored per host as a stable key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Profile {
    Pc,
    Xbox,
    PlayStation,
    Switch,
}

impl Profile {
    /// Every identity offered by the pairing screen, in display order.
    pub const ALL: [Self; 4] = [Self::Pc, Self::Xbox, Self::PlayStation, Self::Switch];

    pub fn spec(self) -> &'static ProfileSpec {
        match self {
            Self::Pc => &super::generic_hid::SPEC,
            Self::Xbox => &super::xbox::SPEC,
            Self::PlayStation => &super::dualshock4::SPEC,
            Self::Switch => &super::switch_pro::SPEC,
        }
    }

    pub fn label(self) -> &'static str {
        self.spec().label
    }

    pub fn bt_name(self) -> &'static str {
        self.spec().bluetooth_name
    }

    pub fn descriptor(self) -> &'static [u8] {
        self.spec().descriptor
    }

    pub fn schedule(self) -> Schedule {
        self.spec().schedule
    }

    pub fn has_rumble(self) -> bool {
        self.spec().rumble
    }

    pub fn device_id(self) -> Option<String> {
        self.spec().usb.map(|id| {
            format!(
                "usb:{:04X}:{:04X}:{:04X}",
                id.vendor, id.product, id.version
            )
        })
    }

    pub fn key(self) -> &'static str {
        self.spec().key
    }

    /// Parse a stored key. Unknown or obsolete values are ignored by the
    /// caller, so a file written by a newer build still loads.
    pub fn from_key(key: &str) -> Option<Self> {
        let key = key.trim();
        Self::ALL
            .iter()
            .copied()
            .find(|profile| profile.key().eq_ignore_ascii_case(key))
            .or_else(|| match key.to_ascii_lowercase().as_str() {
                // Accept the full name as a human-written alias.
                "playstation" | "dualshock4" => Some(Self::PlayStation),
                _ => None,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_profile_key_round_trips() {
        for profile in Profile::ALL {
            assert_eq!(Profile::from_key(profile.key()), Some(profile));
        }
        assert_eq!(Profile::from_key(" PC "), Some(Profile::Pc));
        assert_eq!(Profile::from_key("PlayStation"), Some(Profile::PlayStation));
        assert_eq!(Profile::from_key("ds4"), None);
    }

    #[test]
    fn profile_metadata_is_complete() {
        for profile in Profile::ALL {
            let spec = profile.spec();
            assert!(!spec.key.is_empty());
            assert!(!spec.label.is_empty());
            assert!(!spec.bluetooth_name.is_empty());
            assert!(!spec.descriptor.is_empty());
        }
    }

    #[test]
    fn device_ids_use_the_hid_record_format() {
        assert_eq!(Profile::Pc.device_id(), None);
        assert_eq!(
            Profile::Switch.device_id().as_deref(),
            Some("usb:057E:2009:0000")
        );
        assert_eq!(
            Profile::Xbox.device_id().as_deref(),
            Some("usb:045E:02FD:0408")
        );
        assert_eq!(
            Profile::PlayStation.device_id().as_deref(),
            Some("usb:054C:09CC:0100")
        );
    }
}
