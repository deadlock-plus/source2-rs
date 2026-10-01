//! Text writer output and text round trips.

use crate::*;

const ID_A: &str = "8aa7f40b-f824-4431-aea0-0f5e40cfc8b5";
const ID_B: &str = "0a5d19f3-6a67-4a23-94d1-7c1474291afe";
const ID_C: &str = "30f47fdc-47ae-4348-8e12-1f7e724e91fa";

fn uuid(s: &str) -> Uuid {
    Uuid::parse(s).unwrap()
}

fn doc() -> Document {
    Document::with_encoding(Encoding::KeyValues2, 1, "dmx", 1)
}

fn text(d: &Document) -> String {
    String::from_utf8(d.to_bytes().unwrap()).unwrap()
}

fn roundtrip(d: &Document) -> Document {
    let bytes = d.to_bytes().unwrap();
    Document::parse(&bytes).unwrap()
}

#[test]
fn exact_output_for_a_small_document() {
    let mut d = doc();
    let mut root = Element::from_parts("DmElement", "root", uuid(ID_A));
    root.attributes.push(Attribute::new("n", Value::Int(3)));
    root.attributes
        .push(Attribute::new("s", Value::String("a\"b".into())));
    root.attributes.push(Attribute::new(
        "ints",
        Value::Array(ValueType::Int, vec![Value::Int(1), Value::Int(2)]),
    ));
    root.attributes.push(Attribute::new(
        "none",
        Value::Array(ValueType::Float, vec![]),
    ));
    d.add_element(root);
    let want = format!(
        "<!-- dmx encoding keyvalues2 1 format dmx 1 -->\n\
\"DmElement\"\n\
{{\n\
\t\"id\" \"elementid\" \"{ID_A}\"\n\
\t\"name\" \"string\" \"root\"\n\
\t\"n\" \"int\" \"3\"\n\
\t\"s\" \"string\" \"a\\\"b\"\n\
\t\"ints\" \"int_array\" \n\
\t[\n\
\t\t\"1\",\n\
\t\t\"2\"\n\
\t]\n\
\t\"none\" \"float_array\" \n\
\t[\n\
\t]\n\
}}\n\
\n"
    );
    assert_eq!(text(&d), want);
}

#[test]
fn unnamed_elements_omit_the_name_attribute() {
    let mut d = doc();
    d.add_element(Element::from_parts("E", "", uuid(ID_A)));
    assert!(!text(&d).contains("\"name\""));
}

#[test]
fn inline_then_reference_for_shared_elements() {
    let mut d = doc();
    d.add_element(Element::from_parts("Root", "", uuid(ID_A)));
    let leaf = d.add_element(Element::from_parts("Leaf", "", uuid(ID_B)));
    d.elements[0].attributes.push(Attribute::new(
        "first",
        Value::Element(ElementRef::Element(leaf)),
    ));
    d.elements[0].attributes.push(Attribute::new(
        "second",
        Value::Element(ElementRef::Element(leaf)),
    ));
    d.elements[0]
        .attributes
        .push(Attribute::new("null", Value::Element(ElementRef::Null)));
    d.elements[0].attributes.push(Attribute::new(
        "ext",
        Value::Element(ElementRef::External(uuid(ID_C))),
    ));
    let t = text(&d);
    assert!(t.contains(&format!("\"second\" \"element\" \"{ID_B}\"")));
    assert!(t.contains("\"null\" \"element\" \"\""));
    assert!(t.contains(&format!("\"ext\" \"element\" \"{ID_C}\"")));
    assert_eq!(t.matches("\"Leaf\"").count(), 1);
    let back = Document::parse(t.as_bytes()).unwrap();
    assert_eq!(back, d);
}

#[test]
fn cycles_are_written_as_references() {
    let mut d = doc();
    let a = d.add_element(Element::from_parts("A", "a", uuid(ID_A)));
    let b = d.add_element(Element::from_parts("B", "b", uuid(ID_B)));
    d.elements[0]
        .attributes
        .push(Attribute::new("b", Value::Element(ElementRef::Element(b))));
    d.elements[1]
        .attributes
        .push(Attribute::new("a", Value::Element(ElementRef::Element(a))));
    assert_eq!(roundtrip(&d), d);
}

#[test]
fn every_type_round_trips() {
    let mut d = doc();
    d.add_element(Element::from_parts("Root", "r", uuid(ID_A)));
    let other = d.add_element(Element::from_parts("Leaf", "l", uuid(ID_B)));
    let m: [f32; 16] = core::array::from_fn(|i| i as f32 * 0.5 - 3.0);
    let scalars = vec![
        Value::Element(ElementRef::Element(other)),
        Value::Int(i32::MIN),
        Value::Float(0.1),
        Value::Float(-3.5e-7),
        Value::Float(1.0e20),
        Value::Bool(true),
        Value::String("tab\t nl\n quote\" bs\\ caf\u{e9}".into()),
        Value::Binary(vec![0, 1, 0xfe, 0xff]),
        Value::ObjectId(uuid(ID_C)),
        Value::Time(Time(-12345)),
        Value::Time(Time(i32::MAX)),
        Value::Color(Color {
            r: 1,
            g: 2,
            b: 3,
            a: 255,
        }),
        Value::Vector2([1.0, -2.0]),
        Value::Vector3([0.1, 0.2, 0.3]),
        Value::Vector4([1.0, 2.0, 3.0, 4.0]),
        Value::QAngle([90.0, 0.0, -45.5]),
        Value::Quaternion([0.0, 0.0, 0.0, 1.0]),
        Value::Matrix(m),
        Value::UInt64(u64::MAX),
        Value::UInt8(255),
    ];
    for (i, v) in scalars.iter().enumerate() {
        d.elements[0]
            .attributes
            .push(Attribute::new(format!("s{i}"), v.clone()));
        d.elements[0].attributes.push(Attribute::new(
            format!("a{i}"),
            Value::Array(v.value_type(), vec![v.clone(), v.clone()]),
        ));
        d.elements[0].attributes.push(Attribute::new(
            format!("e{i}"),
            Value::Array(v.value_type(), vec![]),
        ));
    }
    assert_eq!(roundtrip(&d), d);
}

#[test]
fn time_text_is_exact() {
    let mut d = doc();
    d.add_element(Element::from_parts("E", "", uuid(ID_A)));
    for (ticks, s) in [
        (15000, "1.5"),
        (1, "0.0001"),
        (-1, "-0.0001"),
        (0, "0"),
        (20000, "2"),
    ] {
        d.elements[0].attributes.clear();
        d.elements[0]
            .attributes
            .push(Attribute::new("t", Value::Time(Time(ticks))));
        assert!(
            text(&d).contains(&format!("\"t\" \"time\" \"{s}\"")),
            "{ticks}"
        );
    }
}

#[test]
fn prefix_round_trips() {
    let mut d = doc();
    d.prefix.push(Prefix {
        id: None,
        attributes: vec![
            Attribute::new("a", Value::String("x".into())),
            Attribute::new(
                "refs",
                Value::Array(ValueType::String, vec![Value::String("m.vmat".into())]),
            ),
        ],
    });
    d.add_element(Element::from_parts("Root", "", uuid(ID_A)));
    let t = text(&d);
    assert!(t.contains("\"$prefix_element$\""));
    assert_eq!(roundtrip(&d), d);
}

#[test]
fn empty_document_round_trips() {
    let d = doc();
    assert_eq!(roundtrip(&d), d);
}

#[test]
fn text_to_model_to_text_is_stable() {
    let src = format!(
        "<!-- dmx encoding keyvalues2 1 format dmx 1 -->\n\
\"Root\"\n\
{{\n\
\t\"id\" \"elementid\" \"{ID_A}\"\n\
\t\"kids\" \"element_array\"\n\
\t[\n\
\t\t\"Leaf\"\n\
\t\t{{\n\
\t\t\t\"id\" \"elementid\" \"{ID_B}\"\n\
\t\t\t\"v\" \"vector3\" \"1 2 3\"\n\
\t\t}},\n\
\t\t\"element\" \"{ID_B}\"\n\
\t]\n\
}}\n"
    );
    let d = Document::parse(src.as_bytes()).unwrap();
    assert_eq!(text(&d), src);
}

#[test]
fn noids_output_has_no_ids_and_inlines_everything() {
    let mut d = Document::with_encoding(Encoding::KeyValues2NoIds, 1, "dmx", 1);
    let leaf = ElementId(1);
    d.add_element(Element::from_parts("Root", "r", uuid(ID_A)));
    d.add_element(Element::from_parts("Leaf", "l", uuid(ID_B)));
    d.elements[0].attributes.push(Attribute::new(
        "a",
        Value::Element(ElementRef::Element(leaf)),
    ));
    d.elements[0].attributes.push(Attribute::new(
        "b",
        Value::Element(ElementRef::Element(leaf)),
    ));
    let t = text(&d);
    assert!(t.starts_with("<!-- dmx encoding keyvalues2_noids 1 format dmx 1 -->\n"));
    assert!(!t.contains("elementid"));
    assert_eq!(t.matches("\"Leaf\"").count(), 2);
    let back = Document::parse(t.as_bytes()).unwrap();
    assert_eq!(back.elements.len(), 3);
    assert_eq!(back.elements[1].name, "l");
    assert_eq!(back.elements[2].name, "l");
}

#[test]
fn noids_cycle_is_rejected() {
    let mut d = Document::with_encoding(Encoding::KeyValues2NoIds, 1, "dmx", 1);
    d.add_element(Element::from_parts("Root", "r", uuid(ID_A)));
    d.elements[0].attributes.push(Attribute::new(
        "self",
        Value::Element(ElementRef::Element(ElementId(0))),
    ));
    assert!(matches!(
        d.to_bytes(),
        Err(Error::TooDeep | Error::InvalidModel(_))
    ));
}

#[test]
fn noids_rejects_external_references() {
    let mut d = Document::with_encoding(Encoding::KeyValues2NoIds, 1, "dmx", 1);
    d.add_element(Element::from_parts("Root", "r", uuid(ID_A)));
    d.elements[0].attributes.push(Attribute::new(
        "x",
        Value::Element(ElementRef::External(uuid(ID_B))),
    ));
    assert!(matches!(d.to_bytes(), Err(Error::InvalidModel(_))));
}

#[test]
fn invalid_models_are_errors() {
    let bad_index = {
        let mut d = doc();
        d.add_element(Element::from_parts("Root", "", uuid(ID_A)));
        d.elements[0].attributes.push(Attribute::new(
            "x",
            Value::Element(ElementRef::Element(ElementId(9))),
        ));
        d
    };
    let mixed_array = {
        let mut d = doc();
        d.add_element(Element::from_parts("Root", "", uuid(ID_A)));
        d.elements[0].attributes.push(Attribute::new(
            "x",
            Value::Array(ValueType::Int, vec![Value::Float(1.0)]),
        ));
        d
    };
    let nested_array = {
        let mut d = doc();
        d.add_element(Element::from_parts("Root", "", uuid(ID_A)));
        d.elements[0].attributes.push(Attribute::new(
            "x",
            Value::Array(ValueType::Int, vec![Value::Array(ValueType::Int, vec![])]),
        ));
        d
    };
    let space_in_format = {
        let mut d = doc();
        d.format = "a b".into();
        d
    };
    let dup = {
        let mut d = doc();
        d.add_element(Element::from_parts("A", "", uuid(ID_A)));
        d.add_element(Element::from_parts("B", "", uuid(ID_A)));
        d
    };
    let empty_class = {
        let mut d = doc();
        d.add_element(Element::from_parts("", "", uuid(ID_A)));
        d
    };
    let empty_attr_name = {
        let mut d = doc();
        d.add_element(Element::from_parts("A", "", uuid(ID_A)));
        d.elements[0]
            .attributes
            .push(Attribute::new("", Value::Int(1)));
        d
    };
    for (what, d) in [
        ("bad index", bad_index),
        ("mixed array", mixed_array),
        ("nested array", nested_array),
        ("space in format", space_in_format),
        ("duplicate ids", dup),
        ("empty class", empty_class),
        ("empty attribute name", empty_attr_name),
    ] {
        assert!(d.to_bytes().is_err(), "{what}");
    }
}

#[test]
fn shipped_file_shape_round_trips() {
    let src = format!(
        "<!-- dmx encoding keyvalues2 1 format dmx 1 -->\r\n\"PlayerStats\"\r\n{{\r\n\t\"id\" \"elementid\" \"{ID_A}\"\r\n\t\"aClassStats\" \"element_array\" \r\n\t[\r\n\t\t\"ClassStats_t\"\r\n\t\t{{\r\n\t\t\t\"id\" \"elementid\" \"{ID_B}\"\r\n\t\t\t\"iBackstabs\" \"int\" \"0\"\r\n\t\t}}\r\n\t]\r\n}}\r\n"
    );
    let d = Document::parse(src.as_bytes()).unwrap();
    assert_eq!(roundtrip(&d), d);
}

#[test]
fn unreachable_elements_are_written_as_extra_top_level_blocks() {
    let mut d = doc();
    d.add_element(Element::from_parts("Root", "", uuid(ID_A)));
    let mut orphan = Element::from_parts("Orphan", "o", uuid(ID_B));
    orphan.text.standalone = true;
    d.add_element(orphan);
    let t = text(&d);
    assert!(t.contains("\"Orphan\""), "{t}");
    let back = roundtrip(&d);
    assert_eq!(back, d);
}
