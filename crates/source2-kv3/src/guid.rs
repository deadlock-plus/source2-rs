//! GUIDs as the text header spells them and the binary header stores them.
//!
//! Binary headers hold a GUID in Microsoft layout: the first three groups little-endian, the
//! last two as written.

use std::fmt::Write as _;

/// The generic format, `7412167c-06e9-4698-aff2-e63eb59037e7`.
pub const GENERIC_FORMAT: [u8; 16] = [
    0x7C, 0x16, 0x12, 0x74, 0xE9, 0x06, 0x98, 0x46, 0xAF, 0xF2, 0xE6, 0x3E, 0xB5, 0x90, 0x37, 0xE7,
];

/// The text encoding, `e21c7f3c-8a33-41c5-9977-a76d3a32aa0d`.
pub const TEXT_ENCODING: [u8; 16] = [
    0x3C, 0x7F, 0x1C, 0xE2, 0x33, 0x8A, 0xC5, 0x41, 0x99, 0x77, 0xA7, 0x6D, 0x3A, 0x32, 0xAA, 0x0D,
];

/// Spell a GUID the way text headers do: lowercase, `8-4-4-4-12`.
pub(crate) fn format_guid(g: &[u8; 16]) -> String {
    let order = [3, 2, 1, 0, 5, 4, 7, 6, 8, 9, 10, 11, 12, 13, 14, 15];
    let mut s = String::with_capacity(36);
    for (i, &at) in order.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            s.push('-');
        }
        write!(s, "{:02x}", g[at]).expect("writing to a String cannot fail");
    }
    s
}

/// Read a GUID spelled `8-4-4-4-12` in hexadecimal.
pub(crate) fn parse_guid(text: &str) -> Option<[u8; 16]> {
    let lens = [8, 4, 4, 4, 12];
    let mut groups = text.split('-');
    let mut hex = Vec::with_capacity(16);
    for len in lens {
        let g = groups.next()?;
        if g.len() != len || !g.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        hex.push(
            g.as_bytes()
                .chunks(2)
                .map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok())
                .collect::<Option<Vec<u8>>>()?,
        );
    }
    if groups.next().is_some() {
        return None;
    }
    let mut out = [0u8; 16];
    out[..4].copy_from_slice(&[hex[0][3], hex[0][2], hex[0][1], hex[0][0]]);
    out[4..6].copy_from_slice(&[hex[1][1], hex[1][0]]);
    out[6..8].copy_from_slice(&[hex[2][1], hex[2][0]]);
    out[8..10].copy_from_slice(&hex[3]);
    out[10..].copy_from_slice(&hex[4]);
    Some(out)
}
