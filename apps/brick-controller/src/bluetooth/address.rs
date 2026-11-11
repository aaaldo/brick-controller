// Copyright 2026 ligt (https://github.com/duyquang6/padpod)
// SPDX-License-Identifier: GPL-3.0-only

//! A Bluetooth device address, kept in printed order.
//!
//! The kernel's `sockaddr_l2` stores the address least-significant octet
//! first, while `Display` and the `profiles` file use the printed order. The
//! conversions live here so no socket or profile code has to remember which
//! way round it is.

/// A Bluetooth device address, stored as printed (most significant first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Address(pub [u8; 6]);

impl Address {
    pub fn parse(text: &str) -> Option<Self> {
        let parts: Vec<u8> = text
            .trim()
            .split(':')
            .map(|p| u8::from_str_radix(p, 16).ok())
            .collect::<Option<_>>()?;
        Some(Self(parts.try_into().ok()?))
    }

    pub(crate) fn to_wire(self) -> [u8; 6] {
        let mut bytes = self.0;
        bytes.reverse();
        bytes
    }

    pub(crate) fn from_wire(mut bytes: [u8; 6]) -> Self {
        bytes.reverse();
        Self(bytes)
    }
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [a, b, c, d, e, g] = self.0;
        write!(f, "{a:02X}:{b:02X}:{c:02X}:{d:02X}:{e:02X}:{g:02X}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing_and_display_use_the_printed_order() {
        let address = Address::parse("02:11:22:33:44:55").unwrap();
        assert_eq!(address.to_string(), "02:11:22:33:44:55");
        assert_eq!(address.to_wire(), [0x55, 0x44, 0x33, 0x22, 0x11, 0x02]);
        assert_eq!(Address::from_wire(address.to_wire()), address);
    }
}
