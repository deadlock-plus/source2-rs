//! Writing every revision: layout, compression pairing and round trips.

use crate::compression::{compress_block, decompress_block};
use crate::node;
use crate::value::{ArrayForm, Kind, Storage};
use crate::{
    Compression, Document, Error, Header, MAGIC_LEGACY, MAGIC_V1, MAGIC_V2, MAGIC_V3, MAGIC_V4,
    Object, Value, Version, WriteOptions, decode, parse, write,
};

const FORMAT: [u8; 16] = [
    0x7C, 0x16, 0x12, 0x74, 0xE9, 0x06, 0x98, 0x46, 0xAF, 0xF2, 0xE6, 0x3E, 0xB5, 0x90, 0x37, 0xE7,
];

const ALL: [Version; 6] = [
    Version::Legacy,
    Version::V1,
    Version::V2,
    Version::V3,
    Version::V4,
    Version::V5,
];

fn options(version: Version, compression: Compression) -> WriteOptions {
    WriteOptions {
        version,
        compression,
        format: FORMAT,
        ..WriteOptions::default()
    }
}

fn root(members: Vec<(&str, Value)>) -> Value {
    Value::from(Object::from(members))
}

fn le(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// Every kind of value except blobs, nested a little.
fn rich() -> Value {
    root(vec![
        ("null", Value::null()),
        ("yes", Value::from(true)),
        ("no", Value::from(false)),
        ("zero", Value::int(0)),
        ("one", Value::int(1)),
        ("small", Value::int(-7)),
        ("big", Value::int(i64::MIN)),
        ("unsigned", Value::uint(u64::MAX)),
        ("half", Value::double(0.5)),
        ("text", Value::from("hello")),
        ("empty", Value::from("")),
        ("ints", Value::array(vec![Value::int(1), Value::int(2)])),
        (
            "mixed",
            Value::array(vec![Value::int(1), Value::from("two"), Value::null()]),
        ),
        ("nothing", Value::array(vec![])),
        (
            "inner",
            root(vec![
                ("deep", root(vec![("x", Value::double(2.5))])),
                (
                    "names",
                    Value::array(vec![Value::from("a"), Value::from("b")]),
                ),
            ]),
        ),
        ("dup", Value::int(1)),
        ("dup", Value::int(2)),
    ])
}

fn pairings() -> Vec<(Version, Compression)> {
    let mut out = Vec::new();
    for version in ALL {
        out.push((version, Compression::None));
        #[cfg(feature = "lz4")]
        out.push((version, Compression::Lz4));
        if version == Version::Legacy {
            out.push((version, Compression::Block));
        } else {
            #[cfg(feature = "zstd")]
            out.push((version, Compression::Zstd));
        }
    }
    out
}

#[test]
fn every_revision_and_compression_round_trips() {
    let tree = rich();
    for (version, compression) in pairings() {
        let block = write(&tree, &options(version, compression)).expect("write");
        let doc = parse(&block).unwrap_or_else(|e| panic!("{version:?}/{compression:?}: {e}"));
        assert_eq!(doc.root, tree, "{version:?}/{compression:?}");
        assert_eq!(doc.options.version, version);
        assert_eq!(doc.options.compression, compression);
        assert_eq!(doc.options.format, FORMAT);

        let again = doc.to_bytes().expect("write again");
        assert_eq!(
            decode(&again).expect("decode").payload,
            decode(&block).expect("decode").payload,
            "{version:?}/{compression:?}"
        );
    }
}

#[test]
fn the_magic_of_each_revision_opens_the_block() {
    for (version, magic) in [
        (Version::Legacy, MAGIC_LEGACY),
        (Version::V1, MAGIC_V1),
        (Version::V2, MAGIC_V2),
        (Version::V3, MAGIC_V3),
        (Version::V4, MAGIC_V4),
    ] {
        let block = write(&rich(), &options(version, Compression::None)).unwrap();
        assert_eq!(block[..4], magic.to_le_bytes(), "{version:?}");
        assert_eq!(Header::parse(&block).unwrap().version, version);
        assert_eq!(version.magic(), magic);
        assert_eq!(Version::from_magic(magic), Some(version));
    }
}

#[test]
fn v1_layout_is_pools_strings_types_and_trailer() {
    let tree = root(vec![("a", Value::int(5))]);
    let block = write(&tree, &options(Version::V1, Compression::None)).unwrap();

    let mut payload = le(&[1, 1, 0, 5]);
    payload.extend(b"a\0");
    payload.extend([9, 11]);
    payload.extend([0x00, 0xDD, 0xEE, 0xFF]);

    let mut expected = MAGIC_V1.to_le_bytes().to_vec();
    expected.extend(FORMAT);
    expected.extend(le(&[0, 0, 4, 0, payload.len() as u32]));
    expected.extend(&payload);
    assert_eq!(block, expected);
}

#[test]
fn v2_adds_the_dictionary_and_frame_fields_to_the_v1_header() {
    let tree = root(vec![("a", Value::int(5))]);
    let o = WriteOptions {
        dictionary_id: 3,
        frame_size: 4096,
        ..options(Version::V2, Compression::None)
    };
    let block = write(&tree, &o).unwrap();
    assert_eq!(block[20..24], 0u32.to_le_bytes());
    assert_eq!(block[24..26], 3u16.to_le_bytes());
    assert_eq!(block[26..28], 4096u16.to_le_bytes());
    let h = Header::parse(&block).unwrap();
    assert_eq!((h.dictionary_id, h.frame_size), (3, 4096));
    assert_eq!(h.payload_offset, 44);
    assert_eq!(parse(&block).unwrap().root, tree);
}

#[test]
fn v3_is_laid_out_as_v4_under_its_own_magic() {
    let tree = rich();
    let v3 = write(&tree, &options(Version::V3, Compression::None)).unwrap();
    let v4 = write(&tree, &options(Version::V4, Compression::None)).unwrap();
    assert_eq!(v3[..4], MAGIC_V3.to_le_bytes());
    assert_eq!(v3[4..], v4[4..]);
}

#[test]
fn legacy_stored_stream_is_a_string_count_strings_body_and_an_end_marker() {
    let tree = root(vec![("a", Value::int(5))]);
    let block = write(&tree, &options(Version::Legacy, Compression::None)).unwrap();

    let mut expected = MAGIC_LEGACY.to_le_bytes().to_vec();
    expected.extend(crate::header::ENCODING_STORED);
    expected.extend(FORMAT);
    expected.extend(le(&[1]));
    expected.extend(b"a\0");
    expected.push(9);
    expected.extend(le(&[1, 0]));
    expected.push(11);
    expected.extend(le(&[5, 0xFFFF_FFFF]));
    assert_eq!(block, expected);
}

#[cfg(feature = "lz4")]
#[test]
fn legacy_lz4_states_the_stream_length_before_the_block() {
    let tree = rich();
    let stored = write(&tree, &options(Version::Legacy, Compression::None)).unwrap();
    let packed = write(&tree, &options(Version::Legacy, Compression::Lz4)).unwrap();
    assert_eq!(packed[4..20], crate::header::ENCODING_LZ4);
    let stream_len = (stored.len() - 36) as u32;
    assert_eq!(packed[36..40], stream_len.to_le_bytes());
}

#[test]
fn legacy_block_scheme_stores_tiny_streams_and_packs_larger_ones() {
    let tiny = write(
        &Value::from(Object::new()),
        &options(Version::Legacy, Compression::Block),
    )
    .unwrap();
    assert_eq!(tiny[39] & 0x80, 0x80, "a stored stream sets the top bit");
    assert_eq!(parse(&tiny).unwrap().root, Value::from(Object::new()));

    let words: Vec<Value> = (0..200).map(|_| Value::from("repeated text")).collect();
    let big = root(vec![("words", Value::array(words))]);
    let packed = write(&big, &options(Version::Legacy, Compression::Block)).unwrap();
    let stored = write(&big, &options(Version::Legacy, Compression::None)).unwrap();
    assert_eq!(packed[39] & 0x80, 0);
    assert!(packed.len() < stored.len());
    let d = decode(&packed).unwrap();
    assert_eq!(d.payload, decode(&stored).unwrap().payload);
    assert_eq!(parse(&packed).unwrap().root, big);
}

#[test]
fn block_compression_inverts_for_assorted_data() {
    let mut noise = Vec::new();
    let mut seed = 0x1234_5678_u64;
    for _ in 0..5000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        noise.push((seed >> 33) as u8);
    }
    let cases: Vec<Vec<u8>> = vec![
        vec![],
        vec![7],
        vec![1, 2],
        vec![0; 100],
        b"abcabcabcabcabcabcabcabcabcabc".to_vec(),
        noise.clone(),
        [noise.clone(), noise.clone()].concat(),
        (0..20_000u32)
            .flat_map(|i| (i % 300).to_le_bytes())
            .collect(),
        b"x".repeat(4096 + 40),
    ];
    for case in cases {
        let packed = compress_block(&case);
        assert_eq!(
            decompress_block(&packed, case.len()).expect("decompress"),
            case,
            "{} bytes",
            case.len()
        );
    }
    let runs = vec![0u8; 10_000];
    assert!(compress_block(&runs).len() < 2000);
}

#[test]
fn block_copies_reach_exactly_the_whole_window() {
    let mut data = b"unique-marker".to_vec();
    data.extend(std::iter::repeat_n(0xAB_u8, 4096 - data.len()));
    data.extend(b"unique-marker");
    let packed = compress_block(&data);
    assert_eq!(decompress_block(&packed, data.len()).unwrap(), data);
    assert!(packed.len() < data.len());
}

#[test]
fn pairings_no_file_uses_are_refused() {
    let tree = rich();
    for (version, compression) in [
        (Version::Legacy, Compression::Zstd),
        (Version::V5, Compression::Block),
        (Version::V1, Compression::Block),
        (Version::V4, Compression::Unknown(9)),
        (Version::Legacy, Compression::Unknown(9)),
    ] {
        let err = write(&tree, &options(version, compression)).unwrap_err();
        assert!(
            matches!(err, Error::Unsupported(_)),
            "{version:?}/{compression:?}: {err}"
        );
    }
}

#[test]
fn every_revision_can_hold_a_blob() {
    let tree = root(vec![
        ("b", Value::blob(vec![1, 2, 3])),
        ("e", Value::blob(vec![])),
    ]);
    for version in [
        Version::Legacy,
        Version::V1,
        Version::V2,
        Version::V3,
        Version::V4,
        Version::V5,
    ] {
        let block = write(&tree, &options(version, Compression::None)).unwrap();
        assert_eq!(parse(&block).unwrap().root, tree, "{version:?}");
    }
}

#[test]
fn two_byte_scalars_are_widened_where_the_revision_has_no_pool() {
    let tree = root(vec![
        (
            "s",
            Value::stored(Kind::Int(-2), 0, Storage::Scalar(node::INT16)),
        ),
        (
            "u",
            Value::stored(Kind::UInt(0xFFFE), 0, Storage::Scalar(node::UINT16)),
        ),
    ]);
    for version in ALL {
        let block = write(&tree, &options(version, Compression::None)).unwrap();
        let back = parse(&block).unwrap().root;
        assert_eq!(back, tree, "{version:?}");
        let code = |key: &str| match back.get(key).unwrap().storage() {
            Storage::Scalar(ty) => ty,
            other => panic!("{other:?}"),
        };
        let kept = matches!(
            version,
            Version::Legacy | Version::V3 | Version::V4 | Version::V5
        );
        assert_eq!(code("s") == node::INT16, kept, "{version:?}");
        assert_eq!(code("u") == node::UINT16, kept, "{version:?}");
    }
}

#[test]
fn numeric_arrays_use_the_layout_each_revision_has() {
    let tree = root(vec![(
        "xs",
        Value::array(vec![Value::double(0.5), Value::double(1.5)]),
    )]);
    for (version, wanted) in [
        (Version::Legacy, "typed"),
        (Version::V1, "typed"),
        (Version::V2, "typed"),
        (Version::V3, "byte-length"),
        (Version::V4, "byte-length"),
        (Version::V5, "auxiliary"),
    ] {
        let block = write(&tree, &options(version, Compression::None)).unwrap();
        let back = parse(&block).unwrap().root;
        let Storage::Array(form) = back.get("xs").unwrap().storage() else {
            panic!("not an array");
        };
        let got = match form {
            ArrayForm::General => "general",
            ArrayForm::Typed { .. } => "typed",
            ArrayForm::ByteLength { .. } => "byte-length",
            ArrayForm::Auxiliary { .. } => "auxiliary",
        };
        assert_eq!(got, wanted, "{version:?}");
        assert_eq!(back, tree);
    }
}

#[test]
fn dictionary_id_and_frame_size_are_written_and_read_back() {
    let tree = rich();
    for version in [Version::V3, Version::V4, Version::V5] {
        let o = WriteOptions {
            dictionary_id: 9,
            frame_size: 1234,
            ..options(version, Compression::None)
        };
        let block = write(&tree, &o).unwrap();
        let h = Header::parse(&block).unwrap();
        assert_eq!((h.dictionary_id, h.frame_size), (9, 1234), "{version:?}");
        let doc = parse(&block).unwrap();
        assert_eq!(doc.options.dictionary_id, 9);
        assert_eq!(doc.options.frame_size, 1234);
    }
}

#[cfg(feature = "lz4")]
#[test]
fn lz4_declares_the_frame_size_every_lz4_file_does() {
    let block = write(&rich(), &options(Version::V5, Compression::Lz4)).unwrap();
    assert_eq!(Header::parse(&block).unwrap().frame_size, 16384);
    let o = WriteOptions {
        frame_size: 8192,
        ..options(Version::V5, Compression::Lz4)
    };
    let block = write(&rich(), &o).unwrap();
    assert_eq!(Header::parse(&block).unwrap().frame_size, 8192);
}

#[test]
fn write_options_default_to_valves_usual_encoding() {
    let o = WriteOptions::default();
    assert_eq!(o.version, Version::V5);
    assert_eq!(o.format, crate::GENERIC_FORMAT);
    assert_eq!((o.dictionary_id, o.frame_size), (0, 0));
    let expected = if cfg!(feature = "lz4") {
        Compression::Lz4
    } else if cfg!(feature = "zstd") {
        Compression::Zstd
    } else {
        Compression::None
    };
    assert_eq!(o.compression, expected);
}

#[test]
fn options_from_a_header_repeat_it_exactly() {
    for (version, compression) in pairings() {
        let block = write(&rich(), &options(version, compression)).unwrap();
        let header = Header::parse(&block).unwrap();
        let o = WriteOptions::try_from(&header).unwrap();
        assert_eq!(o.version, version);
        assert_eq!(o.compression, compression);
        assert_eq!(o.format, FORMAT);
    }
}

#[test]
fn options_from_a_header_with_an_unknown_compression_are_an_error() {
    let mut block = write(&rich(), &options(Version::V5, Compression::None)).unwrap();
    block[20..24].copy_from_slice(&9u32.to_le_bytes());
    let header = Header::parse(&block).unwrap();
    assert_eq!(header.compression, Compression::Unknown(9));
    assert!(matches!(
        WriteOptions::try_from(&header),
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(decode(&block), Err(Error::Unsupported(_))));
}

#[test]
fn new_documents_write_with_valves_defaults_and_read_back() {
    let doc = Document::new(rich());
    let back = Document::from_bytes(&doc.to_bytes().unwrap()).unwrap();
    assert_eq!(back.root, doc.root);
    assert_eq!(back.options.version, Version::V5);
    assert_eq!(back.options.format, crate::GENERIC_FORMAT);
}

#[test]
fn nul_in_a_string_is_an_invalid_value_in_every_revision() {
    let tree = root(vec![("k", Value::from("a\0b"))]);
    for version in ALL {
        let err = write(&tree, &options(version, Compression::None)).unwrap_err();
        assert!(matches!(err, Error::Invalid(_)), "{version:?}: {err}");
    }
}

#[test]
fn nesting_past_the_limit_is_refused_by_every_writer() {
    let mut v = Value::int(1);
    for _ in 0..200 {
        v = Value::array(vec![v]);
    }
    for version in ALL {
        let err = write(&v, &options(version, Compression::None)).unwrap_err();
        assert!(matches!(err, Error::Invalid(_)), "{version:?}: {err}");
    }
}

#[test]
fn flags_survive_every_revision() {
    let tree = root(vec![
        ("p", Value::from("a.bin").with_flags(crate::flag::RESOURCE)),
        (
            "list",
            Value::array(vec![
                Value::from("x").with_flags(crate::flag::SUBCLASS),
                Value::from("y").with_flags(crate::flag::SUBCLASS),
            ]),
        ),
        (
            "obj",
            root(vec![("n", Value::int(1))]).with_flags(crate::flag::PANORAMA),
        ),
        ("odd", Value::int(3).with_flags(0x40)),
    ]);
    for version in ALL {
        let block = write(&tree, &options(version, Compression::None)).unwrap();
        assert_eq!(parse(&block).unwrap().root, tree, "{version:?}");
    }
}
