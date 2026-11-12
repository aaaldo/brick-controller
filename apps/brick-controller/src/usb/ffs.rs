// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! FunctionFS descriptor blocks for the wired HID function.
//!
//! FunctionFS counts descriptors, not bytes: the block advertises three
//! descriptors for the full-speed and high-speed configurations, and the
//! kernel serialises them into the device's configuration descriptor. The
//! report descriptor itself is passed by the PC profile so there is one source
//! of truth for the nine-byte report.

/// `bInterfaceClass = HID`, one endpoint, no subclass or protocol.
pub const INTERFACE: [u8; 9] = [
    9, 4, 0, 0, 1, 3, 0, 0, 0, // length, interface, alt, endpoints, class, ...
];

/// Interrupt IN, 64 bytes, 1 ms. The report is 9 bytes; the endpoint size
/// leaves room for a host that reads a full packet.
pub const ENDPOINT_IN: [u8; 7] = [
    7, 5, 0x81, 3, 0x40, 0x00, 1, // length, endpoint, address, interrupt, ...
];

/// The three descriptors repeated for FS and HS: interface, HID, endpoint.
pub const DESCRIPTOR_COUNT: u32 = 3;

const MAGIC_V2: u32 = 3;
const FLAGS_FS_AND_HS: u32 = 3;
const STRINGS_MAGIC: u32 = 2;

/// The HID class descriptor declared inside the interface.
pub fn hid_descriptor(report_len: u16) -> [u8; 9] {
    [
        9,
        0x21, // HID_DT_HID
        0x11,
        0x01, // bcdHID 1.11
        0x00, // bCountryCode
        0x01, // bNumDescriptors
        0x22, // HID_DT_REPORT
        (report_len & 0xFF) as u8,
        (report_len >> 8) as u8,
    ]
}

/// The `FUNCTIONFS_DESCRIPTORS_MAGIC_V2` block.
pub fn descriptors(report_len: u16) -> Vec<u8> {
    let mut fs = Vec::with_capacity(25);
    fs.extend_from_slice(&INTERFACE);
    fs.extend_from_slice(&hid_descriptor(report_len));
    fs.extend_from_slice(&ENDPOINT_IN);
    // The same layout is valid at both speeds; the kernel picks one.
    let hs = fs.clone();

    let total = 20 + fs.len() + hs.len();
    let mut block = Vec::with_capacity(total);
    block.extend_from_slice(&MAGIC_V2.to_le_bytes());
    block.extend_from_slice(&(total as u32).to_le_bytes());
    block.extend_from_slice(&FLAGS_FS_AND_HS.to_le_bytes());
    block.extend_from_slice(&DESCRIPTOR_COUNT.to_le_bytes());
    block.extend_from_slice(&DESCRIPTOR_COUNT.to_le_bytes());
    block.extend_from_slice(&fs);
    block.extend_from_slice(&hs);
    block
}

/// The `FUNCTIONFS_STRINGS_MAGIC` block.
///
/// Device strings come from configfs, so the function itself declares none.
pub fn strings() -> Vec<u8> {
    let mut block = Vec::with_capacity(16);
    block.extend_from_slice(&STRINGS_MAGIC.to_le_bytes());
    block.extend_from_slice(&16u32.to_le_bytes());
    block.extend_from_slice(&0u32.to_le_bytes());
    block.extend_from_slice(&0u32.to_le_bytes());
    block
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT_LEN: u16 = 9;

    #[test]
    fn the_hid_descriptor_declares_the_profile_report_length() {
        let hid = hid_descriptor(75);
        assert_eq!(hid[0], 9);
        assert_eq!(hid[1], 0x21);
        assert_eq!(hid[6], 0x22);
        assert_eq!(u16::from_le_bytes([hid[7], hid[8]]), 75);
    }

    #[test]
    fn the_descriptor_block_counts_descriptors_not_bytes() {
        let block = descriptors(REPORT_LEN);
        assert_eq!(u32::from_le_bytes(block[0..4].try_into().unwrap()), 3);
        assert_eq!(
            u32::from_le_bytes(block[4..8].try_into().unwrap()) as usize,
            block.len()
        );
        assert_eq!(u32::from_le_bytes(block[8..12].try_into().unwrap()), 3);
        assert_eq!(u32::from_le_bytes(block[12..16].try_into().unwrap()), 3);
        assert_eq!(u32::from_le_bytes(block[16..20].try_into().unwrap()), 3);
    }

    #[test]
    fn the_interface_hid_and_endpoint_are_in_the_block() {
        let block = descriptors(REPORT_LEN);
        let fs = &block[20..45];
        assert_eq!(&fs[..9], &INTERFACE);
        assert_eq!(&fs[9..18], &hid_descriptor(REPORT_LEN));
        assert_eq!(&fs[18..25], &ENDPOINT_IN);
    }

    #[test]
    fn the_strings_block_declares_no_strings() {
        let block = strings();
        assert_eq!(u32::from_le_bytes(block[0..4].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(block[4..8].try_into().unwrap()), 16);
        assert_eq!(block.len(), 16);
    }
}
