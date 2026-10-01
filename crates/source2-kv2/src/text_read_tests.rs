//! Hand-written `KeyValues2` text against expected trees.

use crate::*;

const HDR: &str = "<!-- dmx encoding keyvalues2 1 format dmx 1 -->\n";

const ID_A: &str = "8aa7f40b-f824-4431-aea0-0f5e40cfc8b5";
const ID_B: &str = "0a5d19f3-6a67-4a23-94d1-7c1474291afe";

fn uuid(s: &str) -> Uuid {
    Uuid::parse(s).unwrap()
}

fn parse(body: &str) -> Result<Document> {
    Document::parse(format!("{HDR}{body}").as_bytes())
}

fn parse_ok(body: &str) -> Document {
    parse(body).unwrap()
}

fn attr<'a>(doc: &'a Document, elem: usize, name: &str) -> &'a Value {
    doc.elements[elem]
        .attribute(name)
        .unwrap_or_else(|| panic!("no attribute {name}"))
}

fn one(ty: &str, val: &str) -> Value {
    let doc = parse_ok(&format!(
        "\"Root\"\n{{\n\t\"id\" \"elementid\" \"{ID_A}\"\n\t\"x\" \"{ty}\" \"{val}\"\n}}\n"
    ));
    attr(&doc, 0, "x").clone()
}

#[test]
fn header_fields() {
    let text = format!(
        "<!-- dmx encoding keyvalues2 4 format vmap 29 -->\n\"Root\"\n{{\n\t\"id\" \"elementid\" \"{ID_A}\"\n}}\n"
    );
    let doc = Document::parse(text.as_bytes()).unwrap();
    assert_eq!(doc.encoding, Encoding::KeyValues2);
    assert_eq!(doc.encoding_version, 4);
    assert_eq!(doc.format, "vmap");
    assert_eq!(doc.format_version, 29);
}

#[test]
fn header_rejects_garbage() {
    for bad in [
        "",
        "hello\n\"Root\"\n{\n}\n",
        "<!-- dmx encoding keyvalues2 1 format dmx -->\n",
        "<!-- dmx encoding keyvalues2 x format dmx 1 -->\n",
        "<!-- dmx encoding keyvalues2 1 format dmx 1 -->",
        "<!-- dmx encoding keyvalues2 1 fromat dmx 1 -->\n",
        "<!-- dmx encoding keyvalues2 1 format dmx 1 extra -->\n",
    ] {
        assert!(
            matches!(Document::parse(bad.as_bytes()), Err(Error::BadHeader(_))),
            "{bad:?}"
        );
    }
}

#[test]
fn unknown_encoding_is_reported() {
    let e = Document::parse(b"<!-- dmx encoding ascii 1 format dmx 1 -->\n").unwrap_err();
    assert!(matches!(e, Error::UnsupportedEncoding(ref n) if n == "ascii"));
}

#[test]
fn minimal_element() {
    let doc = parse_ok(&format!(
        "\"DmElement\"\n{{\n\t\"id\" \"elementid\" \"{ID_A}\"\n\t\"name\" \"string\" \"root\"\n}}\n"
    ));
    assert_eq!(doc.elements.len(), 1);
    let e = doc.root().unwrap();
    assert_eq!(e.class, "DmElement");
    assert_eq!(e.name, "root");
    assert_eq!(e.id, uuid(ID_A));
    assert!(e.attributes.is_empty());
}

#[test]
fn unnamed_element_has_empty_name() {
    let doc = parse_ok(&format!(
        "\"Root\"\n{{\n\t\"id\" \"elementid\" \"{ID_A}\"\n}}\n"
    ));
    assert_eq!(doc.root().unwrap().name, "");
}

#[test]
fn scalar_types() {
    assert_eq!(one("int", "-42"), Value::Int(-42));
    assert_eq!(one("float", "1.5"), Value::Float(1.5));
    assert_eq!(one("float", "-0.25"), Value::Float(-0.25));
    assert_eq!(one("bool", "1"), Value::Bool(true));
    assert_eq!(one("bool", "0"), Value::Bool(false));
    assert_eq!(
        one("string", "hello world"),
        Value::String("hello world".into())
    );
    assert_eq!(
        one("binary", "00FFa1"),
        Value::Binary(vec![0x00, 0xff, 0xa1])
    );
    assert_eq!(one("binary", ""), Value::Binary(vec![]));
    assert_eq!(one("time", "1.5"), Value::Time(Time(15000)));
    assert_eq!(one("time", "0.0001"), Value::Time(Time(1)));
    assert_eq!(
        one("color", "255 128 0 64"),
        Value::Color(Color {
            r: 255,
            g: 128,
            b: 0,
            a: 64
        })
    );
    assert_eq!(one("vector2", "1 2"), Value::Vector2([1.0, 2.0]));
    assert_eq!(one("vector3", "1 2.5 -3"), Value::Vector3([1.0, 2.5, -3.0]));
    assert_eq!(
        one("vector4", "1 2 3 4"),
        Value::Vector4([1.0, 2.0, 3.0, 4.0])
    );
    assert_eq!(one("qangle", "10 20 30"), Value::QAngle([10.0, 20.0, 30.0]));
    assert_eq!(
        one("quaternion", "0 0 0 1"),
        Value::Quaternion([0.0, 0.0, 0.0, 1.0])
    );
    assert_eq!(
        one("uint64", "18446744073709551615"),
        Value::UInt64(u64::MAX)
    );
    assert_eq!(one("uint8", "200"), Value::UInt8(200));
    assert_eq!(one("objectid", ID_B), Value::ObjectId(uuid(ID_B)));
}

#[test]
fn matrix_accepts_any_whitespace() {
    let want: [f32; 16] = core::array::from_fn(|i| i as f32);
    let rows = one("matrix", "0 1 2 3\n4 5 6 7\n8 9 10 11\n12 13 14 15");
    assert_eq!(rows, Value::Matrix(want));
    let flat = one("matrix", "0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15");
    assert_eq!(flat, Value::Matrix(want));
}

#[test]
fn angle_alias_reads_as_qangle() {
    assert_eq!(one("angle", "1 2 3"), Value::QAngle([1.0, 2.0, 3.0]));
}

#[test]
fn bad_scalar_values_are_errors() {
    for (ty, val) in [
        ("int", "abc"),
        ("int", "99999999999"),
        ("float", "x"),
        ("bool", "2"),
        ("binary", "abc"),
        ("binary", "zz"),
        ("color", "1 2 3"),
        ("color", "1 2 3 256"),
        ("vector3", "1 2"),
        ("vector3", "1 2 3 4"),
        ("matrix", "1 2 3"),
        ("uint8", "256"),
        ("objectid", "nope"),
    ] {
        let r = parse(&format!(
            "\"Root\"\n{{\n\t\"id\" \"elementid\" \"{ID_A}\"\n\t\"x\" \"{ty}\" \"{val}\"\n}}\n"
        ));
        assert!(
            matches!(r, Err(Error::Syntax { .. })),
            "{ty} {val:?}: {r:?}"
        );
    }
}

#[test]
fn string_escapes() {
    let v = one("string", r#"a\"b\\c\nd\te"#);
    assert_eq!(v, Value::String("a\"b\\c\nd\te".into()));
}

#[test]
fn scalar_arrays() {
    let doc = parse_ok(&format!(
        r#""Root"
{{
	"id" "elementid" "{ID_A}"
	"ints" "int_array"
	[
		"1",
		"-2",
		"3"
	]
	"strs" "string_array" [ "a", "b c" ]
	"none" "float_array"
	[
	]
	"vecs" "vector3_array"
	[
		"1 2 3",
		"4 5 6"
	]
	"bools" "bool_array" [ "1", "0" ]
	"bins" "binary_array" [ "FF", "" ]
	"times" "time_array" [ "2" ]
	"colors" "color_array" [ "1 2 3 4" ]
	"qa" "qangle_array" [ "1 2 3" ]
	"q" "quaternion_array" [ "0 0 0 1" ]
	"v2" "vector2_array" [ "1 2" ]
	"v4" "vector4_array" [ "1 2 3 4" ]
	"m" "matrix_array" [ "1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1" ]
	"u" "uint64_array" [ "7" ]
	"b" "uint8_array" [ "7" ]
	"f" "float_array" [ "0.5", ]
}}
"#
    ));
    assert_eq!(
        attr(&doc, 0, "ints"),
        &Value::Array(
            ValueType::Int,
            vec![Value::Int(1), Value::Int(-2), Value::Int(3)]
        )
    );
    assert_eq!(
        attr(&doc, 0, "strs"),
        &Value::Array(
            ValueType::String,
            vec![Value::String("a".into()), Value::String("b c".into())]
        )
    );
    assert_eq!(
        attr(&doc, 0, "none"),
        &Value::Array(ValueType::Float, vec![])
    );
    assert_eq!(
        attr(&doc, 0, "vecs"),
        &Value::Array(
            ValueType::Vector3,
            vec![
                Value::Vector3([1.0, 2.0, 3.0]),
                Value::Vector3([4.0, 5.0, 6.0])
            ]
        )
    );
    assert_eq!(
        attr(&doc, 0, "bools"),
        &Value::Array(ValueType::Bool, vec![Value::Bool(true), Value::Bool(false)])
    );
    assert_eq!(
        attr(&doc, 0, "bins"),
        &Value::Array(
            ValueType::Binary,
            vec![Value::Binary(vec![0xff]), Value::Binary(vec![])]
        )
    );
    assert_eq!(
        attr(&doc, 0, "times"),
        &Value::Array(ValueType::Time, vec![Value::Time(Time(20000))])
    );
    assert_eq!(attr(&doc, 0, "m").value_type(), ValueType::Matrix);
    assert_eq!(
        attr(&doc, 0, "f"),
        &Value::Array(ValueType::Float, vec![Value::Float(0.5)])
    );
    for n in ["colors", "qa", "q", "v2", "v4", "u", "b"] {
        assert!(attr(&doc, 0, n).is_array(), "{n}");
    }
}

#[test]
fn inline_child_and_reference() {
    let doc = parse_ok(&format!(
        r#""Root"
{{
	"id" "elementid" "{ID_A}"
	"child" "Leaf"
	{{
		"id" "elementid" "{ID_B}"
		"name" "string" "leaf"
	}}
	"again" "element" "{ID_B}"
	"nothing" "element" ""
}}
"#
    ));
    assert_eq!(doc.elements.len(), 2);
    assert_eq!(doc.elements[1].class, "Leaf");
    assert_eq!(doc.elements[1].name, "leaf");
    let leaf = Value::Element(ElementRef::Element(ElementId(1)));
    assert_eq!(attr(&doc, 0, "child"), &leaf);
    assert_eq!(attr(&doc, 0, "again"), &leaf);
    assert_eq!(attr(&doc, 0, "nothing"), &Value::Element(ElementRef::Null));
}

#[test]
fn forward_reference_resolves() {
    let doc = parse_ok(&format!(
        r#""Root"
{{
	"id" "elementid" "{ID_A}"
	"early" "element" "{ID_B}"
	"late" "Leaf"
	{{
		"id" "elementid" "{ID_B}"
	}}
}}
"#
    ));
    assert_eq!(attr(&doc, 0, "early"), attr(&doc, 0, "late"));
}

#[test]
fn reference_back_to_ancestor_makes_a_cycle() {
    let doc = parse_ok(&format!(
        r#""Root"
{{
	"id" "elementid" "{ID_A}"
	"child" "Leaf"
	{{
		"id" "elementid" "{ID_B}"
		"parent" "element" "{ID_A}"
	}}
}}
"#
    ));
    assert_eq!(
        doc.elements[1].attribute("parent"),
        Some(&Value::Element(ElementRef::Element(ElementId(0))))
    );
}

#[test]
fn element_arrays_mix_inline_and_references() {
    let doc = parse_ok(&format!(
        r#""Root"
{{
	"id" "elementid" "{ID_A}"
	"kids" "element_array"
	[
		"Leaf"
		{{
			"id" "elementid" "{ID_B}"
		}},
		"element" "{ID_B}",
		"element" "",
		"Leaf"
		{{
			"id" "elementid" "c0c0c0c0-0000-0000-0000-000000000001"
		}}
	]
}}
"#
    ));
    assert_eq!(doc.elements.len(), 3);
    assert_eq!(
        attr(&doc, 0, "kids"),
        &Value::Array(
            ValueType::Element,
            vec![
                Value::Element(ElementRef::Element(ElementId(1))),
                Value::Element(ElementRef::Element(ElementId(1))),
                Value::Element(ElementRef::Null),
                Value::Element(ElementRef::Element(ElementId(2))),
            ]
        )
    );
}

#[test]
fn unresolved_reference_is_an_error_by_default() {
    let r = parse(&format!(
        "\"Root\"\n{{\n\t\"id\" \"elementid\" \"{ID_A}\"\n\t\"r\" \"element\" \"{ID_B}\"\n}}\n"
    ));
    assert!(matches!(r, Err(Error::UnresolvedReference(u)) if u == uuid(ID_B)));
}

#[test]
fn unresolved_reference_in_array_is_an_error() {
    let r = parse(&format!(
        "\"Root\"\n{{\n\t\"id\" \"elementid\" \"{ID_A}\"\n\t\"r\" \"element_array\" [ \"element\" \"{ID_B}\" ]\n}}\n"
    ));
    assert!(matches!(r, Err(Error::UnresolvedReference(_))));
}

#[test]
fn unresolved_reference_can_be_kept() {
    let text = format!(
        "{HDR}\"Root\"\n{{\n\t\"id\" \"elementid\" \"{ID_A}\"\n\t\"r\" \"element\" \"{ID_B}\"\n}}\n"
    );
    let doc = Document::parse_with(
        text.as_bytes(),
        &ReadOptions {
            allow_unresolved: true,
        },
    )
    .unwrap();
    assert_eq!(
        attr(&doc, 0, "r"),
        &Value::Element(ElementRef::External(uuid(ID_B)))
    );
}

#[test]
fn duplicate_ids_are_an_error() {
    let r = parse(&format!(
        r#""Root"
{{
	"id" "elementid" "{ID_A}"
	"a" "Leaf"
	{{
		"id" "elementid" "{ID_A}"
	}}
}}
"#
    ));
    assert!(matches!(r, Err(Error::DuplicateId(u)) if u == uuid(ID_A)));
}

#[test]
fn missing_id_is_an_error() {
    let r = parse("\"Root\"\n{\n\t\"name\" \"string\" \"x\"\n}\n");
    assert!(matches!(r, Err(Error::Syntax { .. })), "{r:?}");
}

#[test]
fn crlf_and_comments() {
    let text = format!(
        "<!-- dmx encoding keyvalues2 1 format dmx 1 -->\r\n// leading\r\n\"Root\"\r\n{{\r\n\t\"id\" \"elementid\" \"{ID_A}\" // trailing\r\n\t\"n\" \"int\" \"5\"\r\n}}\r\n"
    );
    let doc = Document::parse(text.as_bytes()).unwrap();
    assert_eq!(attr(&doc, 0, "n"), &Value::Int(5));
}

#[test]
fn prefix_elements_come_before_the_root() {
    let doc = parse_ok(&format!(
        r#""$prefix_element$"
{{
	"id" "elementid" "c0c0c0c0-0000-0000-0000-00000000000f"
	"thumb" "string" "abc"
	"refs" "string_array" [ "a.vmat" ]
}}
"Root"
{{
	"id" "elementid" "{ID_A}"
}}
"#
    ));
    assert_eq!(doc.prefix.len(), 1);
    assert_eq!(doc.prefix[0].len(), 2);
    assert_eq!(doc.prefix[0][0].name, "thumb");
    assert_eq!(doc.elements.len(), 1);
    assert_eq!(doc.root().unwrap().class, "Root");
}

#[test]
fn extra_top_level_elements_join_the_arena() {
    let doc = parse_ok(&format!(
        r#""Root"
{{
	"id" "elementid" "{ID_A}"
	"r" "element" "{ID_B}"
}}
"Other"
{{
	"id" "elementid" "{ID_B}"
}}
"#
    ));
    assert_eq!(doc.elements.len(), 2);
    assert_eq!(
        attr(&doc, 0, "r"),
        &Value::Element(ElementRef::Element(ElementId(1)))
    );
}

#[test]
fn noids_encoding_needs_no_ids_and_invents_distinct_ones() {
    let doc = Document::parse(
        b"<!-- dmx encoding keyvalues2_noids 1 format dmx 1 -->\n\"Root\"\n{\n\t\"name\" \"string\" \"r\"\n\t\"c\" \"Leaf\"\n\t{\n\t\t\"name\" \"string\" \"l\"\n\t}\n}\n".as_slice(),
    )
    .unwrap();
    assert_eq!(doc.encoding, Encoding::KeyValues2NoIds);
    assert_eq!(doc.elements.len(), 2);
    assert_ne!(doc.elements[0].id, doc.elements[1].id);
    assert_eq!(doc.elements[1].name, "l");
}

#[test]
fn empty_body_is_an_empty_document() {
    let doc = parse_ok("");
    assert!(doc.elements.is_empty());
    assert!(doc.root().is_none());
}

#[test]
fn unicode_strings_survive() {
    assert_eq!(
        one("string", "caf\u{e9} \u{1f600}"),
        Value::String("caf\u{e9} \u{1f600}".into())
    );
}

#[test]
fn invalid_utf8_is_an_error() {
    let mut text =
        format!("{HDR}\"Root\"\n{{\n\t\"id\" \"elementid\" \"{ID_A}\"\n\t\"s\" \"string\" \"")
            .into_bytes();
    text.push(0xff);
    text.extend_from_slice(b"\"\n}\n");
    assert!(matches!(Document::parse(&text), Err(Error::Syntax { .. })));
}

#[test]
fn truncation_anywhere_is_an_error_not_a_panic() {
    let full = format!(
        r#"{HDR}"Root"
{{
	"id" "elementid" "{ID_A}"
	"kids" "element_array"
	[
		"Leaf"
		{{
			"id" "elementid" "{ID_B}"
			"v" "vector3" "1 2 3"
		}},
		"element" "{ID_B}"
	]
}}
"#
    );
    assert!(Document::parse(full.as_bytes()).is_ok());
    let bytes = full.as_bytes();
    let body_start = HDR.len();
    for cut in body_start + 1..bytes.len() - 1 {
        let r = Document::parse(&bytes[..cut]);
        // a cut that lands exactly between two complete top-level elements would parse,
        // but nothing in this document has one
        assert!(r.is_err(), "cut at {cut} parsed");
    }
}

#[test]
fn errors_carry_line_numbers() {
    let r = parse("\"Root\"\n{\n\t\"id\" \"elementid\" \"nope\"\n}\n");
    match r {
        Err(Error::Syntax { line, .. }) => assert_eq!(line, 4),
        other => panic!("{other:?}"),
    }
}

#[test]
fn structural_garbage_is_an_error() {
    for body in [
        "\"Root\"",
        "\"Root\" {",
        "{ }",
        "\"Root\"\n{\n\"a\"\n}\n",
        "\"Root\"\n{\n\"a\" \"int\"\n}\n",
        "\"Root\"\n{\n\"a\" \"int_array\" \"1\"\n}\n",
        "\"Root\"\n{\n\"a\" \"int_array\" [ \"1\" \"2\" ]\n}\n",
        "\"Root\"\n{\n\"a\" \"int_array\" [ \"1\"\n}\n",
        "\"Root\"\n{\n\"a\" \"Leaf\" \"x\"\n}\n",
        "\"Root\"\n{\n\"a\" \"element_array\" [ \"Leaf\" ]\n}\n",
        "\"Root\"\n{\n\"a\" \"int\" \"1\n}\n",
        "\"Root\"\n{\n\"a\" \"int\" \"1\"\n}\n}\n",
        "\"Root\"\n{\n\"a\" \"int_array\" [ [ ] ]\n}\n",
    ] {
        assert!(parse(body).is_err(), "{body:?}");
    }
}

#[test]
fn hostile_nesting_is_an_error() {
    let mut body = String::new();
    let depth = 100_000;
    for i in 0..depth {
        body.push_str(&format!("\"E\"\n{{\n\"c{i}\" "));
    }
    let r = parse(&body);
    assert!(r.is_err());

    let mut deep = String::from("\"Root\"\n{\n");
    for _ in 0..100_000 {
        deep.push_str("\"c\" \"E\"\n{\n");
    }
    assert!(matches!(parse(&deep), Err(Error::TooDeep)));
}

#[test]
fn moderate_nesting_is_fine() {
    let mut s = String::from("\"Root\"\n{\n");
    for _ in 0..50 {
        s.push_str("\"c\" \"E\"\n{\n");
    }
    for _ in 0..50 {
        s.push_str("}\n");
    }
    s.push_str("}\n");
    // ids are required, so this needs noids to stay short
    let text = format!("<!-- dmx encoding keyvalues2_noids 1 format dmx 1 -->\n{s}");
    let doc = Document::parse(text.as_bytes()).unwrap();
    assert_eq!(doc.elements.len(), 51);
}

#[test]
fn real_world_shape_from_a_shipped_file() {
    let text = format!(
        "<!-- dmx encoding keyvalues2 1 format dmx 1 -->\r\n\"PlayerStats\"\r\n{{\r\n\t\"id\" \"elementid\" \"{ID_A}\"\r\n\t\"aClassStats\" \"element_array\" \r\n\t[\r\n\t\t\"ClassStats_t\"\r\n\t\t{{\r\n\t\t\t\"id\" \"elementid\" \"{ID_B}\"\r\n\t\t\t\"accumulated\" \"RoundStats_t\"\r\n\t\t\t{{\r\n\t\t\t\t\"id\" \"elementid\" \"30f47fdc-47ae-4348-8e12-1f7e724e91fa\"\r\n\t\t\t\t\"iBackstabs\" \"int\" \"0\"\r\n\t\t\t}}\r\n\t\t\t\r\n\t\t}}\r\n\t]\r\n}}\r\n"
    );
    let doc = Document::parse(text.as_bytes()).unwrap();
    assert_eq!(doc.elements.len(), 3);
    assert_eq!(doc.elements[2].class, "RoundStats_t");
}
