//! Editing a parsed tree: storage hints that go stale must never reach the bytes.

use crate::node;
use crate::value::{ArrayForm, Kind, Storage};
use crate::{Compression, Document, Object, Value, Version, WriteOptions, decode, parse, write};

fn options(version: Version) -> WriteOptions {
    WriteOptions {
        version,
        compression: Compression::None,
        ..WriteOptions::default()
    }
}

fn hinted(kind: Kind, ty: u8) -> Value {
    Value::stored(kind, 0, Storage::Scalar(ty))
}

fn array_hinted(form: ArrayForm, items: Vec<Value>) -> Value {
    Value::stored(Kind::Array(items), 0, Storage::Array(form))
}

fn root(members: Vec<(&str, Value)>) -> Value {
    Value::from(Object::from(members))
}

/// Write, then read back, so the tree carries hints exactly as a file would give them.
fn through_a_file(tree: &Value, version: Version) -> Document {
    parse(&write(tree, &options(version)).expect("write")).expect("parse")
}

fn scalar_code_of(v: &Value) -> u8 {
    match v.storage() {
        Storage::Scalar(ty) => ty,
        other => panic!("not a scalar: {other:?}"),
    }
}

fn form_of(v: &Value) -> ArrayForm {
    match v.storage() {
        Storage::Array(form) => form,
        other => panic!("not an array: {other:?}"),
    }
}

#[test]
fn a_byte_sized_integer_that_grows_is_written_wider() {
    for version in [Version::V5, Version::V4, Version::V1, Version::Legacy] {
        let tree = root(vec![("n", hinted(Kind::Int(5), node::INT32_AS_BYTE))]);
        let mut doc = through_a_file(&tree, version);
        doc.options = options(version);
        assert_eq!(
            scalar_code_of(doc.root.get("n").unwrap()),
            node::INT32_AS_BYTE
        );

        *doc.root.get_mut("n").unwrap().kind_mut() = Kind::Int(300);
        let back = parse(&doc.to_bytes().unwrap()).unwrap();
        assert_eq!(back.root.get("n").and_then(Value::as_i64), Some(300));
        assert_eq!(scalar_code_of(back.root.get("n").unwrap()), node::INT32);
    }
}

#[test]
fn an_edit_that_still_fits_keeps_the_stored_width() {
    let tree = root(vec![("n", hinted(Kind::Int(5), node::INT32_AS_BYTE))]);
    let mut doc = through_a_file(&tree, Version::V5);
    *doc.root.get_mut("n").unwrap().kind_mut() = Kind::Int(-100);
    let back = parse(&doc.to_bytes_with(&options(Version::V5)).unwrap()).unwrap();
    assert_eq!(
        scalar_code_of(back.root.get("n").unwrap()),
        node::INT32_AS_BYTE
    );
    assert_eq!(back.root.get("n").and_then(Value::as_i64), Some(-100));
}

#[test]
fn a_constant_type_code_does_not_outlive_its_value() {
    let tree = root(vec![
        ("zero", hinted(Kind::Int(0), node::INT64_ZERO)),
        ("one", hinted(Kind::Int(1), node::INT64_ONE)),
        ("half", hinted(Kind::Double(0.0), node::DOUBLE_ZERO)),
        ("yes", hinted(Kind::Bool(true), node::BOOLEAN_TRUE)),
    ]);
    let mut doc = through_a_file(&tree, Version::V5);
    *doc.root.get_mut("zero").unwrap().kind_mut() = Kind::Int(5);
    *doc.root.get_mut("one").unwrap().kind_mut() = Kind::String("one".into());
    *doc.root.get_mut("half").unwrap().kind_mut() = Kind::Double(0.5);
    *doc.root.get_mut("yes").unwrap().kind_mut() = Kind::Bool(false);
    let expected = doc.root.clone();
    let back = parse(&doc.to_bytes_with(&options(Version::V5)).unwrap()).unwrap();
    assert_eq!(back.root, expected);
}

#[test]
fn changing_the_kind_of_a_hinted_value_is_safe() {
    let tree = root(vec![("n", hinted(Kind::Int(5), node::INT32_AS_BYTE))]);
    let mut doc = through_a_file(&tree, Version::V5);
    for kind in [
        Kind::UInt(u64::MAX),
        Kind::Double(1.25),
        Kind::Null,
        Kind::Array(vec![Value::int(1)]),
        Kind::Object(Object::new()),
        Kind::String("s".into()),
        Kind::Blob(vec![1, 2]),
    ] {
        *doc.root.get_mut("n").unwrap().kind_mut() = kind.clone();
        let back = parse(&doc.to_bytes_with(&options(Version::V5)).unwrap()).unwrap();
        assert_eq!(back.root.get("n").unwrap().kind(), &kind);
    }
}

#[test]
fn an_element_that_changes_type_turns_a_typed_array_into_a_general_one() {
    let nums = array_hinted(
        ArrayForm::Typed {
            ty: node::INT32,
            flags: 0,
        },
        vec![
            hinted(Kind::Int(1), node::INT32),
            hinted(Kind::Int(2), node::INT32),
        ],
    );
    let tree = root(vec![("nums", nums)]);
    let mut doc = through_a_file(&tree, Version::V5);
    assert!(matches!(
        form_of(doc.root.get("nums").unwrap()),
        ArrayForm::Typed { .. }
    ));

    doc.root.get_mut("nums").unwrap().as_array_mut().unwrap()[1] = Value::from("two");
    let back = parse(&doc.to_bytes_with(&options(Version::V5)).unwrap()).unwrap();
    let items = back.root.get("nums").unwrap().as_array().unwrap();
    assert_eq!(items[0].as_i64(), Some(1));
    assert_eq!(items[1].as_str(), Some("two"));
    assert_eq!(form_of(back.root.get("nums").unwrap()), ArrayForm::General);
}

#[test]
fn an_element_that_outgrows_the_element_type_is_written_wider() {
    let wide = |v: i64| hinted(Kind::Int(v), node::INT16);
    let tree = root(vec![(
        "nums",
        array_hinted(
            ArrayForm::Typed {
                ty: node::INT16,
                flags: 0,
            },
            vec![wide(1), wide(2)],
        ),
    )]);
    let mut doc = through_a_file(&tree, Version::V5);
    doc.root.get_mut("nums").unwrap().as_array_mut().unwrap()[0] = Value::int(1 << 40);
    let back = parse(&doc.to_bytes_with(&options(Version::V5)).unwrap()).unwrap();
    let items = back.root.get("nums").unwrap().as_array().unwrap();
    assert_eq!(items[0].as_i64(), Some(1 << 40));
    assert_eq!(items[1].as_i64(), Some(2));
}

#[test]
fn a_flag_change_on_one_element_breaks_the_shared_element_type() {
    let tree = root(vec![(
        "paths",
        array_hinted(
            ArrayForm::ByteLength {
                ty: node::STRING,
                flags: 0,
            },
            vec![Value::from("a"), Value::from("b")],
        ),
    )]);
    let mut doc = through_a_file(&tree, Version::V4);
    doc.root.get_mut("paths").unwrap().as_array_mut().unwrap()[0].set_flags(crate::flag::RESOURCE);
    let expected = doc.root.clone();
    let back = parse(&doc.to_bytes_with(&options(Version::V4)).unwrap()).unwrap();
    assert_eq!(back.root, expected);
}

#[test]
fn pushing_past_255_elements_leaves_the_one_byte_length_behind() {
    for version in [Version::V4, Version::V5] {
        let words: Vec<Value> = (0..255).map(|i| Value::from(format!("w{i}"))).collect();
        let tree = root(vec![("words", Value::array(words))]);
        let mut doc = through_a_file(&tree, version);
        assert!(matches!(
            form_of(doc.root.get("words").unwrap()),
            ArrayForm::ByteLength { .. }
        ));

        doc.root
            .get_mut("words")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(Value::from("one more"));
        let back = parse(&doc.to_bytes_with(&options(version)).unwrap()).unwrap();
        let items = back.root.get("words").unwrap().as_array().unwrap();
        assert_eq!(items.len(), 256);
        assert_eq!(items[255].as_str(), Some("one more"));
        assert!(matches!(
            form_of(back.root.get("words").unwrap()),
            ArrayForm::Typed { .. }
        ));
    }
}

#[test]
fn an_auxiliary_array_moved_to_v4_is_written_with_a_byte_length() {
    let tree = root(vec![(
        "xs",
        array_hinted(
            ArrayForm::Auxiliary {
                ty: node::DOUBLE,
                flags: 0,
            },
            vec![Value::double(0.5), Value::double(1.5)],
        ),
    )]);
    let v5 = through_a_file(&tree, Version::V5);
    assert!(matches!(
        form_of(v5.root.get("xs").unwrap()),
        ArrayForm::Auxiliary { .. }
    ));
    let v4 = parse(&write(&v5.root, &options(Version::V4)).unwrap()).unwrap();
    assert!(matches!(
        form_of(v4.root.get("xs").unwrap()),
        ArrayForm::ByteLength { .. }
    ));
    assert_eq!(v4.root, v5.root);
}

#[test]
fn removing_and_adding_members_to_a_parsed_object_writes_correctly() {
    let tree = root(vec![
        ("keep", hinted(Kind::Int(5), node::INT32_AS_BYTE)),
        ("drop", Value::from("gone")),
        ("tail", Value::from(true)),
    ]);
    let mut doc = through_a_file(&tree, Version::V5);
    let object = doc.root.as_object_mut().unwrap();
    assert!(object.remove("drop").is_some());
    object.insert("added", 7);
    object.insert("keep", 6);
    let expected = doc.root.clone();

    let back = parse(&doc.to_bytes_with(&options(Version::V5)).unwrap()).unwrap();
    assert_eq!(back.root, expected);
    let names: Vec<_> = back.root.as_object().unwrap().keys().collect();
    assert_eq!(names, ["keep", "tail", "added"]);
}

#[test]
fn editing_through_iter_mut_writes_correctly() {
    let tree = root(vec![
        ("a", hinted(Kind::Int(1), node::INT32_AS_BYTE)),
        ("b", hinted(Kind::Int(2), node::INT32_AS_BYTE)),
    ]);
    let mut doc = through_a_file(&tree, Version::V5);
    for (_, v) in doc.root.as_object_mut().unwrap().iter_mut() {
        let n = v.as_i64().unwrap();
        *v.kind_mut() = Kind::Int(n * 1000);
    }
    let back = parse(&doc.to_bytes_with(&options(Version::V5)).unwrap()).unwrap();
    assert_eq!(back.root.get("a").and_then(Value::as_i64), Some(1000));
    assert_eq!(back.root.get("b").and_then(Value::as_i64), Some(2000));
}

#[test]
fn set_kind_drops_the_hint() {
    let mut v = hinted(Kind::Int(5), node::INT32_AS_BYTE);
    v.set_kind(Kind::Int(6));
    assert_eq!(v.storage(), Storage::Inferred);
    let block = write(&root(vec![("n", v)]), &options(Version::V5)).unwrap();
    let back = parse(&block).unwrap();
    assert_eq!(scalar_code_of(back.root.get("n").unwrap()), node::INT32);
}

#[test]
fn clear_storage_asks_for_the_default_encoding_everywhere() {
    let tree = root(vec![
        ("byte", hinted(Kind::Int(5), node::INT32_AS_BYTE)),
        (
            "list",
            array_hinted(
                ArrayForm::General,
                vec![hinted(Kind::Int(1), node::INT32), Value::int(2)],
            ),
        ),
    ]);
    let mut parsed = through_a_file(&tree, Version::V5);
    parsed.root.clear_storage();
    let plain = root(vec![
        ("byte", Value::int(5)),
        ("list", Value::array(vec![Value::int(1), Value::int(2)])),
    ]);
    let a = decode(&parsed.to_bytes_with(&options(Version::V5)).unwrap()).unwrap();
    let b = decode(&write(&plain, &options(Version::V5)).unwrap()).unwrap();
    assert_eq!(a.payload, b.payload);
}

#[test]
fn an_unedited_parsed_tree_writes_the_payload_it_was_read_from() {
    let tree = root(vec![
        ("byte", hinted(Kind::Int(5), node::INT32_AS_BYTE)),
        ("float", hinted(Kind::Double(1.5), node::FLOAT)),
        (
            "list",
            array_hinted(
                ArrayForm::General,
                vec![Value::double(2.5), Value::double(2.5)],
            ),
        ),
    ]);
    let block = write(&tree, &options(Version::V5)).unwrap();
    let doc = parse(&block).unwrap();
    let again = doc.to_bytes_with(&options(Version::V5)).unwrap();
    assert_eq!(
        decode(&again).unwrap().payload,
        decode(&block).unwrap().payload
    );
}

#[test]
fn clones_keep_the_hints_and_equality_ignores_them() {
    let hinted_five = hinted(Kind::Int(5), node::INT32_AS_BYTE);
    let plain_five = Value::int(5);
    assert_eq!(hinted_five, plain_five);
    assert_eq!(hinted_five.clone().storage(), hinted_five.storage());

    let a = write(
        &root(vec![("n", hinted_five.clone())]),
        &options(Version::V5),
    )
    .unwrap();
    let b = write(&root(vec![("n", plain_five)]), &options(Version::V5)).unwrap();
    assert_ne!(a, b, "the hint changes the bytes");
}

#[test]
fn equality_still_compares_flags() {
    assert_ne!(
        Value::from("x"),
        Value::from("x").with_flags(crate::flag::RESOURCE)
    );
}

#[test]
fn debug_output_hides_the_storage_hint() {
    let v = hinted(Kind::Int(5), node::INT32_AS_BYTE);
    let text = format!("{v:?}");
    assert_eq!(text, "Value { kind: Int(5) }");
    let flagged = Value::from("x").with_flags(4);
    assert_eq!(
        format!("{flagged:?}"),
        "Value { kind: String(\"x\"), flags: 4 }"
    );
}

#[test]
fn nan_doubles_are_not_equal_to_themselves_but_round_trip_by_bits() {
    let v = Value::double(f64::NAN);
    assert_ne!(v, v.clone());
    let block = write(&root(vec![("x", v)]), &options(Version::V5)).unwrap();
    let back = parse(&block).unwrap();
    let Kind::Double(d) = back.root.get("x").unwrap().kind() else {
        panic!("not a double");
    };
    assert_eq!(d.to_bits(), f64::NAN.to_bits());
}

#[test]
fn a_single_precision_float_is_stored_in_four_bytes_on_request() {
    let tree = root(vec![("x", Value::float(0.1))]);
    let doc = through_a_file(&tree, Version::V5);
    assert_eq!(scalar_code_of(doc.root.get("x").unwrap()), node::FLOAT);
    assert_eq!(doc.root.get("x").and_then(Value::to_f32_lossy), Some(0.1));
    assert_eq!(
        doc.root.get("x").and_then(Value::as_f64),
        Some(f64::from(0.1f32))
    );
}

#[test]
fn from_f32_widens_exactly_and_stores_a_double() {
    let v = Value::from(0.1f32);
    assert_eq!(v.kind(), &Kind::Double(f64::from(0.1f32)));
    let doc = through_a_file(&root(vec![("x", v)]), Version::V5);
    assert_eq!(scalar_code_of(doc.root.get("x").unwrap()), node::DOUBLE);
}

#[test]
fn a_float_edited_to_a_value_f32_cannot_hold_is_written_as_a_double() {
    let tree = root(vec![("x", Value::float(0.5))]);
    let mut doc = through_a_file(&tree, Version::V5);
    *doc.root.get_mut("x").unwrap().kind_mut() = Kind::Double(0.1);
    let back = parse(&doc.to_bytes_with(&options(Version::V5)).unwrap()).unwrap();
    assert_eq!(back.root.get("x").and_then(Value::as_f64), Some(0.1));
}

#[test]
fn multiline_hints_follow_the_string() {
    let mut v = Value::multiline("a\nb");
    assert!(v.is_multiline());
    *v.kind_mut() = Kind::Int(1);
    assert!(!v.is_multiline());
    v.set_kind(Kind::String("x".into()));
    assert!(!v.is_multiline());
}
