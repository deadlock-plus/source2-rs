//! Tests for the binary writer.

use crate::error::Error;
use crate::value::MAX_DEPTH;
use crate::writer::{Version, WriteOptions, write};
use crate::{Compression, Object, Value, parse};

fn options(version: Version, compression: Compression) -> WriteOptions {
    WriteOptions {
        version,
        compression,
        format: [0; 16],
    }
}

fn object(members: Vec<(&str, Value)>) -> Value {
    let mut o = Object::default();
    for (k, v) in members {
        o.insert(k.to_string(), v);
    }
    Value::Object(o)
}

fn string(s: &str) -> Value {
    Value::String(s.to_string())
}

/// `{ "a": 5, "b": "xy" }`, the document both hand-built blocks below encode.
fn tiny() -> Value {
    object(vec![("a", Value::Int(5)), ("b", string("xy"))])
}

fn words(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn header(len: usize, fields: &[(usize, u32)], magic: u32) -> Vec<u8> {
    let mut h = vec![0u8; len];
    h[0..4].copy_from_slice(&magic.to_le_bytes());
    for &(at, v) in fields {
        h[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    h
}

#[test]
fn v5_uncompressed_bytes_match_the_layout() {
    // Buffer 1: the string blob, padded to 4, then the 4-byte pool holding the string count.
    let mut buf1 = b"a\0b\0xy\0".to_vec();
    buf1.push(0);
    buf1.extend(words(&[3]));

    // Buffer 2: object-length table, 1-byte pool, padding to 4, 4-byte pool, types, trailer.
    // The member operands are name, name, then the string value's index, in stream order.
    let mut buf2 = words(&[2]);
    buf2.push(5);
    buf2.extend([0; 3]);
    buf2.extend(words(&[0, 1, 2]));
    buf2.extend([9, 23, 6]);
    buf2.extend(words(&[0xFFEE_DD00]));

    let total = (buf1.len() + buf2.len()) as u32;
    let mut expected = header(
        120,
        &[
            (28, 7),
            (32, 1),
            (40, 3),
            (44, 1),
            (48, total),
            (52, total),
            (72, buf1.len() as u32),
            (80, buf2.len() as u32),
            (88, 1),
            (96, 3),
            (104, 3),
            (108, 1),
        ],
        crate::MAGIC_V5,
    );
    expected.extend(&buf1);
    expected.extend(&buf2);

    let got = write(&tiny(), &options(Version::V5, Compression::None)).expect("write");
    assert_eq!(got, expected);
}

#[test]
fn v4_uncompressed_bytes_match_the_layout() {
    // The 1-byte pool, padded to 4, then the 4-byte pool: string count, the root's member
    // count (v4 has no length table), then name, name, string index.
    let mut payload = vec![5, 0, 0, 0];
    payload.extend(words(&[3, 2, 0, 1, 2]));
    // One region holds the strings and then the type stream.
    payload.extend(b"a\0b\0xy\0");
    payload.extend([9, 23, 6]);
    payload.extend(words(&[0xFFEE_DD00]));

    let mut expected = header(
        72,
        &[
            (28, 1),
            (32, 5),
            (40, 10),
            (44, 1),
            (48, payload.len() as u32),
            (52, payload.len() as u32),
        ],
        crate::MAGIC_V4,
    );
    expected.extend(&payload);

    let got = write(&tiny(), &options(Version::V4, Compression::None)).expect("write");
    assert_eq!(got, expected);
}

#[test]
fn the_format_guid_lands_in_the_header() {
    let format = *b"0123456789abcdef";
    for version in [Version::V4, Version::V5] {
        let opts = WriteOptions {
            format,
            ..options(version, Compression::None)
        };
        let bytes = write(&tiny(), &opts).expect("write");
        assert_eq!(&bytes[4..20], &format);
        assert_eq!(parse(&bytes).expect("parse").header.format, format);
    }
}

fn every_variant() -> Value {
    object(vec![
        ("null", Value::Null),
        ("yes", Value::Bool(true)),
        ("no", Value::Bool(false)),
        ("zero", Value::Int(0)),
        ("one", Value::Int(1)),
        ("byte", Value::Int(-128)),
        ("byte_max", Value::Int(127)),
        ("short", Value::Int(-129)),
        ("short_max", Value::Int(32767)),
        ("int", Value::Int(-40_000)),
        ("int_min", Value::Int(i64::from(i32::MIN))),
        ("int_max", Value::Int(i64::from(i32::MAX))),
        ("wide", Value::Int(i64::from(i32::MAX) + 1)),
        ("wide_min", Value::Int(i64::MIN)),
        ("wide_max", Value::Int(i64::MAX)),
        ("u0", Value::UInt(0)),
        ("u16", Value::UInt(65_535)),
        ("u32", Value::UInt(65_536)),
        ("u32_max", Value::UInt(u64::from(u32::MAX))),
        ("u64", Value::UInt(u64::from(u32::MAX) + 1)),
        ("u64_max", Value::UInt(u64::MAX)),
        ("d0", Value::Double(0.0)),
        ("d1", Value::Double(1.0)),
        ("d", Value::Double(2.5)),
        ("d_neg", Value::Double(-0.1)),
        ("d_inf", Value::Double(f64::INFINITY)),
        ("d_min", Value::Double(f64::MIN_POSITIVE)),
        ("empty", string("")),
        ("text", string("hello")),
        ("unicode", string("h\u{e9}llo \u{1f980}")),
        (
            "list",
            Value::Array(vec![Value::Int(1), string("two"), Value::Null]),
        ),
        ("empty_list", Value::Array(vec![])),
        ("empty_object", Value::Object(Object::default())),
        (
            "nested",
            object(vec![
                (
                    "deeper",
                    object(vec![("list", Value::Array(vec![Value::Array(vec![])]))]),
                ),
                ("text", string("hello")),
            ]),
        ),
    ])
}

fn round_trip(value: &Value, version: Version, compression: Compression) {
    let bytes = write(value, &options(version, compression)).expect("write");
    let doc = parse(&bytes).expect("parse");
    assert_eq!(&doc.root, value);
    assert_eq!(
        doc.header.version,
        if version == Version::V5 { 5 } else { 4 }
    );
    assert_eq!(doc.header.compression, compression);
}

#[test]
fn every_value_variant_round_trips_uncompressed() {
    for version in [Version::V4, Version::V5] {
        round_trip(&every_variant(), version, Compression::None);
    }
}

#[test]
fn a_negative_zero_keeps_its_sign() {
    let value = object(vec![("z", Value::Double(-0.0))]);
    let bytes = write(&value, &options(Version::V5, Compression::None)).expect("write");
    let root = parse(&bytes).expect("parse").root;
    let got = root.get("z").and_then(Value::as_f64).expect("double");
    assert!(got.is_sign_negative());
}

#[test]
fn insertion_order_and_repeated_keys_survive() {
    let value = object(vec![
        ("z", Value::Int(1)),
        ("a", Value::Int(2)),
        ("z", Value::Int(3)),
    ]);
    for version in [Version::V4, Version::V5] {
        round_trip(&value, version, Compression::None);
    }
}

#[test]
fn strings_shared_between_names_and_values_are_pooled_once() {
    let value = object(vec![("same", string("same")), ("other", string("same"))]);
    let bytes = write(&value, &options(Version::V5, Compression::None)).expect("write");
    let h = parse(&bytes).expect("parse").header;
    assert_eq!(h.binary_byte_count as usize, "same\0other\0".len());
    round_trip(&value, Version::V5, Compression::None);
}

#[test]
fn a_document_with_no_object_round_trips() {
    for version in [Version::V4, Version::V5] {
        round_trip(&Value::Int(7), version, Compression::None);
        round_trip(&Value::Array(vec![string("x")]), version, Compression::None);
    }
}

#[test]
fn a_large_document_round_trips() {
    let members: Vec<(String, Value)> = (0..2000)
        .map(|i| {
            (
                format!("key_{i}"),
                Value::Array(vec![
                    Value::Int(i),
                    string(&format!("value {i}")),
                    Value::Double(i as f64 + 0.5),
                ]),
            )
        })
        .collect();
    let mut o = Object::default();
    for (k, v) in members {
        o.insert(k, v);
    }
    let value = Value::Object(o);
    for version in [Version::V4, Version::V5] {
        round_trip(&value, version, Compression::None);
    }
}

#[cfg(feature = "lz4")]
#[test]
fn lz4_round_trips() {
    for version in [Version::V4, Version::V5] {
        round_trip(&every_variant(), version, Compression::Lz4);
        round_trip(&tiny(), version, Compression::Lz4);
    }
}

#[cfg(feature = "lz4")]
#[test]
fn lz4_records_the_compressed_sizes_of_each_buffer() {
    let bytes = write(&every_variant(), &options(Version::V5, Compression::Lz4)).expect("write");
    let h = parse(&bytes).expect("parse").header;
    assert_eq!(h.payload_offset, 120);
    assert_eq!(
        h.compressed_size,
        h.buffer1_compressed_size + h.buffer2_compressed_size
    );
    assert_eq!(
        h.uncompressed_size,
        h.buffer1_uncompressed_size + h.buffer2_uncompressed_size
    );
    assert_eq!(bytes.len(), 120 + h.compressed_size as usize);
}

#[cfg(feature = "zstd")]
#[test]
fn zstd_round_trips() {
    for version in [Version::V4, Version::V5] {
        round_trip(&every_variant(), version, Compression::Zstd);
        round_trip(&tiny(), version, Compression::Zstd);
    }
}

#[cfg(feature = "zstd")]
#[test]
fn zstd_stores_a_frame_per_buffer_on_v5() {
    let bytes = write(&every_variant(), &options(Version::V5, Compression::Zstd)).expect("write");
    let h = parse(&bytes).expect("parse").header;
    let payload = &bytes[h.payload_offset..];
    let split = h.buffer1_compressed_size as usize;
    assert_eq!(&payload[..4], &[0x28, 0xB5, 0x2F, 0xFD]);
    assert_eq!(&payload[split..split + 4], &[0x28, 0xB5, 0x2F, 0xFD]);
    assert_eq!(h.compressed_size as usize, payload.len());
}

#[cfg(not(feature = "lz4"))]
#[test]
fn lz4_without_the_feature_is_refused() {
    let err = write(&tiny(), &options(Version::V5, Compression::Lz4)).unwrap_err();
    assert!(format!("{err}").contains("lz4"), "{err}");
}

#[cfg(not(feature = "zstd"))]
#[test]
fn zstd_without_the_feature_is_refused() {
    let err = write(&tiny(), &options(Version::V5, Compression::Zstd)).unwrap_err();
    assert!(format!("{err}").contains("zstd"), "{err}");
}

#[test]
fn an_unknown_compression_method_is_refused() {
    let err = write(&tiny(), &options(Version::V5, Compression::Unknown(9))).unwrap_err();
    assert!(format!("{err}").contains('9'), "{err}");
}

#[test]
fn a_v4_blob_is_refused_because_v4_has_no_blob_area() {
    let value = object(vec![("b", Value::Blob(vec![1, 2, 3]))]);
    let err = write(&value, &options(Version::V4, Compression::None)).unwrap_err();
    assert!(matches!(err, Error::Malformed(_)));
    assert!(format!("{err}").contains("blob"), "{err}");
}

fn blobs() -> Value {
    object(vec![
        ("a", Value::Blob(vec![1, 2, 3])),
        ("empty", Value::Blob(Vec::new())),
        (
            "nested",
            Value::Array(vec![Value::Blob((0..=255).cycle().take(40_000).collect())]),
        ),
    ])
}

#[test]
fn v5_blobs_round_trip_uncompressed() {
    round_trip(&blobs(), Version::V5, Compression::None);
}

#[cfg(feature = "lz4")]
#[test]
fn v5_blobs_round_trip_through_lz4() {
    round_trip(&blobs(), Version::V5, Compression::Lz4);
}

#[cfg(feature = "zstd")]
#[test]
fn v5_blobs_round_trip_through_zstd() {
    round_trip(&blobs(), Version::V5, Compression::Zstd);
}

#[cfg(feature = "lz4")]
#[test]
fn lz4_blobs_state_their_count_and_total_in_the_header() {
    let bytes = write(&blobs(), &options(Version::V5, Compression::Lz4)).expect("write");
    let header = parse(&bytes).expect("parse").header;
    assert_eq!(header.blob_count, 3);
    assert_eq!(header.blob_total_size, 40_003);
}

#[test]
fn a_nul_in_a_string_is_refused() {
    for bad in [
        object(vec![("a", string("x\0y"))]),
        object(vec![("x\0y", Value::Null)]),
    ] {
        let err = write(&bad, &options(Version::V5, Compression::None)).unwrap_err();
        assert!(format!("{err}").contains("NUL"), "{err}");
    }
}

fn nested(depth: u32) -> Value {
    let mut v = Value::Null;
    for _ in 0..depth {
        v = Value::Array(vec![v]);
    }
    v
}

#[test]
fn nesting_up_to_the_reader_limit_round_trips_and_past_it_is_refused() {
    let ok = nested(MAX_DEPTH);
    round_trip(&ok, Version::V5, Compression::None);

    let too_deep = nested(MAX_DEPTH + 1);
    let err = write(&too_deep, &options(Version::V5, Compression::None)).unwrap_err();
    assert!(format!("{err}").contains("nesting"), "{err}");
}

#[test]
fn the_shape_counts_match_what_shipped_files_state() {
    let value = object(vec![
        ("o", object(vec![("n", Value::Null)])),
        ("a", Value::Array(vec![Value::Int(1), Value::Array(vec![])])),
    ]);
    for version in [Version::V4, Version::V5] {
        let bytes = write(&value, &options(version, Compression::None)).expect("write");
        let u16_at = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]);
        assert_eq!(u16_at(44), 2, "objects");
        assert_eq!(u16_at(46), 2, "arrays");
        if version == Version::V5 {
            let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
            assert_eq!(u32_at(104), 6, "values");
            assert_eq!(u32_at(112), 2, "arrays");
            assert_eq!(u32_at(116), 2, "array elements");
        }
    }
}

/// Bytes with no short-range repetition, so only a long-range match can shrink them.
fn noise(len: usize) -> Vec<u8> {
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 32) as u8
        })
        .collect()
}

#[cfg(feature = "lz4")]
#[test]
fn lz4_shrinks_a_redundant_document() {
    let value = object(vec![(
        "zeros",
        Value::Array((0..5000).map(|_| Value::Int(7)).collect()),
    )]);
    for version in [Version::V4, Version::V5] {
        let plain = write(&value, &options(version, Compression::None)).expect("write");
        let packed = write(&value, &options(version, Compression::Lz4)).expect("write");
        assert!(
            packed.len() < plain.len() / 2,
            "{version:?}: {} B packed vs {} B plain",
            packed.len(),
            plain.len()
        );
        round_trip(&value, version, Compression::Lz4);
    }
}

#[cfg(feature = "lz4")]
#[test]
fn lz4_shrinks_repetitive_blobs() {
    let plain = write(&blobs(), &options(Version::V5, Compression::None)).expect("write");
    let packed = write(&blobs(), &options(Version::V5, Compression::Lz4)).expect("write");
    assert!(
        packed.len() < plain.len() / 2,
        "{} B packed vs {} B plain",
        packed.len(),
        plain.len()
    );
}

#[cfg(feature = "lz4")]
#[test]
fn lz4_blob_chunks_match_against_earlier_output() {
    let pattern = noise(20_000);
    let twice: Vec<u8> = pattern.iter().chain(&pattern).copied().collect();
    let value = object(vec![
        ("first", Value::Blob(pattern.clone())),
        ("second", Value::Blob(pattern)),
        ("both", Value::Blob(twice)),
    ]);
    let plain = write(&value, &options(Version::V5, Compression::None)).expect("write");
    let packed = write(&value, &options(Version::V5, Compression::Lz4)).expect("write");
    assert!(
        packed.len() < plain.len() * 2 / 5,
        "{} B packed vs {} B plain",
        packed.len(),
        plain.len()
    );
    round_trip(&value, Version::V5, Compression::Lz4);
}
