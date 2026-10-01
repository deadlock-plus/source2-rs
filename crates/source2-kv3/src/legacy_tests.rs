//! Tests for the pre-v4 revisions: the original `VKV\x03` encoding, `KV3\x01`, `KV3\x02`
//! and `KV3\x03`.

use crate::tests::lz4_literal_block;
use crate::value::{ArrayForm, Storage};
use crate::{
    Compression, Header, MAGIC_LEGACY, MAGIC_V1, MAGIC_V2, MAGIC_V3, Object, Value, Version,
    WriteOptions, decode, parse, write,
};

const FORMAT: [u8; 16] = [
    0x7C, 0x16, 0x12, 0x74, 0xE9, 0x06, 0x98, 0x46, 0xAF, 0xF2, 0xE6, 0x3E, 0xB5, 0x90, 0x37, 0xE7,
];
const ENCODING_BLOCK: [u8; 16] = [
    0x46, 0x1A, 0x79, 0x95, 0xBC, 0x95, 0x6C, 0x4F, 0xA7, 0x0B, 0x05, 0xBC, 0xA1, 0xB7, 0xDF, 0xD2,
];
const ENCODING_LZ4: [u8; 16] = [
    0x8A, 0x34, 0x47, 0x68, 0xA1, 0x63, 0x5C, 0x4F, 0xA1, 0x97, 0x53, 0x80, 0x6F, 0xD9, 0xB1, 0x19,
];
const ENCODING_RAW: [u8; 16] = [
    0x00, 0x05, 0x86, 0x1B, 0xD8, 0xF7, 0xC1, 0x40, 0xAD, 0x82, 0x75, 0xA4, 0x82, 0x67, 0xE7, 0x14,
];

fn u32le(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

fn object(members: Vec<(&str, Value)>) -> Value {
    let mut o = Object::default();
    for (k, v) in members {
        o.push(k.to_string(), v);
    }
    Value::from(o)
}

/// The document every fixture below encodes.
fn sample() -> Value {
    object(vec![
        ("name", Value::from("unit")),
        ("count", Value::int(7)),
        ("pi", Value::double(1.5)),
        ("flag", Value::from(true)),
        (
            "list",
            Value::array(vec![Value::int(1), Value::int(2), Value::int(3)]),
        ),
    ])
}

// ---------------------------------------------------------------------------------------
// Original encoding: one inline stream, strings first, `0xFFFFFFFF` at the end.
// ---------------------------------------------------------------------------------------

const SAMPLE_STRINGS: [&str; 6] = ["name", "count", "pi", "flag", "list", "unit"];

fn legacy_sample_body() -> Vec<u8> {
    let mut b = vec![9];
    b.extend(u32le(5));
    b.extend(u32le(0));
    b.push(6);
    b.extend(u32le(5));
    b.extend(u32le(1));
    b.push(3);
    b.extend(7i64.to_le_bytes());
    b.extend(u32le(2));
    b.push(5);
    b.extend(1.5f64.to_le_bytes());
    b.extend(u32le(3));
    b.push(13);
    b.extend(u32le(4));
    b.push(10);
    b.extend(u32le(3));
    b.push(11);
    for v in [1i32, 2, 3] {
        b.extend(v.to_le_bytes());
    }
    b
}

fn legacy_stream(strings: &[&str], body: &[u8]) -> Vec<u8> {
    let mut out = u32le(strings.len() as u32).to_vec();
    for s in strings {
        out.extend(s.as_bytes());
        out.push(0);
    }
    out.extend(body);
    out.extend(u32le(0xFFFF_FFFF));
    out
}

fn legacy_header(encoding: [u8; 16]) -> Vec<u8> {
    let mut out = b"VKV\x03".to_vec();
    out.extend(encoding);
    out.extend(FORMAT);
    out
}

fn legacy_raw(stream: &[u8]) -> Vec<u8> {
    let mut out = legacy_header(ENCODING_RAW);
    out.extend(stream);
    out
}

fn legacy_lz4(stream: &[u8]) -> Vec<u8> {
    let mut out = legacy_header(ENCODING_LZ4);
    out.extend(u32le(stream.len() as u32));
    out.extend(lz4_literal_block(stream));
    out
}

enum Piece<'a> {
    Literal(&'a [u8]),
    /// Copy `size` bytes from `offset` bytes back.
    Copy {
        offset: usize,
        size: usize,
    },
}

/// Valve's byte-oriented block scheme: a 16-bit mask, then one item per bit, low bit first.
/// A set bit is a 16-bit copy: offset minus one in the top twelve bits, size minus three in
/// the low four.
fn valve_block(pieces: &[Piece]) -> Vec<u8> {
    let mut items: Vec<Vec<u8>> = Vec::new();
    let mut flags: Vec<bool> = Vec::new();
    for piece in pieces {
        match piece {
            Piece::Literal(bytes) => {
                for &b in *bytes {
                    items.push(vec![b]);
                    flags.push(false);
                }
            }
            Piece::Copy { offset, size } => {
                let v = (((offset - 1) << 4) | (size - 3)) as u16;
                items.push(v.to_le_bytes().to_vec());
                flags.push(true);
            }
        }
    }
    let mut out = Vec::new();
    for (chunk, flag_chunk) in items.chunks(16).zip(flags.chunks(16)) {
        let mut mask = 0u16;
        for (i, &f) in flag_chunk.iter().enumerate() {
            if f {
                mask |= 1 << i;
            }
        }
        out.extend(mask.to_le_bytes());
        for item in chunk {
            out.extend(item);
        }
    }
    out
}

fn legacy_block(stream: &[u8]) -> Vec<u8> {
    let mut out = legacy_header(ENCODING_BLOCK);
    out.extend(u32le(stream.len() as u32));
    out.extend(valve_block(&[Piece::Literal(stream)]));
    out
}

#[test]
fn legacy_header_reports_revision_zero_and_the_encoding() {
    let stream = legacy_stream(&SAMPLE_STRINGS, &legacy_sample_body());
    for (block, compression) in [
        (legacy_raw(&stream), Compression::None),
        (legacy_lz4(&stream), Compression::Lz4),
        (legacy_block(&stream), Compression::Block),
    ] {
        let h = Header::parse(&block).expect("header");
        assert_eq!(h.version, Version::Legacy);
        assert_eq!(h.compression, compression);
        assert_eq!(h.format, FORMAT);
        assert_eq!(h.uncompressed_size as usize, stream.len());
    }
}

#[test]
fn legacy_raw_document_parses() {
    let block = legacy_raw(&legacy_stream(&SAMPLE_STRINGS, &legacy_sample_body()));
    assert_eq!(parse(&block).expect("parse").root, sample());
}

#[cfg(feature = "lz4")]
#[test]
fn legacy_lz4_document_parses() {
    let block = legacy_lz4(&legacy_stream(&SAMPLE_STRINGS, &legacy_sample_body()));
    assert_eq!(parse(&block).expect("parse").root, sample());
}

#[test]
fn legacy_block_compressed_document_parses() {
    let block = legacy_block(&legacy_stream(&SAMPLE_STRINGS, &legacy_sample_body()));
    assert_eq!(parse(&block).expect("parse").root, sample());
}

#[test]
fn legacy_block_scheme_expands_copies_including_overlapping_ones() {
    let mut block = legacy_header(ENCODING_BLOCK);
    let want = b"abcabcabc-xxxxxx";
    block.extend(u32le(want.len() as u32));
    block.extend(valve_block(&[
        Piece::Literal(b"abc"),
        Piece::Copy { offset: 3, size: 6 },
        Piece::Literal(b"-x"),
        Piece::Copy { offset: 1, size: 5 },
    ]));
    assert_eq!(decode(&block).expect("decode").payload, want);
}

#[test]
fn legacy_block_scheme_copies_reach_back_the_full_window() {
    let mut data = vec![b'a'];
    data.extend((0..4095u32).map(|i| (i % 251) as u8 + 1));
    let mut block = legacy_header(ENCODING_BLOCK);
    let mut want = data.clone();
    want.extend_from_slice(&data[..3]);
    block.extend(u32le(want.len() as u32));
    block.extend(valve_block(&[
        Piece::Literal(&data),
        Piece::Copy {
            offset: 4096,
            size: 3,
        },
    ]));
    assert_eq!(decode(&block).expect("decode").payload, want);
}

#[test]
fn legacy_block_with_the_uncompressed_flag_is_stored_as_is() {
    let stream = legacy_stream(&[], &[9, 0, 0, 0, 0]);
    let mut block = legacy_header(ENCODING_BLOCK);
    block.extend(u32le(0x8000_0000 | stream.len() as u32));
    block.extend(&stream);
    let doc = parse(&block).expect("parse");
    assert_eq!(doc.root, Value::from(Object::default()));
    assert_eq!(decode(&block).expect("decode").payload, stream);
}

#[test]
fn legacy_block_size_that_disagrees_with_the_output_is_an_error() {
    let stream = legacy_stream(&SAMPLE_STRINGS, &legacy_sample_body());
    let mut block = legacy_block(&stream);
    block[36..40].copy_from_slice(&u32le(stream.len() as u32 + 10));
    assert!(decode(&block).is_err());
}

#[test]
fn legacy_lz4_size_that_disagrees_with_the_output_is_an_error() {
    let stream = legacy_stream(&SAMPLE_STRINGS, &legacy_sample_body());
    let mut block = legacy_lz4(&stream);
    block[36..40].copy_from_slice(&u32le(stream.len() as u32 + 1));
    assert!(decode(&block).is_err());
}

#[test]
fn legacy_unknown_encoding_is_an_error() {
    let mut block = legacy_raw(&legacy_stream(&[], &[1]));
    block[4] ^= 0xFF;
    assert!(parse(&block).is_err());
}

#[test]
fn legacy_stream_without_its_end_marker_is_an_error() {
    let mut stream = legacy_stream(&SAMPLE_STRINGS, &legacy_sample_body());
    stream.truncate(stream.len() - 4);
    assert!(parse(&legacy_raw(&stream)).is_err());
}

#[test]
fn legacy_stream_with_trailing_bytes_is_an_error() {
    let mut stream = legacy_stream(&SAMPLE_STRINGS, &legacy_sample_body());
    stream.push(0);
    assert!(parse(&legacy_raw(&stream)).is_err());
}

#[test]
fn legacy_truncated_value_is_an_error() {
    let body = legacy_sample_body();
    let stream = legacy_stream(&SAMPLE_STRINGS, &body[..body.len() - 6]);
    assert!(parse(&legacy_raw(&stream)).is_err());
}

#[test]
fn legacy_string_index_out_of_range_is_an_error() {
    let mut body = vec![9];
    body.extend(u32le(1));
    body.extend(u32le(9));
    body.push(1);
    assert!(parse(&legacy_raw(&legacy_stream(&["a"], &body))).is_err());
}

#[test]
fn legacy_empty_string_index_is_the_empty_string() {
    let mut body = vec![9];
    body.extend(u32le(1));
    body.extend(u32le(0));
    body.push(6);
    body.extend(u32le(0xFFFF_FFFF));
    let doc = parse(&legacy_raw(&legacy_stream(&["k"], &body))).expect("parse");
    assert_eq!(doc.root.get("k"), Some(&Value::from(String::new())));
}

#[test]
fn legacy_flag_byte_is_kept() {
    let mut body = vec![9];
    body.extend(u32le(1));
    body.extend(u32le(0));
    body.extend([0x86, 0x01]);
    body.extend(u32le(1));
    let doc = parse(&legacy_raw(&legacy_stream(&["k", "v"], &body))).expect("parse");
    let member = doc.root.get("k").expect("member k");
    assert_eq!(member.flags(), 1);
    assert_eq!(member.as_str(), Some("v"));
}

#[test]
fn legacy_typed_arrays_keep_their_form_and_element_flags() {
    let mut body = vec![10];
    body.extend(u32le(2));
    body.extend([0x84, 0x02]);
    body.extend(5u64.to_le_bytes());
    body.extend(6u64.to_le_bytes());
    let doc = parse(&legacy_raw(&legacy_stream(&[], &body))).expect("parse");
    assert_eq!(
        doc.root.storage(),
        Storage::Array(ArrayForm::Typed { ty: 4, flags: 2 })
    );
    let items = doc.root.as_array().expect("root is an array");
    assert_eq!(items.len(), 2);
    assert_eq!(items[1], Value::uint(6).with_flags(2));
}

#[test]
fn legacy_constant_types_take_no_operand() {
    let mut body = vec![8];
    body.extend(u32le(7));
    body.extend([1, 14, 15, 16, 17, 18, 13]);
    let doc = parse(&legacy_raw(&legacy_stream(&[], &body))).expect("parse");
    assert_eq!(
        doc.root,
        Value::array(vec![
            Value::null(),
            Value::from(false),
            Value::int(0),
            Value::int(1),
            Value::double(0.0),
            Value::double(1.0),
            Value::from(true),
        ])
    );
}

#[test]
fn legacy_blob_is_read_inline() {
    let mut body = vec![7];
    body.extend(u32le(3));
    body.extend([1, 2, 3]);
    let doc = parse(&legacy_raw(&legacy_stream(&[], &body))).expect("parse");
    assert_eq!(doc.root, Value::blob(vec![1, 2, 3]));
}

#[test]
fn legacy_nesting_past_the_limit_is_an_error() {
    let mut body = Vec::new();
    for _ in 0..200 {
        body.push(8);
        body.extend(u32le(1));
    }
    body.push(1);
    assert!(parse(&legacy_raw(&legacy_stream(&[], &body))).is_err());
}

#[test]
fn legacy_huge_declared_lengths_fail_without_allocating_them() {
    for ty in [8u8, 9, 7, 10] {
        let mut body = vec![ty];
        body.extend(u32le(u32::MAX));
        assert!(parse(&legacy_raw(&legacy_stream(&[], &body))).is_err());
    }
}

#[test]
fn legacy_string_count_larger_than_the_stream_is_an_error() {
    let mut stream = u32le(u32::MAX).to_vec();
    stream.extend([0, 0xFF, 0xFF, 0xFF, 0xFF]);
    assert!(parse(&legacy_raw(&stream)).is_err());
}

// ---------------------------------------------------------------------------------------
// KV3\x01, KV3\x02: header counts, then pools laid out as in v4.
// ---------------------------------------------------------------------------------------

#[derive(Default)]
struct FlatDoc {
    bytes: Vec<u8>,
    ints: Vec<u32>,
    eights: Vec<u64>,
    strings: Vec<&'static str>,
    types: Vec<u8>,
}

impl FlatDoc {
    fn int_count(&self) -> u32 {
        self.ints.len() as u32 + 1
    }

    /// Bytes, 4-aligned ints led by the string count, then eights and strings from the next
    /// 8-byte boundary (eights or not), types, trailer.
    fn payload(&self) -> Vec<u8> {
        let mut out = self.bytes.clone();
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
        out.extend(u32le(self.strings.len() as u32));
        for &i in &self.ints {
            out.extend(u32le(i));
        }
        while !out.len().is_multiple_of(8) {
            out.push(0);
        }
        for &e in &self.eights {
            out.extend(e.to_le_bytes());
        }
        for s in &self.strings {
            out.extend(s.as_bytes());
            out.push(0);
        }
        out.extend(&self.types);
        out.extend(u32le(0xFFEE_DD00));
        out
    }

    fn header(&self, magic: u32, compression: u32, unc: usize) -> Vec<u8> {
        let mut out = magic.to_le_bytes().to_vec();
        out.extend(FORMAT);
        out.extend(u32le(compression));
        if magic != MAGIC_V1 {
            out.extend([0, 0, 0, 0]);
        }
        out.extend(u32le(self.bytes.len() as u32));
        out.extend(u32le(self.int_count()));
        out.extend(u32le(self.eights.len() as u32));
        out.extend(u32le(unc as u32));
        out
    }

    fn block(&self, magic: u32, compression: Compression) -> Vec<u8> {
        let payload = self.payload();
        match compression {
            Compression::None => {
                let mut out = self.header(magic, 0, payload.len());
                out.extend(payload);
                out
            }
            Compression::Lz4 => {
                let mut out = self.header(magic, 1, payload.len());
                out.extend(lz4_literal_block(&payload));
                out
            }
            other => panic!("no fixture for {other:?}"),
        }
    }
}

/// `{ a: 5, b: "hi", c: 2.5 }`.
fn flat_sample() -> FlatDoc {
    FlatDoc {
        ints: vec![3, 0, 5, 1, 2, 3],
        eights: vec![2.5f64.to_bits()],
        strings: vec!["a", "b", "hi", "c"],
        types: vec![9, 11, 6, 5],
        ..FlatDoc::default()
    }
}

fn flat_expected() -> Value {
    object(vec![
        ("a", Value::int(5)),
        ("b", Value::from("hi")),
        ("c", Value::double(2.5)),
    ])
}

#[test]
fn v1_uncompressed_document_parses() {
    let block = flat_sample().block(MAGIC_V1, Compression::None);
    assert_eq!(parse(&block).expect("parse").root, flat_expected());
}

#[cfg(feature = "lz4")]
#[test]
fn v1_lz4_document_parses() {
    let block = flat_sample().block(MAGIC_V1, Compression::Lz4);
    assert_eq!(parse(&block).expect("parse").root, flat_expected());
}

#[test]
fn v1_header_reports_the_counts_and_where_the_payload_starts() {
    let doc = flat_sample();
    let block = doc.block(MAGIC_V1, Compression::Lz4);
    let h = Header::parse(&block).expect("header");
    assert_eq!(h.version, Version::V1);
    assert_eq!(h.format, FORMAT);
    assert_eq!(h.compression, Compression::Lz4);
    assert_eq!(h.binary_byte_count, 0);
    assert_eq!(h.integer_count, 7);
    assert_eq!(h.eight_byte_count, 1);
    assert_eq!(h.uncompressed_size as usize, doc.payload().len());
    assert_eq!(h.payload_offset, 40);
}

#[test]
fn v2_header_reads_the_dictionary_and_frame_fields_and_shifts_the_rest() {
    let doc = flat_sample();
    let mut block = doc.block(MAGIC_V2, Compression::Lz4);
    block[24..26].copy_from_slice(&7u16.to_le_bytes());
    block[26..28].copy_from_slice(&16384u16.to_le_bytes());
    let h = Header::parse(&block).expect("header");
    assert_eq!(h.version, Version::V2);
    assert_eq!(h.dictionary_id, 7);
    assert_eq!(h.frame_size, 16384);
    assert_eq!(h.integer_count, 7);
    assert_eq!(h.eight_byte_count, 1);
    assert_eq!(h.payload_offset, 44);
}

#[cfg(feature = "lz4")]
#[test]
fn v2_documents_parse() {
    for compression in [Compression::None, Compression::Lz4] {
        let block = flat_sample().block(MAGIC_V2, compression);
        assert_eq!(parse(&block).expect("parse").root, flat_expected());
    }
}

#[test]
fn v1_strings_start_on_an_eight_byte_boundary_even_without_eight_byte_values() {
    let doc = FlatDoc {
        ints: vec![1, 4],
        types: vec![8, 11],
        ..FlatDoc::default()
    };
    assert_eq!(doc.int_count(), 3);
    let block = doc.block(MAGIC_V1, Compression::None);
    assert_eq!(
        parse(&block).expect("parse").root,
        Value::array(vec![Value::int(4)])
    );
}

#[test]
fn v1_byte_pool_feeds_booleans_and_small_integers() {
    let doc = FlatDoc {
        bytes: vec![1, 0xFE],
        ints: vec![2, 0, 1, 1],
        strings: vec!["t", "n"],
        types: vec![9, 2, 23],
        ..FlatDoc::default()
    };
    let block = doc.block(MAGIC_V1, Compression::None);
    assert_eq!(
        parse(&block).expect("parse").root,
        object(vec![("t", Value::from(true)), ("n", Value::int(-2))])
    );
}

#[test]
fn v1_typed_arrays_parse() {
    let doc = FlatDoc {
        ints: vec![1, 0, 3, 10, 20, 30],
        strings: vec!["xs"],
        types: vec![9, 10, 11],
        ..FlatDoc::default()
    };
    let parsed = parse(&doc.block(MAGIC_V1, Compression::None)).expect("parse");
    assert_eq!(
        parsed.root.get("xs"),
        Some(&Value::array(vec![
            Value::int(10),
            Value::int(20),
            Value::int(30)
        ]))
    );
}

#[test]
fn v1_flag_bytes_follow_the_type_code() {
    let doc = FlatDoc {
        ints: vec![1, 0, 0],
        strings: vec!["k"],
        types: vec![9, 0x86, 0x01],
        ..FlatDoc::default()
    };
    let parsed = parse(&doc.block(MAGIC_V1, Compression::None)).expect("parse");
    let member = parsed.root.get("k").expect("member k");
    assert_eq!(member.flags(), 1);
    assert_eq!(member.as_str(), Some("k"));
}

#[test]
fn v1_uncompressed_size_that_disagrees_with_the_output_is_an_error() {
    let mut block = flat_sample().block(MAGIC_V1, Compression::Lz4);
    let wrong = u32le(flat_sample().payload().len() as u32 + 3);
    block[36..40].copy_from_slice(&wrong);
    assert!(parse(&block).is_err());
}

#[test]
fn v1_uncompressed_payload_of_the_wrong_length_is_an_error() {
    let mut block = flat_sample().block(MAGIC_V1, Compression::None);
    block.push(0);
    assert!(parse(&block).is_err());
}

#[test]
fn v1_missing_trailer_is_an_error() {
    let mut doc = flat_sample().block(MAGIC_V1, Compression::None);
    let n = doc.len();
    doc[n - 1] = 0;
    assert!(parse(&doc).is_err());
}

#[test]
fn v1_string_index_out_of_range_is_an_error() {
    let mut doc = flat_sample();
    doc.ints[1] = 77;
    assert!(parse(&doc.block(MAGIC_V1, Compression::None)).is_err());
}

#[test]
fn v1_pools_larger_than_the_payload_are_an_error() {
    let mut block = flat_sample().block(MAGIC_V1, Compression::None);
    block[32..36].copy_from_slice(&u32le(u32::MAX));
    assert!(parse(&block).is_err());
}

#[test]
fn v1_short_block_is_an_error() {
    let block = flat_sample().block(MAGIC_V1, Compression::None);
    assert!(Header::parse(&block[..39]).is_err());
}

#[test]
fn v1_unknown_compression_is_an_error() {
    let mut block = flat_sample().block(MAGIC_V1, Compression::None);
    block[20..24].copy_from_slice(&u32le(9));
    assert!(parse(&block).is_err());
}

#[test]
fn v1_nesting_past_the_limit_is_an_error() {
    let mut doc = FlatDoc {
        strings: vec![],
        ..FlatDoc::default()
    };
    for _ in 0..200 {
        doc.types.push(8);
        doc.ints.push(1);
    }
    doc.types.push(1);
    assert!(parse(&doc.block(MAGIC_V1, Compression::None)).is_err());
}

// ---------------------------------------------------------------------------------------
// KV3\x03 shares the v4 layout.
// ---------------------------------------------------------------------------------------

#[test]
fn v3_header_and_payload_follow_the_v4_layout() {
    let options = WriteOptions {
        version: Version::V4,
        compression: Compression::None,
        format: FORMAT,
        ..WriteOptions::default()
    };
    let mut block = write(&sample(), &options).expect("write");
    block[0..4].copy_from_slice(&MAGIC_V3.to_le_bytes());
    let h = Header::parse(&block).expect("header");
    assert_eq!(h.version, Version::V3);
    assert_eq!(parse(&block).expect("parse").root, sample());
}

#[cfg(feature = "lz4")]
#[test]
fn v3_lz4_documents_parse() {
    let options = WriteOptions {
        version: Version::V4,
        compression: Compression::Lz4,
        format: FORMAT,
        ..WriteOptions::default()
    };
    let mut block = write(&sample(), &options).expect("write");
    block[0..4].copy_from_slice(&MAGIC_V3.to_le_bytes());
    assert_eq!(parse(&block).expect("parse").root, sample());
}

#[test]
fn the_magics_are_what_the_files_carry() {
    assert_eq!(MAGIC_LEGACY.to_le_bytes(), *b"VKV");
    assert_eq!(MAGIC_V1.to_le_bytes(), *b"3VK");
    assert_eq!(MAGIC_V2.to_le_bytes(), *b"3VK");
    assert_eq!(MAGIC_V3.to_le_bytes(), *b"3VK");
}
