//! Hand construction, layout preservation and text details.

use crate::*;

const ID_A: &str = "8aa7f40b-f824-4431-aea0-0f5e40cfc8b5";
const ID_B: &str = "0a5d19f3-6a67-4a23-94d1-7c1474291afe";
const ID_C: &str = "30f47fdc-47ae-4348-8e12-1f7e724e91fa";

fn uuid(s: &str) -> Uuid {
    Uuid::parse(s).unwrap()
}

fn text_doc(body: &str) -> String {
    format!("<!-- dmx encoding keyvalues2 1 format dmx 1 -->\n{body}")
}

fn rewrite(src: &str) -> String {
    let doc = Document::parse(src.as_bytes()).unwrap();
    String::from_utf8(doc.to_bytes().unwrap()).unwrap()
}

#[test]
fn from_scratch_build_writes_in_every_encoding() {
    for (enc, ver) in [
        (Encoding::KeyValues2, 1),
        (Encoding::Binary, 2),
        (Encoding::Binary, 5),
        (Encoding::Binary, 9),
    ] {
        let mut doc = Document::with_encoding(enc, ver, "thing", 3);
        let root = doc.add_element(
            Element::new("Root")
                .name("top")
                .attr("count", 3)
                .attr("ratio", 0.5f32)
                .attr("label", "hi")
                .attr("on", true)
                .attr("pos", [1.0f32, 2.0, 3.0])
                .push("tags", "a")
                .push("tags", "b"),
        );
        let kid = doc.add_child(root, "kid", Element::new("Kid").attr("n", 1));
        doc.push_child(root, "kids", Element::new("Kid").attr("n", 2));
        doc.push_link(root, "kids", kid);
        let bytes = doc.to_bytes().unwrap();
        let mut back = Document::parse(&bytes).unwrap();
        back.string_table.clear();
        assert_eq!(back, doc, "{enc:?} {ver}");
        assert_eq!(back.elements.len(), 3);
    }
}

#[test]
fn default_document_is_empty_text() {
    let d = Document::default();
    assert_eq!(d.encoding, Encoding::KeyValues2);
    assert!(d.elements.is_empty());
    assert_eq!(
        d.to_bytes().unwrap(),
        b"<!-- dmx encoding keyvalues2 1 format dmx 1 -->\n"
    );
}

#[test]
fn value_from_impls_pick_the_natural_type() {
    assert_eq!(Value::from(1i32), Value::Int(1));
    assert_eq!(Value::from(1.5f32), Value::Float(1.5));
    assert_eq!(Value::from(true), Value::Bool(true));
    assert_eq!(Value::from("x"), Value::String("x".into()));
    assert_eq!(Value::from(String::from("x")), Value::String("x".into()));
    assert_eq!(Value::from(vec![1u8, 2]), Value::Binary(vec![1, 2]));
    assert_eq!(Value::from(7u8), Value::UInt8(7));
    assert_eq!(Value::from(7u64), Value::UInt64(7));
    assert_eq!(Value::from([1.0f32, 2.0]), Value::Vector2([1.0, 2.0]));
    assert_eq!(
        Value::from(ElementId(3)),
        Value::Element(ElementRef::Element(ElementId(3)))
    );
    assert_eq!(
        Value::qangle([1.0, 2.0, 3.0]),
        Value::QAngle([1.0, 2.0, 3.0])
    );
    assert_eq!(
        Value::array(ValueType::Int, [1, 2]),
        Value::Array(ValueType::Int, vec![Value::Int(1), Value::Int(2)])
    );
    assert_eq!(
        Value::array::<i32>(ValueType::Int, []),
        Value::Array(ValueType::Int, vec![])
    );
}

#[test]
fn element_set_replaces_and_push_appends() {
    let e = Element::new("E")
        .attr("a", 1)
        .attr("a", 2)
        .push("l", 1)
        .push("l", 2);
    assert_eq!(e.attributes.len(), 2);
    assert_eq!(e.attribute("a"), Some(&Value::Int(2)));
    assert_eq!(
        e.attribute("l"),
        Some(&Value::Array(
            ValueType::Int,
            vec![Value::Int(1), Value::Int(2)]
        ))
    );
}

#[test]
#[should_panic(expected = "not an array")]
fn push_onto_a_scalar_panics() {
    let _ = Element::new("E").attr("a", 1).push("a", 2);
}

#[test]
fn uuid_numeric_and_generated() {
    let u = Uuid::from_u128(0x8aa7f40b_f824_4431_aea0_0f5e40cfc8b5);
    assert_eq!(u, uuid(ID_A));
    assert_eq!(u.as_u128(), 0x8aa7f40b_f824_4431_aea0_0f5e40cfc8b5);
    let ids: std::collections::HashSet<_> = (0..1000).map(|_| Uuid::generate()).collect();
    assert_eq!(ids.len(), 1000);
    let g = Uuid::generate();
    assert_eq!(g.0[6] >> 4, 4);
    assert_eq!(g.0[8] >> 6, 2);
    assert_eq!(Uuid::parse(&g.to_string()), Some(g));
}

#[test]
fn id_and_name_positions_are_preserved() {
    let src = text_doc(&format!(
        "\"Root\"\n{{\n\t\"name\" \"string\" \"first\"\n\t\"a\" \"int\" \"1\"\n\t\"id\" \"elementid\" \"{ID_A}\"\n\t\"b\" \"int\" \"2\"\n}}\n\n"
    ));
    let doc = Document::parse(src.as_bytes()).unwrap();
    let t = doc.root().unwrap().text;
    assert_eq!((t.id_position, t.name_position), (2, Some(0)));
    assert_eq!(rewrite(&src), src);
}

#[test]
fn id_after_all_attributes_is_preserved() {
    let src = text_doc(&format!(
        "\"Root\"\n{{\n\t\"a\" \"int\" \"1\"\n\t\"id\" \"elementid\" \"{ID_A}\"\n}}\n\n"
    ));
    assert_eq!(rewrite(&src), src);
}

#[test]
fn an_empty_name_that_was_written_stays_written() {
    let src = text_doc(&format!(
        "\"Root\"\n{{\n\t\"id\" \"elementid\" \"{ID_A}\"\n\t\"name\" \"string\" \"\"\n}}\n\n"
    ));
    assert_eq!(rewrite(&src), src);
}

#[test]
fn reserved_names_are_rejected_when_writing_text() {
    for name in ["id", "name"] {
        let mut d = Document::default();
        d.add_element(Element::new("E").attr(name, 1));
        assert!(
            matches!(d.to_bytes(), Err(Error::InvalidModel(_))),
            "{name}"
        );
        d.encoding = Encoding::Binary;
        d.encoding_version = 5;
        d.to_bytes().expect("binary has no such collision");
    }
    let mut d = Document::default();
    d.add_element(Element::new("int"));
    assert!(matches!(d.to_bytes(), Err(Error::InvalidModel(_))));
    let mut d = Document::default();
    d.prefix.push(Prefix::new().attr("name", 1));
    d.add_element(Element::new("E"));
    assert!(matches!(d.to_bytes(), Err(Error::InvalidModel(_))));
}

#[test]
fn reader_rejects_reserved_name_misuse() {
    for body in [
        format!(
            "\"R\"\n{{\n\t\"id\" \"elementid\" \"{ID_A}\"\n\t\"id\" \"elementid\" \"{ID_B}\"\n}}\n"
        ),
        String::from("\"R\"\n{{\n\t\"id\" \"int\" \"1\"\n}}\n"),
        format!("\"R\"\n{{\n\t\"id\" \"elementid\" \"{ID_A}\"\n\t\"name\" \"int\" \"1\"\n}}\n"),
        format!(
            "\"R\"\n{{\n\t\"id\" \"elementid\" \"{ID_A}\"\n\t\"name\" \"string\" \"a\"\n\t\"name\" \"string\" \"b\"\n}}\n"
        ),
    ] {
        let r = Document::parse(text_doc(&body).as_bytes());
        assert!(matches!(r, Err(Error::Syntax { .. })), "{body}");
    }
}

fn float_doc(values: &[f32]) -> Document {
    let mut d = Document::default();
    d.add_element(
        Element::new("E").attr("f", Value::array(ValueType::Float, values.iter().copied())),
    );
    d
}

#[test]
fn special_floats_survive_text() {
    let values = [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        -0.0,
        0.0,
        1e-30,
        3.4e38,
        0.1,
    ];
    let d = float_doc(&values);
    let back = Document::parse(&d.to_bytes().unwrap()).unwrap();
    let Some(Value::Array(_, items)) = back.root().unwrap().attribute("f") else {
        panic!("array expected");
    };
    for (item, want) in items.iter().zip(values) {
        let Value::Float(got) = item else { panic!() };
        if want.is_nan() {
            assert!(got.is_nan());
        } else {
            assert_eq!(got.to_bits(), want.to_bits(), "{want}");
        }
    }
}

#[test]
fn special_floats_survive_binary() {
    let nan = f32::from_bits(0x7fc0_1234);
    let mut d = Document::with_encoding(Encoding::Binary, 5, "dmx", 1);
    d.add_element(Element::new("E").attr("f", nan).attr("z", -0.0f32));
    let back = Document::parse(&d.to_bytes().unwrap()).unwrap();
    let e = back.root().unwrap();
    let Some(Value::Float(f)) = e.attribute("f") else {
        panic!()
    };
    assert_eq!(f.to_bits(), nan.to_bits());
    let Some(Value::Float(z)) = e.attribute("z") else {
        panic!()
    };
    assert_eq!(z.to_bits(), (-0.0f32).to_bits());
}

#[test]
fn float_spellings_in_text() {
    let d = float_doc(&[f32::NAN, f32::INFINITY, -0.0]);
    let t = String::from_utf8(d.to_bytes().unwrap()).unwrap();
    assert!(t.contains("\"NaN\",\n"), "{t}");
    assert!(t.contains("\"inf\",\n"), "{t}");
    assert!(t.contains("\"-0\"\n"), "{t}");
}

#[test]
fn windows_runtime_float_spellings_are_read() {
    let src = text_doc(&format!(
        "\"R\"\n{{\n\t\"id\" \"elementid\" \"{ID_A}\"\n\t\"v\" \"vector3\" \"1.#INF -1.#INF -1.#IND00\"\n\t\"f\" \"float\" \"1.#QNAN\"\n}}\n"
    ));
    let doc = Document::parse(src.as_bytes()).unwrap();
    let e = doc.root().unwrap();
    let Some(Value::Vector3(v)) = e.attribute("v") else {
        panic!()
    };
    assert_eq!(v[0], f32::INFINITY);
    assert_eq!(v[1], f32::NEG_INFINITY);
    assert!(v[2].is_nan());
    let Some(Value::Float(f)) = e.attribute("f") else {
        panic!()
    };
    assert!(f.is_nan());
}

#[test]
fn fixed_ten_float_style_is_detected_and_reproduced() {
    let src = text_doc(&format!(
        "\"R\"\n{{\n\t\"id\" \"elementid\" \"{ID_A}\"\n\t\"p\" \"vector3\" \"10.0524339676 -0 0.5\"\n}}\n\n"
    ));
    let doc = Document::parse(src.as_bytes()).unwrap();
    assert_eq!(doc.text_style.float_format, FloatFormat::Fixed10);
    assert_eq!(rewrite(&src), src);
}

#[test]
fn short_floats_keep_the_shortest_style() {
    let src = text_doc(&format!(
        "\"R\"\n{{\n\t\"id\" \"elementid\" \"{ID_A}\"\n\t\"p\" \"vector3\" \"1.5 -0 0.1\"\n}}\n\n"
    ));
    let doc = Document::parse(src.as_bytes()).unwrap();
    assert_eq!(doc.text_style.float_format, FloatFormat::Shortest);
    assert_eq!(rewrite(&src), src);
}

#[test]
fn crlf_and_blank_line_habits_round_trip() {
    let src = format!(
        "<!-- dmx encoding keyvalues2 1 format dmx 1 -->\r\n\"R\"\r\n{{\r\n\t\"id\" \"elementid\" \"{ID_A}\"\r\n\t\"c\" \"C\"\r\n\t{{\r\n\t\t\"id\" \"elementid\" \"{ID_B}\"\r\n\t}}\r\n\t\r\n}}\r\n\r\n"
    );
    let doc = Document::parse(src.as_bytes()).unwrap();
    assert_eq!(doc.text_style.newline, Newline::CrLf);
    assert!(doc.text_style.blank_line_after_element);
    assert_eq!(rewrite(&src), src);
}

#[test]
fn missing_final_newline_round_trips() {
    for nl in ["\n", "\r\n"] {
        let src = format!(
            "<!-- dmx encoding keyvalues2_noids 1 format vtex 1 -->{nl}\"R\"{nl}{{{nl}\t\"b\" \"bool\" \"0\"{nl}}}"
        );
        let doc = Document::parse(src.as_bytes()).unwrap();
        assert!(!doc.text_style.final_newline);
        assert_eq!(rewrite(&src), src);
        let with = format!("{src}{nl}");
        assert!(
            Document::parse(with.as_bytes())
                .unwrap()
                .text_style
                .final_newline
        );
        assert_eq!(rewrite(&with), with);
    }
}

#[test]
fn compact_style_round_trips() {
    let src = text_doc(&format!(
        "\"R\"\n{{\n\t\"id\" \"elementid\" \"{ID_A}\"\n\t\"c\" \"C\"\n\t{{\n\t\t\"id\" \"elementid\" \"{ID_B}\"\n\t}}\n\t\"l\" \"int_array\" [ \"1\", \"2\" ]\n\t\"e\" \"element_array\" [ ]\n}}\n"
    ));
    let doc = Document::parse(src.as_bytes()).unwrap();
    assert!(!doc.text_style.blank_line_after_element);
    assert!(!doc.text_style.blank_line_after_block);
    assert!(doc.text_style.inline_arrays);
    assert_eq!(rewrite(&src), src);
}

#[test]
fn shared_elements_written_as_top_level_blocks_keep_that_shape() {
    let src = text_doc(&format!(
        "\"R\"\n{{\n\t\"id\" \"elementid\" \"{ID_A}\"\n\t\"x\" \"element\" \"{ID_B}\"\n\t\"y\" \"element\" \"{ID_B}\"\n}}\n\n\"S\"\n{{\n\t\"id\" \"elementid\" \"{ID_B}\"\n}}\n\n"
    ));
    let doc = Document::parse(src.as_bytes()).unwrap();
    assert_eq!(doc.elements.len(), 2);
    assert!(doc.elements[1].text.standalone);
    assert_eq!(rewrite(&src), src);
}

#[test]
fn forward_reference_to_an_inline_element_keeps_its_place() {
    let src = text_doc(&format!(
        "\"R\"\n{{\n\t\"id\" \"elementid\" \"{ID_A}\"\n\t\"early\" \"element\" \"{ID_C}\"\n\t\"k\" \"K\"\n\t{{\n\t\t\"id\" \"elementid\" \"{ID_B}\"\n\t\t\"deep\" \"D\"\n\t\t{{\n\t\t\t\"id\" \"elementid\" \"{ID_C}\"\n\t\t}}\n\t\t\n\t}}\n\t\n}}\n\n"
    ));
    let doc = Document::parse(src.as_bytes()).unwrap();
    assert_eq!(
        doc.root().unwrap().attribute("early"),
        Some(&Value::Element(ElementRef::Element(ElementId(2))))
    );
    assert_eq!(rewrite(&src), src);
}

#[test]
fn every_element_is_written_by_both_encodings() {
    let mut d = Document::default();
    let root = d.add_element(Element::new("Root"));
    let orphan = d.add_element(Element::new("Orphan").attr("n", 5));
    d.add_child(orphan, "kid", Element::new("Kid"));
    let _ = root;
    let text = Document::parse(&d.to_bytes().unwrap()).unwrap();
    assert_eq!(text.elements.len(), 3);
    d.encoding = Encoding::Binary;
    d.encoding_version = 5;
    let bin = Document::parse(&d.to_bytes().unwrap()).unwrap();
    assert_eq!(bin.elements.len(), 3);
    let classes = |doc: &Document| -> Vec<String> {
        let mut c: Vec<_> = doc.elements.iter().map(|e| e.class.clone()).collect();
        c.sort();
        c
    };
    assert_eq!(classes(&text), classes(&bin));
}

#[test]
fn noids_writes_unreachable_elements_too() {
    let mut d = Document::with_encoding(Encoding::KeyValues2NoIds, 1, "dmx", 1);
    d.add_element(Element::new("Root"));
    d.add_element(Element::new("Orphan"));
    let t = String::from_utf8(d.to_bytes().unwrap()).unwrap();
    assert!(t.contains("\"Orphan\""), "{t}");
}

#[test]
fn headers_accept_crlf_tabs_and_a_bare_line() {
    let a = Document::parse(b"<!--\tdmx  encoding keyvalues2 1 format dmx 1 -->\r\n").unwrap();
    assert_eq!(a.text_style.newline, Newline::CrLf);
    let b = Document::parse(b"<!-- dmx encoding keyvalues2 1 format dmx 1 -->").unwrap();
    assert!(b.elements.is_empty());
    assert!(
        Document::parse(b"\xEF\xBB\xBF<!-- dmx encoding keyvalues2 1 format dmx 1 -->\n").is_err()
    );
}

fn header(v: u32) -> Vec<u8> {
    let mut b = format!("<!-- dmx encoding binary {v} format dmx 1 -->\n").into_bytes();
    b.push(0);
    b
}

fn cstr(b: &mut Vec<u8>, s: &str) {
    b.extend_from_slice(s.as_bytes());
    b.push(0);
}

#[test]
fn binary_string_table_order_and_extras_are_kept() {
    let mut b = header(5);
    b.extend_from_slice(&4i32.to_le_bytes());
    for s in ["zzz", "unused", "R", "x"] {
        cstr(&mut b, s);
    }
    b.extend_from_slice(&1i32.to_le_bytes());
    b.extend_from_slice(&2i32.to_le_bytes());
    b.extend_from_slice(&0i32.to_le_bytes());
    b.extend_from_slice(&uuid(ID_A).to_guid_bytes());
    b.extend_from_slice(&1i32.to_le_bytes());
    b.extend_from_slice(&3i32.to_le_bytes());
    b.push(2);
    b.extend_from_slice(&9i32.to_le_bytes());
    let doc = Document::parse(&b).unwrap();
    assert_eq!(doc.string_table, ["zzz", "unused", "R", "x"]);
    assert_eq!(doc.to_bytes().unwrap(), b);
}

#[test]
fn new_strings_are_appended_after_the_kept_table() {
    let mut d = Document::with_encoding(Encoding::Binary, 5, "dmx", 1);
    d.string_table = vec!["b".into(), "a".into()];
    d.add_element(Element::new("a").name("b").attr("new", 1));
    let back = Document::parse(&d.to_bytes().unwrap()).unwrap();
    assert_eq!(back.string_table, ["b", "a", "new"]);
}

#[test]
fn binary_6_to_8_are_reported_as_unsupported() {
    for v in [6u32, 7, 8, 10] {
        let mut b = header(v);
        b.extend_from_slice(&0i32.to_le_bytes());
        let e = Document::parse(&b).unwrap_err();
        assert!(
            matches!(e, Error::UnsupportedVersion { version, .. } if version == v),
            "{e:?}"
        );
    }
}
