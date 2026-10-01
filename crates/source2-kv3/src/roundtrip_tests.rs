//! What the writer stores, and that a document written back is the document that was read.

use crate::node;
use crate::value::{ArrayForm, Kind, Storage, flag};
use crate::{
    Compression, Document, Object, Value, Version, WriteOptions, decode, parse, parse_text, write,
};

fn options(version: Version) -> WriteOptions {
    WriteOptions {
        version,
        compression: Compression::None,
        format: [0; 16],
        ..WriteOptions::default()
    }
}

fn object(members: Vec<(&str, Value)>) -> Value {
    let mut o = Object::default();
    for (k, v) in members {
        o.push(k.to_string(), v);
    }
    Value::from(o)
}

fn out_text(v: &Value) -> String {
    Document::new(v.clone()).to_text().expect("write text")
}

fn s(text: &str) -> Value {
    Value::from(text)
}

/// Write `{ "a": value }` and read back the value stored under `a`.
fn stored(value: Value, version: Version) -> Value {
    let block = write(&object(vec![("a", value)]), &options(version)).expect("write");
    let doc = parse(&block).expect("read back");
    doc.root.get("a").expect("member a").clone()
}

fn form(v: &Value) -> ArrayForm {
    match v.storage() {
        Storage::Array(form) => form,
        other => panic!("not an array: {other:?}"),
    }
}

fn items(v: &Value) -> &[Value] {
    v.as_array().expect("an array")
}

fn type_code(v: &Value) -> u8 {
    match v.storage() {
        Storage::Scalar(ty) => ty,
        other => panic!("not a scalar: {other:?}"),
    }
}

fn header_word(block: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(block[at..at + 4].try_into().unwrap())
}

#[test]
fn small_integers_are_stored_as_int32_not_as_a_byte() {
    let n = stored(Value::int(5), Version::V5);
    assert_eq!(type_code(&n), node::INT32);
    assert_eq!(n.as_i64(), Some(5));
    let n = stored(Value::int(-1), Version::V5);
    assert_eq!(type_code(&n), node::INT32);
    assert_eq!(n.as_i64(), Some(-1));
}

#[test]
fn zero_and_one_keep_their_constant_codes() {
    assert_eq!(
        type_code(&stored(Value::int(0), Version::V5)),
        node::INT64_ZERO
    );
    assert_eq!(
        type_code(&stored(Value::int(1), Version::V5)),
        node::INT64_ONE
    );
    assert_eq!(
        type_code(&stored(Value::double(0.0), Version::V5)),
        node::DOUBLE_ZERO
    );
    assert_eq!(
        type_code(&stored(Value::double(1.0), Version::V5)),
        node::DOUBLE_ONE
    );
}

#[test]
fn v5_numeric_arrays_live_in_the_auxiliary_buffer() {
    let doubles = stored(
        Value::array(vec![Value::double(1.5), Value::double(2.5)]),
        Version::V5,
    );
    assert_eq!(
        form(&doubles),
        ArrayForm::Auxiliary {
            ty: node::DOUBLE,
            flags: 0
        }
    );
    assert_eq!(items(&doubles).len(), 2);

    let ints = stored(
        Value::array(vec![Value::int(3), Value::int(4)]),
        Version::V5,
    );
    assert_eq!(
        form(&ints),
        ArrayForm::Auxiliary {
            ty: node::INT32,
            flags: 0
        }
    );

    let uints = stored(
        Value::array(vec![Value::uint(3), Value::uint(4)]),
        Version::V5,
    );
    assert_eq!(
        form(&uints),
        ArrayForm::Auxiliary {
            ty: node::UINT32,
            flags: 0
        }
    );
}

#[test]
fn a_one_element_numeric_array_is_still_auxiliary() {
    let n = stored(Value::array(vec![Value::double(2.5)]), Version::V5);
    assert!(matches!(form(&n), ArrayForm::Auxiliary { .. }));
}

#[test]
fn typed_arrays_store_zero_and_one_under_the_element_type() {
    let n = stored(
        Value::array(vec![Value::double(0.0), Value::double(0.5)]),
        Version::V5,
    );
    assert!(matches!(
        form(&n),
        ArrayForm::Auxiliary {
            ty: node::DOUBLE,
            ..
        }
    ));
    assert_eq!(type_code(&items(&n)[0]), node::DOUBLE);

    let n = stored(
        Value::array(vec![Value::int(0), Value::int(1), Value::int(7)]),
        Version::V5,
    );
    assert_eq!(type_code(&items(&n)[0]), node::INT32);
    assert_eq!(type_code(&items(&n)[1]), node::INT32);
}

#[test]
fn v4_has_no_auxiliary_buffer_so_numeric_arrays_use_a_byte_length() {
    let n = stored(
        Value::array(vec![Value::int(3), Value::int(4)]),
        Version::V4,
    );
    assert_eq!(
        form(&n),
        ArrayForm::ByteLength {
            ty: node::INT32,
            flags: 0
        }
    );
}

#[test]
fn arrays_past_255_elements_take_a_four_byte_length() {
    let many: Vec<Value> = (0..300).map(|i| Value::int(i + 2)).collect();
    let n = stored(Value::array(many), Version::V5);
    assert_eq!(
        form(&n),
        ArrayForm::Typed {
            ty: node::INT32,
            flags: 0
        }
    );
    let strings: Vec<Value> = (0..300).map(|i| s(&i.to_string())).collect();
    let n = stored(Value::array(strings), Version::V5);
    assert!(matches!(
        form(&n),
        ArrayForm::Typed {
            ty: node::STRING,
            ..
        }
    ));
}

#[test]
fn arrays_of_mixed_kinds_keep_a_type_code_per_element() {
    let n = stored(
        Value::array(vec![Value::double(1.0), Value::int(2), Value::null()]),
        Version::V5,
    );
    assert_eq!(form(&n), ArrayForm::General);
    assert_eq!(type_code(&items(&n)[0]), node::DOUBLE_ONE);
    assert_eq!(type_code(&items(&n)[1]), node::INT32);
    assert_eq!(type_code(&items(&n)[2]), node::NULL);
}

#[test]
fn empty_arrays_and_bool_arrays_are_general() {
    assert_eq!(
        form(&stored(Value::array(vec![]), Version::V5)),
        ArrayForm::General
    );
    let n = stored(
        Value::array(vec![Value::from(true), Value::from(true)]),
        Version::V5,
    );
    assert_eq!(form(&n), ArrayForm::General);
}

#[test]
fn strings_objects_and_arrays_of_arrays_are_byte_length_typed() {
    let n = stored(Value::array(vec![s("x"), s("y")]), Version::V5);
    assert_eq!(
        form(&n),
        ArrayForm::ByteLength {
            ty: node::STRING,
            flags: 0
        }
    );

    let o = || object(vec![("k", s("v"))]);
    let n = stored(Value::array(vec![o(), o()]), Version::V5);
    assert_eq!(
        form(&n),
        ArrayForm::ByteLength {
            ty: node::OBJECT,
            flags: 0
        }
    );

    let inner = || Value::array(vec![Value::double(1.5), Value::double(2.5)]);
    let n = stored(Value::array(vec![inner(), inner()]), Version::V5);
    assert_eq!(
        form(&n),
        ArrayForm::ByteLength {
            ty: node::ARRAY_TYPE_AUXILIARY_BUFFER,
            flags: 0
        }
    );
}

#[test]
fn the_value_tree_survives_the_new_forms() {
    let v = object(vec![
        (
            "a",
            Value::array(vec![Value::double(0.0), Value::double(2.5)]),
        ),
        (
            "b",
            Value::array(vec![Value::int(-4), Value::int(0), Value::int(9)]),
        ),
        ("c", Value::array(vec![s("x"), s("y")])),
        (
            "d",
            Value::array(vec![Value::null(), Value::double(1.0), Value::int(3)]),
        ),
        (
            "e",
            Value::array(vec![Value::array(vec![Value::uint(5)]); 3]),
        ),
    ]);
    for version in [Version::V4, Version::V5] {
        let block = write(&v, &options(version)).expect("write");
        assert_eq!(parse(&block).expect("parse").root, v);
    }
}

/// `{ a: [1.5, 2.5], b: ["x"], c: [] }`, then `d`, an array long enough to be counted.
#[test]
fn the_v5_header_counts_values_arrays_and_elements() {
    let doc = |with_long: bool| {
        let mut members = vec![
            (
                "a",
                Value::array(vec![Value::double(1.5), Value::double(2.5)]),
            ),
            ("b", Value::array(vec![s("x")])),
            ("c", Value::array(vec![])),
        ];
        if with_long {
            members.push((
                "d",
                Value::array(
                    (0..40)
                        .map(|i| Value::double(f64::from(i) + 0.5))
                        .collect::<Vec<_>>(),
                ),
            ));
        }
        write(&object(members), &options(Version::V5)).expect("write")
    };

    let short = doc(false);
    // Root, a, b and c carry a type code of their own; typed elements and sub-codes do not.
    assert_eq!(header_word(&short, 104), 4);
    // Only arrays outside the auxiliary buffer are counted, and an empty one counts for one.
    assert_eq!(header_word(&short, 112), 2);
    assert_eq!(header_word(&short, 116), 2);

    let long = doc(true);
    assert_eq!(header_word(&long, 104), 5);
    // An auxiliary array of 32 or more elements is counted like any other.
    assert_eq!(header_word(&long, 112), 3);
    assert_eq!(header_word(&long, 116), 42);
}

#[test]
fn rewriting_what_was_read_keeps_the_payload() {
    let v = object(vec![
        (
            "a",
            Value::array(vec![Value::double(0.0), Value::double(2.5)]),
        ),
        ("c", Value::array(vec![s("x"), s("y")])),
    ]);
    let block = write(&v, &options(Version::V5)).expect("write");
    let first = decode(&block).expect("decode");
    let again = write(&parse(&block).expect("parse").root, &options(Version::V5)).expect("write");
    assert_eq!(decode(&again).expect("decode").payload, first.payload);
}

#[cfg(feature = "lz4")]
#[test]
fn lz4_blobs_state_the_size_of_the_chunk_table_in_the_header() {
    let blob = |len: usize| Value::blob((0..len).map(|i| (i % 251) as u8).collect());
    let v = object(vec![("a", blob(20_000)), ("b", blob(100))]);
    let options = WriteOptions {
        version: Version::V5,
        compression: Compression::Lz4,
        format: [0; 16],
        ..WriteOptions::default()
    };
    let block = write(&v, &options).expect("write");
    // 20,000 bytes take two 16,384-byte chunks and 100 bytes take one, 2 bytes apiece.
    assert_eq!(header_word(&block, 68), 6);

    let plain = write(&v, &self::options(Version::V5)).expect("write");
    assert_eq!(header_word(&plain, 68), 0);
}

/// A scalar stored under the type code a file chose, as a reader would produce it.
fn scalar_as(kind: Kind, ty: u8) -> Value {
    Value::stored(kind, 0, Storage::Scalar(ty))
}

fn string_flagged(text: &str, flags: u8) -> Value {
    Value::from(text).with_flags(flags)
}

fn array_as(form: ArrayForm, items: Vec<Value>) -> Value {
    Value::stored(Kind::Array(items), 0, Storage::Array(form))
}

/// A tree using every storage detail a plain value would not choose for itself.
fn irregular(version: Version) -> Value {
    let double = |v: f64| scalar_as(Kind::Double(v), node::DOUBLE);
    let mut members = vec![
        ("path", string_flagged("models/a.vmdl", flag::RESOURCE)),
        (
            "flagged_object",
            object(vec![("x", scalar_as(Kind::Int(7), node::INT32))])
                .with_flags(flag::RESOURCE | flag::SUBCLASS),
        ),
        ("byte", scalar_as(Kind::Int(-2), node::INT32_AS_BYTE)),
        ("flag_byte", scalar_as(Kind::Bool(true), node::BOOLEAN)),
        ("short", scalar_as(Kind::Int(-2), node::INT16)),
        ("ushort", scalar_as(Kind::UInt(0xFFFE), node::UINT16)),
        ("float", scalar_as(Kind::Double(1.5), node::FLOAT)),
        ("wide_zero", scalar_as(Kind::Int(0), node::INT64)),
        ("small_unsigned", scalar_as(Kind::UInt(3), node::UINT64)),
        (
            "same_but_general",
            array_as(ArrayForm::General, vec![double(2.5), double(2.5)]),
        ),
        (
            "paths",
            array_as(
                ArrayForm::ByteLength {
                    ty: node::STRING,
                    flags: flag::RESOURCE,
                },
                vec![
                    string_flagged("a.vmdl", flag::RESOURCE),
                    string_flagged("b.vmdl", flag::RESOURCE),
                ],
            ),
        ),
        (
            "wide",
            array_as(
                ArrayForm::Typed {
                    ty: node::INT32,
                    flags: 0,
                },
                vec![scalar_as(Kind::Int(3), node::INT32)],
            ),
        ),
        (
            "empty_typed",
            array_as(
                ArrayForm::Typed {
                    ty: node::STRING,
                    flags: flag::SOUND_EVENT,
                },
                vec![],
            ),
        ),
    ];
    if version == Version::V5 {
        members.push((
            "aux",
            array_as(
                ArrayForm::Auxiliary {
                    ty: node::DOUBLE,
                    flags: 0,
                },
                vec![double(0.0), double(2.5)],
            ),
        ));
        members.push((
            "aux_of_aux",
            array_as(
                ArrayForm::ByteLength {
                    ty: node::ARRAY_TYPE_AUXILIARY_BUFFER,
                    flags: 0,
                },
                vec![array_as(
                    ArrayForm::Auxiliary {
                        ty: node::UINT32,
                        flags: 0,
                    },
                    vec![scalar_as(Kind::UInt(9), node::UINT32)],
                )],
            ),
        ));
    }
    object(members)
}

/// Whether two trees hold the same values, flags and storage choices.
fn identical(a: &Value, b: &Value) -> bool {
    if a != b || a.storage() != b.storage() {
        return false;
    }
    match (a.kind(), b.kind()) {
        (Kind::Array(x), Kind::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| identical(p, q))
        }
        (Kind::Object(x), Kind::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y.iter())
                    .all(|((k, p), (l, q))| k == l && identical(p, q))
        }
        _ => true,
    }
}

#[test]
fn a_parsed_document_keeps_flags_widths_and_array_forms() {
    for version in [Version::V4, Version::V5] {
        let tree = irregular(version);
        let block = write(&tree, &options(version)).expect("write");
        let back = parse(&block).expect("read back");
        assert!(identical(&back.root, &tree), "{version:?}");

        let again = write(&back.root, &options(version)).expect("write");
        assert_eq!(
            decode(&again).expect("decode").payload,
            decode(&block).expect("decode").payload
        );
    }
}

#[test]
fn what_was_stored_is_repeated_not_rederived() {
    let tree = irregular(Version::V5);
    let block = write(&tree, &options(Version::V5)).expect("write");
    let plain = parse(&block).expect("parse").root;
    let hand_built = object(vec![
        ("byte", Value::int(-2)),
        (
            "same_but_general",
            Value::array(vec![Value::double(2.5); 2]),
        ),
    ]);
    let hand_block = write(&hand_built, &options(Version::V5)).expect("write");
    let hand = parse(&hand_block).expect("parse").root;
    assert_eq!(type_code(plain.get("byte").unwrap()), node::INT32_AS_BYTE);
    assert_eq!(type_code(hand.get("byte").unwrap()), node::INT32);
    assert_eq!(
        form(plain.get("same_but_general").unwrap()),
        ArrayForm::General
    );
    assert!(matches!(
        form(hand.get("same_but_general").unwrap()),
        ArrayForm::Auxiliary { .. }
    ));
}

#[test]
fn a_changed_value_falls_back_to_a_storage_that_holds_it() {
    let tree = object(vec![
        ("byte", scalar_as(Kind::Int(5), node::INT32_AS_BYTE)),
        ("zero", scalar_as(Kind::Int(0), node::INT64_ZERO)),
        (
            "nums",
            array_as(
                ArrayForm::Typed {
                    ty: node::INT32,
                    flags: 0,
                },
                vec![scalar_as(Kind::Int(1), node::INT32)],
            ),
        ),
    ]);
    let block = write(&tree, &options(Version::V5)).expect("write");
    let mut doc = parse(&block).expect("parse");

    let Kind::Object(root) = doc.root.kind().clone() else {
        panic!("root is not an object");
    };
    let mut edited = Object::default();
    for (key, value) in root.iter() {
        let mut value = value.clone();
        match key {
            "byte" => *value.kind_mut() = Kind::Int(1000),
            "zero" => *value.kind_mut() = Kind::Int(7),
            _ => {
                if let Kind::Array(items) = value.kind_mut() {
                    items.push(Value::double(0.5));
                }
            }
        }
        edited.push(key.to_string(), value);
    }
    doc.root = Value::from(edited);

    let rewritten = write(&doc.root, &options(Version::V5)).expect("write");
    let back = parse(&rewritten).expect("parse").root;
    assert_eq!(back.get("byte").and_then(Value::as_i64), Some(1000));
    assert_eq!(back.get("zero").and_then(Value::as_i64), Some(7));
    let nums = back.get("nums").and_then(Value::as_array).expect("nums");
    assert_eq!(nums.len(), 2);
    assert_eq!(nums[1].as_f64(), Some(0.5));
    assert_eq!(doc.root, back);
}

#[test]
fn flags_survive_the_binary_round_trip() {
    let v = object(vec![
        ("path", s("a.vmdl").with_flags(flag::RESOURCE)),
        (
            "both",
            s("x").with_flags(flag::RESOURCE_NAME | flag::PANORAMA),
        ),
        ("odd", s("y").with_flags(0x80 | flag::SUBCLASS)),
        (
            "list",
            Value::array(vec![
                s("p").with_flags(flag::SOUND_EVENT),
                s("q").with_flags(flag::SOUND_EVENT),
            ]),
        ),
        (
            "block",
            object(vec![("n", Value::int(1))]).with_flags(flag::RESOURCE),
        ),
    ]);
    for version in [Version::V4, Version::V5] {
        let block = write(&v, &options(version)).expect("write");
        let back = parse(&block).expect("parse").root;
        assert_eq!(back, v, "{version:?}");
        assert_eq!(back.get("path").unwrap().flags(), flag::RESOURCE);
        assert_eq!(back.get("odd").unwrap().flags(), 0x80 | flag::SUBCLASS);
    }
}

#[test]
fn elements_that_share_flags_make_a_typed_array_and_others_do_not() {
    let same = stored(
        Value::array(vec![
            s("p").with_flags(flag::RESOURCE),
            s("q").with_flags(flag::RESOURCE),
        ]),
        Version::V5,
    );
    assert_eq!(
        form(&same),
        ArrayForm::ByteLength {
            ty: node::STRING,
            flags: flag::RESOURCE
        }
    );

    let mixed = stored(
        Value::array(vec![s("p").with_flags(flag::RESOURCE), s("q")]),
        Version::V5,
    );
    assert_eq!(form(&mixed), ArrayForm::General);
    assert_eq!(items(&mixed)[0].flags(), flag::RESOURCE);
    assert_eq!(items(&mixed)[1].flags(), 0);
}

#[test]
fn flags_survive_a_trip_through_text() {
    let v = object(vec![
        ("path", s("a.vmdl").with_flags(flag::RESOURCE)),
        (
            "both",
            s("x").with_flags(flag::RESOURCE_NAME | flag::SOUND_EVENT),
        ),
        (
            "list",
            Value::array(vec![s("p").with_flags(flag::PANORAMA), s("q")]),
        ),
        (
            "block",
            object(vec![("n", Value::int(1))]).with_flags(flag::SUBCLASS),
        ),
    ]);
    let text = out_text(&v);
    assert!(text.contains("path = resource:\"a.vmdl\""), "{text}");
    assert_eq!(parse_text(&text).expect("parse text").root, v);
}

#[test]
fn text_and_binary_agree_on_flags() {
    let v = object(vec![("path", s("a.vmdl").with_flags(flag::RESOURCE))]);
    let block = write(&v, &options(Version::V5)).expect("write");
    let doc = parse(&block).expect("parse");
    let again = parse_text(&out_text(&doc.root)).expect("parse text");
    assert_eq!(again.root, v);
}

#[test]
fn v4_starts_the_string_blob_on_an_eight_byte_boundary_without_an_eight_byte_pool() {
    let v = object(vec![("a", Value::from(true)), ("b", Value::int(5))]);
    let block = write(&v, &options(Version::V4)).expect("write");
    let payload = decode(&block).expect("decode").payload;

    // String count, root member count, two name indices and the integer: five words, then
    // four bytes of padding.
    let mut expected: Vec<u8> = [2u32, 2, 0, 1, 5]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    expected.extend([0; 4]);
    expected.extend(b"a\0b\0");
    expected.extend([9, 13, 11]);
    expected.extend(0xFFEE_DD00u32.to_le_bytes());
    assert_eq!(payload, expected);
}

#[test]
fn what_a_version_cannot_store_is_written_another_way() {
    let aux = object(vec![(
        "a",
        array_as(
            ArrayForm::Auxiliary {
                ty: node::DOUBLE,
                flags: 0,
            },
            vec![scalar_as(Kind::Double(0.0), node::DOUBLE)],
        ),
    )]);
    let block = write(&aux, &options(Version::V4)).expect("v4 has no auxiliary buffer");
    let back = parse(&block).expect("parse").root;
    assert_eq!(back, aux);
    assert_eq!(
        form(back.get("a").unwrap()),
        ArrayForm::ByteLength {
            ty: node::DOUBLE,
            flags: 0
        }
    );

    let long = object(vec![(
        "a",
        array_as(
            ArrayForm::ByteLength {
                ty: node::INT32,
                flags: 0,
            },
            vec![scalar_as(Kind::Int(3), node::INT32); 256],
        ),
    )]);
    let block = write(&long, &options(Version::V5)).expect("256 elements need a wide length");
    let back = parse(&block).expect("parse").root;
    assert_eq!(back, long);
    assert!(matches!(
        form(back.get("a").unwrap()),
        ArrayForm::Typed { .. }
    ));

    let mismatched = object(vec![(
        "a",
        array_as(
            ArrayForm::Typed {
                ty: node::INT32,
                flags: 0,
            },
            vec![scalar_as(Kind::Double(0.0), node::DOUBLE)],
        ),
    )]);
    let block = write(&mismatched, &options(Version::V5)).expect("element type disagrees");
    assert_eq!(parse(&block).expect("parse").root, mismatched);
}

#[test]
fn options_taken_from_a_header_reproduce_its_layout() {
    let v = object(vec![("a", Value::int(5))]);
    let combos = [
        (Version::V4, Compression::None),
        (Version::V5, Compression::None),
        #[cfg(feature = "lz4")]
        (Version::V5, Compression::Lz4),
        #[cfg(feature = "zstd")]
        (Version::V4, Compression::Zstd),
    ];
    for (version, compression) in combos {
        let wanted = WriteOptions {
            version,
            compression,
            format: [7; 16],
            ..WriteOptions::default()
        };
        let doc = parse(&write(&v, &wanted).expect("write")).expect("parse");
        assert_eq!(doc.options.version, wanted.version);
        assert_eq!(doc.options.compression, wanted.compression);
        assert_eq!(doc.options.format, wanted.format);
    }
}
