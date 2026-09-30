//! Tests against resources built in-process.

use super::*;

/// A minimal compiled resource with one block, built the way the format nests offsets:
/// both the block-table offset and each block's data offset are relative to the field
/// that holds them.
fn build_resource(blocks: &[([u8; 4], &[u8])]) -> Vec<u8> {
    let header = 16;
    let table = blocks.len() * 12;
    let mut out = vec![0u8; header + table];
    let mut payloads = Vec::new();
    for (i, (kind, body)) in blocks.iter().enumerate() {
        let at = header + i * 12;
        let data_at = header + table + payloads.len();
        out[at..at + 4].copy_from_slice(kind);
        // Relative to the offset field itself, which sits at `at + 4`.
        let rel = (data_at - (at + 4)) as u32;
        out[at + 4..at + 8].copy_from_slice(&rel.to_le_bytes());
        out[at + 8..at + 12].copy_from_slice(&(body.len() as u32).to_le_bytes());
        payloads.extend_from_slice(body);
    }
    out.extend_from_slice(&payloads);

    let total = out.len() as u32;
    out[0..4].copy_from_slice(&total.to_le_bytes());
    out[4..6].copy_from_slice(&12u16.to_le_bytes());
    out[6..8].copy_from_slice(&0u16.to_le_bytes());
    // Block table starts right after the header; the offset is relative to byte 8.
    out[8..12].copy_from_slice(&((header - 8) as u32).to_le_bytes());
    out[12..16].copy_from_slice(&(blocks.len() as u32).to_le_bytes());
    out
}

#[test]
fn resource_block_offsets_are_relative_to_their_own_field() {
    let bytes = build_resource(&[
        (*b"RERL", b"refs"),
        (*b"DATA", b"the payload"),
        (*b"FLCI", b"index"),
    ]);
    let r = Resource::parse(&bytes).expect("parse");
    assert_eq!(r.header_version(), 12);
    assert_eq!(r.blocks().len(), 3);
    assert_eq!(r.block(*b"RERL"), Some(&b"refs"[..]));
    assert_eq!(r.data().unwrap(), b"the payload");
    assert_eq!(r.blocks()[2].name(), "FLCI");
}

#[test]
fn a_resource_whose_size_field_disagrees_is_rejected() {
    let mut bytes = build_resource(&[(*b"DATA", b"x")]);
    bytes[0] = bytes[0].wrapping_add(1);
    assert!(matches!(Resource::parse(&bytes), Err(Error::Malformed(_))));
}

#[test]
fn a_resource_with_no_data_block_reports_it() {
    let bytes = build_resource(&[(*b"RERL", b"refs")]);
    let r = Resource::parse(&bytes).expect("parse");
    assert!(r.block(Block::DATA).is_none());
    assert!(r.data().is_err());
}

#[test]
fn a_block_pointing_past_the_end_is_rejected() {
    let mut bytes = build_resource(&[(*b"DATA", b"payload")]);
    bytes[24..28].copy_from_slice(&9999u32.to_le_bytes());
    assert!(matches!(Resource::parse(&bytes), Err(Error::Malformed(_))));
}
