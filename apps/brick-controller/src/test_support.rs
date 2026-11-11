// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Helpers shared by the profile tests.
//!
//! Every profile must prove that its report descriptor declares exactly the
//! bytes it sends. Parsing the descriptor once here keeps that check in step
//! across profiles and avoids five slightly different copies of the same loop.

/// Sum the input-report bits declared for one report ID.
///
/// `report_id` is `None` for a descriptor with no Report ID item (the generic
/// PC profile). The parser follows the HID short-item encoding: prefix, then
/// 0-4 data bytes in little-endian order.
pub fn input_bits(descriptor: &[u8], report_id: Option<u8>) -> u32 {
    let (mut size, mut count, mut current_id) = (0u32, 0u32, None::<u8>);
    let mut bits = 0u32;
    let mut i = 0;
    while i < descriptor.len() {
        let prefix = descriptor[i];
        let len = match prefix & 0x03 {
            3 => 4,
            n => n as usize,
        };
        if i + 1 + len > descriptor.len() {
            break;
        }
        let value = descriptor[i + 1..i + 1 + len]
            .iter()
            .rev()
            .fold(0u32, |v, b| v << 8 | *b as u32);
        match prefix & 0xFC {
            0x74 => size = value,
            0x94 => count = value,
            0x84 => current_id = Some(value as u8),
            // Input item: the low two bits encode its data length, while
            // bits 2-3 select Input/Output/Feature; 0x80 is Input.
            0x80 if current_id == report_id => {
                bits += size * count;
            }
            _ => {}
        }
        i += 1 + len;
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_id_scopes_the_bits_it_declares() {
        // Report 1: one byte input; report 2: two bytes input.
        let descriptor = [
            0x85, 0x01, 0x75, 0x08, 0x95, 0x01, 0x81, 0x02, 0x85, 0x02, 0x75, 0x08, 0x95, 0x02,
            0x81, 0x02,
        ];
        assert_eq!(input_bits(&descriptor, Some(1)), 8);
        assert_eq!(input_bits(&descriptor, Some(2)), 16);
        assert_eq!(input_bits(&descriptor, Some(3)), 0);
    }

    #[test]
    fn a_descriptor_without_a_report_id_sums_every_input_item() {
        let descriptor = [0x75, 0x08, 0x95, 0x03, 0x81, 0x02];
        assert_eq!(input_bits(&descriptor, None), 24);
    }
}
