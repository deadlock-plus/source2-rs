//! Binary DMX against hand-assembled bytes.
//!
//! The version 9 layout was checked against a shipped Source 2 `.vmap`. The layouts for
//! versions 1 to 5 follow the Source 1 serializer and have no sample file to check against,
//! so the byte-level tests here pin this crate's reading of them, not Valve's.

use crate::*;

const ID_A: &str = "8aa7f40b-f824-4431-aea0-0f5e40cfc8b5";
const ID_B: &str = "0a5d19f3-6a67-4a23-94d1-7c1474291afe";
const ID_C: &str = "30f47fdc-47ae-4348-8e12-1f7e724e91fa";

/// Parses and forgets the string table, which hand-built documents do not carry.
fn parse_plain(bytes: &[u8]) -> Result<Document> {
    Document::parse(bytes).map(|mut d| {
        d.string_table.clear();
        d
    })
}

fn uuid(s: &str) -> Uuid {
    Uuid::parse(s).unwrap()
}

fn header(v: u32) -> Vec<u8> {
    let mut b = format!("<!-- dmx encoding binary {v} format dmx 1 -->\n").into_bytes();
    b.push(0);
    b
}

trait Put {
    fn cstr(&mut self, s: &str);
    fn i32(&mut self, n: i32);
    fn u16(&mut self, n: u16);
    fn id(&mut self, id: &str);
}

impl Put for Vec<u8> {
    fn cstr(&mut self, s: &str) {
        self.extend_from_slice(s.as_bytes());
        self.push(0);
    }
    fn i32(&mut self, n: i32) {
        self.extend_from_slice(&n.to_le_bytes());
    }
    fn u16(&mut self, n: u16) {
        self.extend_from_slice(&n.to_le_bytes());
    }
    fn id(&mut self, id: &str) {
        self.extend_from_slice(&uuid(id).to_guid_bytes());
    }
}

/// Root `Root`/`r` with an int, a string and a reference to `Leaf`/`l`.
fn sample_doc(version: u32) -> Document {
    let mut d = Document::with_encoding(Encoding::Binary, version, "dmx", 1);
    let mut root = Element::from_parts("Root", "r", uuid(ID_A));
    root.attributes.push(Attribute::new("n", Value::Int(7)));
    root.attributes
        .push(Attribute::new("s", Value::String("hi".into())));
    root.attributes.push(Attribute::new(
        "k",
        Value::Element(ElementRef::Element(ElementId(1))),
    ));
    d.add_element(root);
    d.add_element(Element::from_parts("Leaf", "l", uuid(ID_B)));
    d
}

fn sample_bytes(version: u32) -> Vec<u8> {
    let mut b = header(version);
    let wide = version >= 5;
    let wide_count = version >= 4;
    let names_in_table = version >= 4;
    let table: &[&str] = match version {
        1 => &[],
        2 | 3 => &["Root", "Leaf", "n", "s", "k"],
        _ => &["Root", "r", "Leaf", "l", "n", "s", "hi", "k"],
    };
    if version == 9 {
        b.i32(0);
    }
    if version >= 2 {
        if wide_count {
            b.i32(table.len() as i32);
        } else {
            b.u16(table.len() as u16);
        }
        for s in table {
            b.cstr(s);
        }
    }
    let idx = |b: &mut Vec<u8>, s: &str| {
        let i = table.iter().position(|t| *t == s).unwrap();
        if wide {
            b.i32(i as i32);
        } else {
            b.u16(i as u16);
        }
    };
    let sym = |b: &mut Vec<u8>, s: &str| {
        if version >= 2 {
            idx(b, s);
        } else {
            b.cstr(s);
        }
    };
    let name = |b: &mut Vec<u8>, s: &str| {
        if names_in_table {
            idx(b, s);
        } else {
            b.cstr(s);
        }
    };
    b.i32(2);
    sym(&mut b, "Root");
    name(&mut b, "r");
    b.id(ID_A);
    sym(&mut b, "Leaf");
    name(&mut b, "l");
    b.id(ID_B);

    b.i32(3);
    sym(&mut b, "n");
    b.push(2);
    b.i32(7);
    sym(&mut b, "s");
    b.push(5);
    name(&mut b, "hi");
    sym(&mut b, "k");
    b.push(1);
    b.i32(1);
    b.i32(0);
    b
}

const ALL: [u32; 6] = [1, 2, 3, 4, 5, 9];

#[test]
fn guid_byte_order() {
    let u = uuid(ID_A);
    assert_eq!(
        u.to_guid_bytes(),
        [
            0x0b, 0xf4, 0xa7, 0x8a, 0x24, 0xf8, 0x31, 0x44, 0xae, 0xa0, 0x0f, 0x5e, 0x40, 0xcf,
            0xc8, 0xb5
        ]
    );
    assert_eq!(Uuid::from_guid_bytes(u.to_guid_bytes()), u);
    assert_eq!(u.to_string(), ID_A);
}

#[test]
fn reads_each_version() {
    for v in ALL {
        let got = parse_plain(&sample_bytes(v)).unwrap_or_else(|e| panic!("v{v}: {e}"));
        assert_eq!(got, sample_doc(v), "v{v}");
    }
}

#[test]
fn writes_each_version_byte_for_byte() {
    for v in ALL {
        assert_eq!(sample_doc(v).to_bytes().unwrap(), sample_bytes(v), "v{v}");
    }
}

#[test]
fn unsupported_versions_are_reported() {
    for v in [0u32, 6, 7, 8, 10, 11, 4_000_000_000] {
        let mut b = header(v);
        b.i32(0);
        let e = parse_plain(&b).unwrap_err();
        assert!(
            matches!(e, Error::UnsupportedVersion { version, .. } if version == v),
            "v{v}: {e:?}"
        );
        let mut d = Document::with_encoding(Encoding::Binary, v, "dmx", 1);
        d.add_element(Element::from_parts("R", "", uuid(ID_A)));
        assert!(matches!(
            d.to_bytes(),
            Err(Error::UnsupportedVersion { .. })
        ));
    }
}

#[test]
fn header_requires_the_nul() {
    let mut no_nul = b"<!-- dmx encoding binary 5 format dmx 1 -->
"
    .to_vec();
    no_nul.extend_from_slice(&[1, 0, 0, 0]);
    assert!(matches!(parse_plain(&no_nul), Err(Error::BadHeader(_))));
}

/// Each case: value, bytes of the type tag and payload in version 5, same in version 9.
type Wire = Option<Vec<u8>>;

fn type_cases() -> Vec<(Value, Wire, Wire)> {
    let f = |x: f32| x.to_le_bytes().to_vec();
    let cat = |parts: &[Vec<u8>]| parts.concat();
    let both = |tag: u8, payload: Vec<u8>| {
        let mut v = vec![tag];
        v.extend(payload);
        (Some(v.clone()), Some(v))
    };
    let mut cases: Vec<(Value, Wire, Wire)> = Vec::new();
    let mut add =
        |v: Value, pair: (Option<Vec<u8>>, Option<Vec<u8>>)| cases.push((v, pair.0, pair.1));
    add(Value::Int(-2), both(2, vec![0xfe, 0xff, 0xff, 0xff]));
    add(Value::Float(1.5), both(3, f(1.5)));
    add(Value::Bool(true), both(4, vec![1]));
    add(Value::Binary(vec![1, 2]), both(6, vec![2, 0, 0, 0, 1, 2]));
    add(Value::Time(Time(15000)), both(7, vec![0x98, 0x3a, 0, 0]));
    add(
        Value::Color(Color {
            r: 1,
            g: 2,
            b: 3,
            a: 4,
        }),
        both(8, vec![1, 2, 3, 4]),
    );
    add(Value::Vector2([1.0, 2.0]), both(9, cat(&[f(1.0), f(2.0)])));
    add(
        Value::Vector3([1.0, 2.0, 3.0]),
        both(10, cat(&[f(1.0), f(2.0), f(3.0)])),
    );
    add(
        Value::Vector4([1.0, 2.0, 3.0, 4.0]),
        both(11, cat(&[f(1.0), f(2.0), f(3.0), f(4.0)])),
    );
    add(
        Value::QAngle([1.0, 2.0, 3.0]),
        both(12, cat(&[f(1.0), f(2.0), f(3.0)])),
    );
    add(
        Value::Quaternion([0.0, 0.0, 0.0, 1.0]),
        both(13, cat(&[f(0.0), f(0.0), f(0.0), f(1.0)])),
    );
    let m: [f32; 16] = core::array::from_fn(|i| i as f32);
    add(
        Value::Matrix(m),
        both(14, m.iter().flat_map(|x| x.to_le_bytes()).collect()),
    );
    add(
        Value::Array(ValueType::Element, vec![]),
        (Some(vec![15, 0, 0, 0, 0]), Some(vec![33, 0, 0, 0, 0])),
    );
    add(
        Value::Array(ValueType::Int, vec![Value::Int(1)]),
        (
            Some(vec![16, 1, 0, 0, 0, 1, 0, 0, 0]),
            Some(vec![34, 1, 0, 0, 0, 1, 0, 0, 0]),
        ),
    );
    add(
        Value::Array(ValueType::String, vec![Value::String("hi".into())]),
        (
            Some(vec![19, 1, 0, 0, 0, b'h', b'i', 0]),
            Some(vec![37, 1, 0, 0, 0, b'h', b'i', 0]),
        ),
    );
    add(
        Value::Array(ValueType::Binary, vec![Value::Binary(vec![9])]),
        (
            Some(vec![20, 1, 0, 0, 0, 1, 0, 0, 0, 9]),
            Some(vec![38, 1, 0, 0, 0, 1, 0, 0, 0, 9]),
        ),
    );
    add(
        Value::UInt64(1),
        (None, Some(vec![15, 1, 0, 0, 0, 0, 0, 0, 0])),
    );
    add(Value::UInt8(9), (None, Some(vec![16, 9])));
    cases
}

fn one_attr_doc(version: u32, v: Value) -> Document {
    let mut d = Document::with_encoding(Encoding::Binary, version, "dmx", 1);
    let mut e = Element::from_parts("E", "", uuid(ID_A));
    e.attributes.push(Attribute::new("a", v));
    d.add_element(e);
    d
}

#[test]
fn type_tags_and_payloads() {
    for (value, v5, v9) in type_cases() {
        for (version, want) in [(5, v5), (9, v9)] {
            let doc = one_attr_doc(version, value.clone());
            match want {
                Some(tail) => {
                    let bytes = doc.to_bytes().unwrap();
                    assert!(
                        bytes.ends_with(&tail),
                        "v{version} {value:?}: {:02x?} does not end with {tail:02x?}",
                        &bytes[bytes.len().saturating_sub(24)..]
                    );
                    assert_eq!(parse_plain(&bytes).unwrap(), doc, "v{version} {value:?}");
                }
                None => assert!(
                    matches!(doc.to_bytes(), Err(Error::InvalidModel(_))),
                    "v{version} {value:?} should not be writable"
                ),
            }
        }
    }
}

#[test]
fn string_scalar_goes_through_the_table_from_v4() {
    let doc = one_attr_doc(5, Value::String("zz".into()));
    let bytes = doc.to_bytes().unwrap();
    assert!(bytes.ends_with(&[5, 3, 0, 0, 0]), "{bytes:02x?}");
    let doc = one_attr_doc(2, Value::String("zz".into()));
    let bytes = doc.to_bytes().unwrap();
    assert!(bytes.ends_with(&[5, b'z', b'z', 0]), "{bytes:02x?}");
}

#[test]
fn object_id_exists_before_v3_and_time_from_v3() {
    let oid = Value::ObjectId(uuid(ID_C));
    for v in [1, 2] {
        let doc = one_attr_doc(v, oid.clone());
        let bytes = doc.to_bytes().unwrap();
        let mut tail = vec![7];
        tail.extend_from_slice(&uuid(ID_C).to_guid_bytes());
        assert!(bytes.ends_with(&tail));
        assert_eq!(parse_plain(&bytes).unwrap(), doc);
        assert!(one_attr_doc(v, Value::Time(Time(1))).to_bytes().is_err());
    }
    for v in [3, 4, 5, 9] {
        assert!(one_attr_doc(v, oid.clone()).to_bytes().is_err(), "v{v}");
    }
    for v in [1, 2, 3, 4, 5] {
        assert!(one_attr_doc(v, Value::UInt64(1)).to_bytes().is_err());
        assert!(one_attr_doc(v, Value::UInt8(1)).to_bytes().is_err());
    }
}

#[test]
fn external_reference_round_trips() {
    for v in ALL {
        let mut d = one_attr_doc(v, Value::Element(ElementRef::External(uuid(ID_B))));
        d.elements[0].attributes.push(Attribute::new(
            "arr",
            Value::Array(
                ValueType::Element,
                vec![
                    Value::Element(ElementRef::Null),
                    Value::Element(ElementRef::External(uuid(ID_C))),
                    Value::Element(ElementRef::Element(ElementId(0))),
                ],
            ),
        ));
        let bytes = d.to_bytes().unwrap();
        assert_eq!(parse_plain(&bytes).unwrap(), d, "v{v}");
    }
}

#[test]
fn null_and_external_markers_on_the_wire() {
    let d = one_attr_doc(5, Value::Element(ElementRef::Null));
    assert!(
        d.to_bytes()
            .unwrap()
            .ends_with(&[1, 0xff, 0xff, 0xff, 0xff])
    );
    let d = one_attr_doc(5, Value::Element(ElementRef::External(uuid(ID_B))));
    let mut tail = vec![1, 0xfe, 0xff, 0xff, 0xff];
    tail.extend_from_slice(ID_B.as_bytes());
    tail.push(0);
    assert!(d.to_bytes().unwrap().ends_with(&tail));
}

fn rich_doc(version: u32) -> Document {
    let mut d = Document::with_encoding(Encoding::Binary, version, "vmap", 28);
    let root = d.add_element(Element::from_parts("CMapRootElement", "", uuid(ID_A)));
    let leaf = d.add_element(Element::from_parts("Leaf", "leaf name", uuid(ID_B)));
    let other = d.add_element(Element::from_parts("Leaf", "", uuid(ID_C)));
    let m: [f32; 16] = core::array::from_fn(|i| i as f32 * 0.25);
    let mut vals = vec![
        Value::Element(ElementRef::Element(leaf)),
        Value::Int(i32::MAX),
        Value::Float(-0.1),
        Value::Bool(false),
        Value::String("caf\u{e9}".into()),
        Value::String(String::new()),
        Value::Binary(vec![0, 255, 1]),
        Value::Binary(vec![]),
        Value::Color(Color {
            r: 9,
            g: 8,
            b: 7,
            a: 6,
        }),
        Value::Vector2([1.0, 2.0]),
        Value::Vector3([3.0, 4.0, 5.0]),
        Value::Vector4([6.0, 7.0, 8.0, 9.0]),
        Value::QAngle([10.0, 11.0, 12.0]),
        Value::Quaternion([0.5, 0.5, 0.5, 0.5]),
        Value::Matrix(m),
    ];
    if version >= 3 {
        vals.push(Value::Time(Time(-77)));
    } else {
        vals.push(Value::ObjectId(uuid(ID_C)));
    }
    if version == 9 {
        vals.push(Value::UInt64(u64::MAX));
        vals.push(Value::UInt8(200));
    }
    for (i, v) in vals.iter().enumerate() {
        let t = v.value_type();
        let attrs = &mut d.elements[root.0 as usize].attributes;
        attrs.push(Attribute::new(format!("s{i}"), v.clone()));
        attrs.push(Attribute::new(
            format!("a{i}"),
            Value::Array(t, vec![v.clone(), v.clone()]),
        ));
        attrs.push(Attribute::new(format!("e{i}"), Value::Array(t, vec![])));
    }
    d.elements[leaf.0 as usize].attributes.push(Attribute::new(
        "back",
        Value::Element(ElementRef::Element(root)),
    ));
    d.elements[other.0 as usize].attributes.push(Attribute::new(
        "kids",
        Value::Array(
            ValueType::Element,
            vec![
                Value::Element(ElementRef::Element(leaf)),
                Value::Element(ElementRef::Null),
                Value::Element(ElementRef::Element(other)),
            ],
        ),
    ));
    if version == 9 {
        d.prefix.push(Prefix {
            id: None,
            attributes: vec![
                Attribute::new("thumb", Value::Binary(vec![1, 2, 3])),
                Attribute::new("fmt", Value::String("jpg".into())),
                Attribute::new(
                    "refs",
                    Value::Array(ValueType::String, vec![Value::String("a.vmat".into())]),
                ),
            ],
        });
    }
    d
}

#[test]
fn every_type_round_trips_in_every_version() {
    for v in ALL {
        let d = rich_doc(v);
        let bytes = d.to_bytes().unwrap_or_else(|e| panic!("v{v}: {e}"));
        let back = parse_plain(&bytes).unwrap_or_else(|e| panic!("v{v}: {e}"));
        assert_eq!(back, d, "v{v}");
        assert_eq!(back.to_bytes().unwrap(), bytes, "v{v} second pass");
    }
}

#[test]
fn v9_prefix_uses_inline_strings_before_the_table() {
    let mut d = Document::with_encoding(Encoding::Binary, 9, "dmx", 1);
    d.prefix.push(Prefix {
        id: None,
        attributes: vec![Attribute::new("p", Value::String("q".into()))],
    });
    d.add_element(Element::from_parts("R", "", uuid(ID_A)));
    let bytes = d.to_bytes().unwrap();
    let mut want = header(9);
    want.i32(1);
    want.i32(1);
    want.cstr("p");
    want.push(5);
    want.cstr("q");
    assert!(bytes.starts_with(&want), "{bytes:02x?}");
}

#[test]
fn empty_document_round_trips() {
    for v in ALL {
        let d = Document::with_encoding(Encoding::Binary, v, "dmx", 1);
        assert_eq!(parse_plain(&d.to_bytes().unwrap()).unwrap(), d, "v{v}");
    }
}

#[test]
fn text_and_binary_convert_both_ways() {
    let text = format!(
        "<!-- dmx encoding keyvalues2 1 format dmx 1 -->\n\"Root\"\n{{\n\t\"id\" \"elementid\" \"{ID_A}\"\n\t\"name\" \"string\" \"r\"\n\t\"kids\" \"element_array\"\n\t[\n\t\t\"Leaf\"\n\t\t{{\n\t\t\t\"id\" \"elementid\" \"{ID_B}\"\n\t\t\t\"v\" \"vector3\" \"1 2 3\"\n\t\t}},\n\t\t\"element\" \"{ID_B}\"\n\t]\n}}\n"
    );
    let mut doc = parse_plain(text.as_bytes()).unwrap();
    for v in [2u32, 5, 9] {
        doc.encoding = Encoding::Binary;
        doc.encoding_version = v;
        let bin = doc.to_bytes().unwrap();
        let mut back = parse_plain(&bin).unwrap();
        back.text_style = doc.text_style;
        assert_eq!(back, doc);
        back.encoding = Encoding::KeyValues2;
        back.encoding_version = 1;
        assert_eq!(back.to_bytes().unwrap(), text.as_bytes());
    }
}

fn expect_err(bytes: &[u8], what: &str) {
    let r = parse_plain(bytes);
    assert!(r.is_err(), "{what}: parsed as {r:?}");
}

#[test]
fn truncation_anywhere_is_an_error_not_a_panic() {
    for v in ALL {
        let bytes = rich_doc(v).to_bytes().unwrap();
        let start = header(v).len();
        for cut in start..bytes.len() {
            let r = parse_plain(&bytes[..cut]);
            assert!(r.is_err(), "v{v} cut {cut}/{} parsed", bytes.len());
        }
    }
}

#[test]
fn hostile_counts_are_errors_without_huge_allocations() {
    for v in ALL {
        let h = header(v);
        let wide = v >= 5;
        let big = 0x7fff_ffff_i32;

        let mut b = h.clone();
        if v == 9 {
            b.i32(big);
        } else if v >= 2 {
            if wide {
                b.i32(big);
            } else {
                b.u16(u16::MAX);
            }
        } else {
            b.i32(big);
        }
        expect_err(&b, &format!("v{v} huge first count"));

        let mut b = h.clone();
        if v == 9 {
            b.i32(0);
        }
        if v >= 2 {
            if wide {
                b.i32(0);
            } else {
                b.u16(0);
            }
        }
        b.i32(big);
        expect_err(&b, &format!("v{v} huge element count"));

        let mut b = h.clone();
        if v == 9 {
            b.i32(0);
        }
        if v >= 2 {
            if wide {
                b.i32(1);
            } else {
                b.u16(1);
            }
            b.cstr("R");
        }
        b.i32(-1);
        expect_err(&b, &format!("v{v} negative element count"));
    }
}

#[test]
fn hostile_inner_sizes_are_errors() {
    let v = 5;
    let mut base = header(v);
    base.i32(2);
    base.cstr("R");
    base.cstr("a");
    base.i32(1);
    base.i32(0);
    base.i32(0);
    base.id(ID_A);

    let with = |tail: &dyn Fn(&mut Vec<u8>), what: &str| {
        let mut b = base.clone();
        tail(&mut b);
        expect_err(&b, what);
    };
    with(&|b| b.i32(i32::MAX), "attribute count");
    with(&|b| b.i32(-5), "negative attribute count");
    with(
        &|b| {
            b.i32(1);
            b.i32(1);
            b.push(15);
            b.i32(i32::MAX);
        },
        "element array count",
    );
    with(
        &|b| {
            b.i32(1);
            b.i32(1);
            b.push(6);
            b.i32(i32::MAX);
        },
        "binary length",
    );
    with(
        &|b| {
            b.i32(1);
            b.i32(1);
            b.push(6);
            b.i32(-1);
        },
        "negative binary length",
    );
    with(
        &|b| {
            b.i32(1);
            b.i32(1);
            b.push(2 + 14 + 14);
            b.i32(1);
        },
        "type out of range",
    );
    with(
        &|b| {
            b.i32(1);
            b.i32(1);
            b.push(0);
        },
        "type zero",
    );
    with(
        &|b| {
            b.i32(1);
            b.i32(1);
            b.push(1);
            b.i32(5);
        },
        "element index out of range",
    );
    with(
        &|b| {
            b.i32(1);
            b.i32(1);
            b.push(1);
            b.i32(-7);
        },
        "negative element index",
    );
    with(
        &|b| {
            b.i32(1);
            b.i32(9);
            b.push(2);
            b.i32(0);
        },
        "attribute name index out of range",
    );
}

#[test]
fn string_table_hazards_are_errors() {
    let mut b = header(5);
    b.i32(2);
    b.cstr("abc");
    expect_err(&b, "table cut short");
    let mut b = header(5);
    b.i32(1);
    b.extend_from_slice(b"no nul at the end");
    expect_err(&b, "unterminated table string");
    let mut b = header(2);
    b.u16(1);
    b.cstr("R");
    b.i32(1);
    b.u16(7);
    b.cstr("x");
    b.id(ID_A);
    expect_err(&b, "class index outside table");
}

#[test]
fn bit_flips_never_panic() {
    let mut seed = 0x1234_5678_u32;
    let mut next = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        seed >> 8
    };
    for v in ALL {
        let good = rich_doc(v).to_bytes().unwrap();
        let start = header(v).len();
        for _ in 0..1500 {
            let mut b = good.clone();
            for _ in 0..1 + next() % 4 {
                let i = start + (next() as usize) % (b.len() - start);
                b[i] = next() as u8;
            }
            let _ = parse_plain(&b);
        }
    }
}

#[test]
fn text_bit_flips_never_panic() {
    let mut d = rich_doc(5);
    d.encoding = Encoding::KeyValues2;
    d.encoding_version = 1;
    let good = d.to_bytes().unwrap();
    let start = good.iter().position(|&c| c == b'\n').unwrap() + 1;
    let mut seed = 99_u32;
    let mut next = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        seed >> 8
    };
    for _ in 0..3000 {
        let mut b = good.clone();
        let i = start + (next() as usize) % (b.len() - start);
        b[i] = next() as u8;
        let _ = parse_plain(&b);
    }
}

#[test]
fn invalid_models_are_rejected_by_the_binary_writer() {
    let mut d = Document::with_encoding(Encoding::Binary, 5, "dmx", 1);
    d.add_element(Element::from_parts("R", "", uuid(ID_A)));
    d.elements[0].attributes.push(Attribute::new(
        "x",
        Value::Element(ElementRef::Element(ElementId(3))),
    ));
    assert!(d.to_bytes().is_err());
    let mut d = Document::with_encoding(Encoding::Binary, 5, "dmx", 1);
    d.add_element(Element::from_parts("R", "", uuid(ID_A)));
    d.prefix.push(Prefix {
        id: None,
        attributes: vec![],
    });
    assert!(d.to_bytes().is_err(), "prefix needs version 9");
}
