//! Tests against resources built in-process.
//!
//! `raw` is an independent encoder: it lays bytes out from the format description alone,
//! so parse and write are checked against it and not only against each other.

use super::*;

struct RawBlock<'a> {
    kind: [u8; 4],
    gap: &'a [u8],
    data: &'a [u8],
}

fn rb<'a>(kind: &[u8; 4], gap: &'a [u8], data: &'a [u8]) -> RawBlock<'a> {
    RawBlock {
        kind: *kind,
        gap,
        data,
    }
}

/// Header, `pre_table` bytes, table, then each block preceded by its gap, then `trailing`.
/// `size` overrides the stored size field.
fn raw(
    header_version: u16,
    resource_version: u16,
    pre_table: &[u8],
    blocks: &[RawBlock<'_>],
    trailing: &[u8],
    size: Option<u32>,
) -> Vec<u8> {
    let table_start = 16 + pre_table.len();
    let mut out = vec![0u8; 16];
    out.extend_from_slice(pre_table);
    out.resize(table_start + blocks.len() * 12, 0);
    let mut body = Vec::new();
    let body_start = out.len();
    for (i, b) in blocks.iter().enumerate() {
        body.extend_from_slice(b.gap);
        let data_at = body_start + body.len();
        let entry = table_start + i * 12;
        out[entry..entry + 4].copy_from_slice(&b.kind);
        let rel = (data_at - (entry + 4)) as u32;
        out[entry + 4..entry + 8].copy_from_slice(&rel.to_le_bytes());
        out[entry + 8..entry + 12].copy_from_slice(&(b.data.len() as u32).to_le_bytes());
        body.extend_from_slice(b.data);
    }
    out.extend_from_slice(&body);
    let end = out.len() as u32;
    out.extend_from_slice(trailing);
    out[0..4].copy_from_slice(&size.unwrap_or(end).to_le_bytes());
    out[4..6].copy_from_slice(&header_version.to_le_bytes());
    out[6..8].copy_from_slice(&resource_version.to_le_bytes());
    out[8..12].copy_from_slice(&((table_start - 8) as u32).to_le_bytes());
    out[12..16].copy_from_slice(&(blocks.len() as u32).to_le_bytes());
    out
}

fn plain(blocks: &[RawBlock<'_>]) -> Vec<u8> {
    raw(12, 0, &[], blocks, &[], None)
}

fn assert_identity(bytes: &[u8]) -> Resource {
    let parsed = Resource::parse(bytes).expect("parse");
    assert_eq!(parsed.to_bytes().expect("write"), bytes);
    parsed
}

#[test]
fn block_offsets_are_relative_to_their_own_field() {
    let bytes = plain(&[
        rb(b"RERL", &[], b"refs"),
        rb(b"DATA", &[], b"the payload"),
        rb(b"FLCI", &[], b"index"),
    ]);
    let r = Resource::parse(&bytes).expect("parse");
    assert_eq!(r.versions.header, 12);
    assert_eq!(r.blocks.len(), 3);
    assert_eq!(r.block(BlockKind::RERL).unwrap().data, b"refs");
    assert_eq!(r.data().unwrap(), b"the payload");
    assert_eq!(r.blocks[2].kind, BlockKind::FLCI);
    assert_eq!(r.blocks[2].kind.to_string(), "FLCI");
}

#[test]
fn bytes_to_model_to_bytes_is_identity_for_each_layout() {
    let zeros = [0u8; 16];
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", plain(&[])),
        (
            "tight blocks",
            plain(&[rb(b"RED2", &[], b"abc"), rb(b"DATA", &[], b"defgh")]),
        ),
        (
            "aligned zero gaps",
            plain(&[
                rb(b"RED2", &zeros[..12], b"abc"),
                rb(b"DATA", &zeros[..13], b"defgh"),
            ]),
        ),
        (
            "non-zero gap bytes",
            plain(&[rb(b"RED2", &[1, 2, 3], b"abc"), rb(b"DATA", &[9], b"de")]),
        ),
        (
            "zero-length block",
            plain(&[rb(b"DATA", &[], b""), rb(b"CTRL", &[], b"ctrl")]),
        ),
        (
            "duplicate tags",
            plain(&[rb(b"DATA", &[], b"a"), rb(b"DATA", &[], b"b")]),
        ),
        ("unknown tag", plain(&[rb(b"ZZzz", &[], b"a")])),
        (
            "bytes before the table",
            raw(
                12,
                3,
                &[7, 7, 7, 7, 7],
                &[rb(b"DATA", &[], b"a")],
                &[],
                None,
            ),
        ),
        (
            "trailing data",
            raw(
                12,
                5,
                &[],
                &[rb(b"DATA", &[], b"a")],
                &[1, 2, 3, 4, 5],
                None,
            ),
        ),
        (
            "size smaller than the file",
            raw(12, 5, &[], &[rb(b"DATA", &[], b"abc")], &[9; 40], Some(20)),
        ),
        (
            "size larger than the file",
            raw(12, 5, &[], &[rb(b"DATA", &[], b"abc")], &[], Some(5000)),
        ),
    ];
    for (name, bytes) in cases {
        let parsed = Resource::parse(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(parsed.to_bytes().unwrap(), bytes, "{name}");
        let again = Resource::parse(&parsed.to_bytes().unwrap()).unwrap();
        assert_eq!(again, parsed, "{name}");
    }
}

#[test]
fn model_to_bytes_to_model_is_identity_for_each_resource_version() {
    for version in [0u16, 1, 2, 3, 4, 5, 18, u16::MAX] {
        let mut r = Resource::new();
        r.versions.resource = version;
        r.push_block(BlockKind::RERL, vec![1; 5]);
        r.push_block(BlockKind::RED2, vec![2; 17]);
        r.push_block(BlockKind::DATA, vec![3; 33]);
        r.trailing = vec![4; 9];
        let bytes = r.to_bytes().unwrap();
        let back = Resource::parse(&bytes).unwrap();
        assert_eq!(back, r, "version {version}");
        assert_eq!(back.versions.resource, version);
        assert_eq!(back.to_bytes().unwrap(), bytes);
    }
}

#[test]
fn a_hand_built_resource_uses_16_byte_aligned_blocks() {
    let mut r = Resource::new();
    r.push_block(BlockKind::RERL, b"refs".to_vec());
    r.push_block(BlockKind::DATA, b"payload".to_vec());
    let zeros = [0u8; 16];
    // 16 header + 24 table = 40 -> first block at 48; ends 52 -> next at 64.
    let expected = plain(&[
        rb(b"RERL", &zeros[..8], b"refs"),
        rb(b"DATA", &zeros[..12], b"payload"),
    ]);
    assert_eq!(r.to_bytes().unwrap(), expected);
    assert_eq!(&expected[0..4], &71u32.to_le_bytes());
}

#[test]
fn a_hand_built_resource_equals_the_parse_of_its_bytes() {
    let mut r = Resource::new();
    r.push_block(BlockKind::DATA, vec![1, 2, 3]);
    r.push_block(BlockKind::STAT, vec![4]);
    let parsed = Resource::parse(&r.to_bytes().unwrap()).unwrap();
    assert_eq!(parsed, r);
    assert!(parsed.blocks.iter().all(|b| b.padding == Padding::Auto));
    assert_eq!(parsed.declared_size, None);
}

#[test]
fn trailing_bytes_and_a_short_size_field_are_kept_not_rejected() {
    let bytes = raw(12, 5, &[], &[rb(b"DATA", &[], b"abc")], &[9; 40], None);
    let r = assert_identity(&bytes);
    assert_eq!(r.trailing, vec![9; 40]);
    assert_eq!(r.declared_size, None);

    let bytes = raw(12, 5, &[], &[rb(b"DATA", &[], b"abc")], &[9; 40], Some(7));
    let r = assert_identity(&bytes);
    assert_eq!(r.declared_size, Some(7));
}

#[test]
fn an_edited_resource_rederives_offsets_lengths_and_size() {
    let bytes = plain(&[rb(b"RERL", &[], b"refs"), rb(b"DATA", &[], b"x")]);
    let mut r = Resource::parse(&bytes).unwrap();
    r.blocks[0].data = vec![0xaa; 100];
    let out = r.to_bytes().unwrap();
    let back = Resource::parse(&out).unwrap();
    assert_eq!(back.blocks[0].data, vec![0xaa; 100]);
    assert_eq!(back.data().unwrap(), b"x");
    assert_eq!(back.declared_size, None);
    assert_eq!(
        u32::from_le_bytes(out[0..4].try_into().unwrap()) as usize,
        out.len()
    );
}

#[test]
fn write_and_read_work_over_io_traits() {
    let mut r = Resource::new();
    r.push_block(BlockKind::DATA, b"payload".to_vec());
    let mut sink = Vec::new();
    r.write(&mut sink).unwrap();
    assert_eq!(sink, r.to_bytes().unwrap());
    let back = Resource::read(&sink[..]).unwrap();
    assert_eq!(back, r);
}

#[test]
fn a_failing_writer_is_an_io_error() {
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("nope"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut r = Resource::new();
    r.push_block(BlockKind::DATA, vec![1]);
    let err = r.write(Broken).unwrap_err();
    assert!(matches!(err, Error::Io(_)));
    assert!(std::error::Error::source(&err).is_some());
}

#[test]
fn other_header_versions_are_unsupported_in_both_directions() {
    for v in [0u16, 1, 11, 13, 100, u16::MAX] {
        let bytes = raw(v, 0, &[], &[rb(b"DATA", &[], b"a")], &[], None);
        match Resource::parse(&bytes) {
            Err(Error::UnsupportedVersion { found }) => assert_eq!(found, v),
            other => panic!("version {v}: {other:?}"),
        }
        let mut r = Resource::new();
        r.versions.header = v;
        match r.to_bytes() {
            Err(Error::UnsupportedVersion { found }) => assert_eq!(found, v),
            other => panic!("version {v}: {other:?}"),
        }
    }
}

#[test]
fn a_short_input_is_truncated() {
    for len in 0..16 {
        let err = Resource::parse(&vec![0u8; len]).unwrap_err();
        assert!(
            matches!(err, Error::Truncated { what: "header", .. }),
            "len {len}: {err:?}"
        );
    }
}

#[test]
fn a_huge_block_count_is_rejected_before_allocating() {
    let mut bytes = plain(&[rb(b"DATA", &[], b"x")]);
    for count in [u32::MAX, 0x2000_0000, 1 << 24, 3] {
        bytes[12..16].copy_from_slice(&count.to_le_bytes());
        match Resource::parse(&bytes) {
            Err(Error::Truncated {
                what: "block table",
                needed,
                available,
            }) => {
                assert!(needed > available, "count {count}");
                assert_eq!(available, bytes.len() as u64);
            }
            other => panic!("count {count}: {other:?}"),
        }
    }
}

#[test]
fn a_table_offset_far_past_the_end_is_rejected() {
    let mut bytes = plain(&[rb(b"DATA", &[], b"x")]);
    bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        Resource::parse(&bytes),
        Err(Error::Truncated {
            what: "block table",
            ..
        })
    ));
}

#[test]
fn a_table_overlapping_the_header_is_a_bad_layout() {
    let mut bytes = plain(&[rb(b"DATA", &[], b"x")]);
    bytes[8..12].copy_from_slice(&7u32.to_le_bytes());
    assert!(matches!(
        Resource::parse(&bytes),
        Err(Error::BadLayout { index: None, .. })
    ));
}

#[test]
fn a_block_pointing_past_the_end_is_a_bad_offset() {
    let mut bytes = plain(&[rb(b"DATA", &[], b"payload")]);
    bytes[24..28].copy_from_slice(&9999u32.to_le_bytes());
    match Resource::parse(&bytes) {
        Err(Error::BadOffset {
            index: 0,
            kind,
            available,
            ..
        }) => {
            assert_eq!(kind, BlockKind::DATA);
            assert_eq!(available, bytes.len() as u64);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_block_length_or_offset_of_u32_max_does_not_overflow() {
    let base = plain(&[rb(b"DATA", &[], b"payload")]);
    let mut long = base.clone();
    long[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        Resource::parse(&long),
        Err(Error::BadOffset { .. })
    ));
    let mut far = base;
    far[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        Resource::parse(&far),
        Err(Error::BadOffset { .. })
    ));
}

#[test]
fn blocks_overlapping_the_table_or_each_other_are_bad_layouts() {
    let base = plain(&[rb(b"RERL", &[], b"aaaa"), rb(b"DATA", &[], b"bbbb")]);

    // First block starts inside the table: its offset field is at 20, data must be >= 40.
    let mut into_table = base.clone();
    into_table[20..24].copy_from_slice(&0u32.to_le_bytes());
    assert!(matches!(
        Resource::parse(&into_table),
        Err(Error::BadLayout { index: Some(0), .. })
    ));

    // Second block starts before the first one ends.
    let mut overlap = base;
    let first = u32::from_le_bytes(overlap[20..24].try_into().unwrap());
    // Entry 1's offset field is at 32; point it at the first block's start.
    overlap[32..36].copy_from_slice(&(first.wrapping_sub(12)).to_le_bytes());
    assert!(matches!(
        Resource::parse(&overlap),
        Err(Error::BadLayout { index: Some(1), .. })
    ));
}

#[test]
fn no_prefix_or_single_byte_corruption_panics() {
    let mut r = Resource::new();
    r.push_block(BlockKind::RERL, vec![1; 7]);
    r.push_block(BlockKind::DATA, vec![2; 30]);
    r.trailing = vec![3; 5];
    let bytes = r.to_bytes().unwrap();
    for len in 0..bytes.len() {
        let _ = Resource::parse(&bytes[..len]);
    }
    for at in 0..bytes.len() {
        for flip in [0x01u8, 0x80, 0xff] {
            let mut copy = bytes.clone();
            copy[at] ^= flip;
            if let Ok(parsed) = Resource::parse(&copy) {
                assert_eq!(parsed.to_bytes().unwrap(), copy, "byte {at} ^ {flip:#x}");
            }
        }
    }
}

#[test]
fn block_kinds_display_debug_and_convert() {
    assert_eq!(BlockKind::DATA.as_bytes(), b"DATA");
    assert_eq!(BlockKind::from(*b"RED2"), BlockKind::RED2);
    assert_eq!(BlockKind::new(*b"LaCo"), BlockKind::LACO);
    assert_eq!(BlockKind::SRMA.to_string(), "SrMa");
    assert_eq!(format!("{:?}", BlockKind::NTRO), "BlockKind(\"NTRO\")");
    assert_eq!(
        BlockKind([0xff, b'A', 0, b'B']).to_string().chars().count(),
        4
    );
}

#[test]
fn errors_display_and_are_thread_safe() {
    fn assert_traits<T: std::error::Error + Send + Sync + 'static>() {}
    assert_traits::<Error>();
    let err = Resource::parse(&[0; 3]).unwrap_err();
    assert!(err.to_string().contains("header"));
    let err = Error::UnsupportedVersion { found: 9 };
    assert!(err.to_string().contains('9'));
}
