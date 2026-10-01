use crate::{Directive, Document, Encoding, Entry, Error, Options, Value};

fn doc(roots: Vec<Entry>) -> Document {
    Document::new(roots)
}

#[test]
fn reads_hand_built_bytes() {
    let bytes = [
        0, b'r', b'o', b'o', b't', 0, // section "root"
        1, b'k', 0, b'v', 0, // string k = v
        2, b'n', 0, 42, 0, 0, 0, // int n = 42
        8, // end root
        8, // end implicit top level
    ];
    let d = Document::from_binary(&bytes).unwrap();
    assert_eq!(
        d,
        doc(vec![Entry::section(
            "root",
            vec![Entry::string("k", "v"), Entry::int("n", 42)]
        )])
    );
}

#[test]
fn writes_exact_bytes() {
    let d = doc(vec![Entry::section(
        "r",
        vec![Entry::string("k", "v"), Entry::new("n", Value::Int(-1))],
    )]);
    assert_eq!(
        d.to_binary().unwrap(),
        [
            0, b'r', 0, 1, b'k', 0, b'v', 0, 2, b'n', 0, 255, 255, 255, 255, 8, 8
        ]
    );
}

#[test]
fn every_type_round_trips() {
    let d = doc(vec![
        Entry::section(
            "all",
            vec![
                Entry::string("s", "h\u{e9}llo"),
                Entry::string("empty", ""),
                Entry::new("i", Value::Int(i32::MIN)),
                Entry::new("f", Value::Float(-0.25)),
                Entry::new("p", Value::Ptr(0xdead_beef)),
                Entry::new("w", Value::WString("wide \u{1f980}".into())),
                Entry::new("c", Value::Color([1, 2, 3, 255])),
                Entry::new("u", Value::UInt64(u64::MAX)),
                Entry::int64("l", i64::MIN),
                Entry::section(
                    "nested",
                    vec![Entry::string("dup", "1"), Entry::string("dup", "2")],
                ),
            ],
        ),
        Entry::string("second", "root"),
    ]);
    let bytes = d.to_binary().unwrap();
    assert_eq!(Document::from_binary(&bytes).unwrap(), d);
}

#[test]
fn wstring_is_length_prefixed_utf16() {
    let d = doc(vec![Entry::new("w", Value::WString("a\u{1f980}".into()))]);
    assert_eq!(
        d.to_binary().unwrap(),
        [5, b'w', 0, 3, 0, b'a', 0, 0x3e, 0xd8, 0x80, 0xdd, 8]
    );
}

#[test]
fn eof_without_final_end_marker_is_accepted() {
    let d = Document::from_binary(&[1, b'k', 0, b'v', 0]).unwrap();
    assert!(!d.layout.end_marker);
    assert_eq!(d.roots, vec![Entry::string("k", "v")]);
    assert_eq!(d.to_binary().unwrap(), [1, b'k', 0, b'v', 0]);
}

#[test]
fn empty_input_and_lone_end_marker_are_empty() {
    assert!(Document::from_binary(&[]).unwrap().roots.is_empty());
    let lone = Document::from_binary(&[8]).unwrap();
    assert!(lone.roots.is_empty() && lone.layout.end_marker);
    assert_eq!(lone.to_binary().unwrap(), [8]);
}

fn bad(bytes: &[u8]) -> Error {
    Document::from_binary(bytes).expect_err("should fail")
}

#[test]
fn truncated_inputs_are_errors() {
    let full = [0, b'r', 0, 2, b'n', 0, 1, 0, 0, 0, 8, 8];
    for cut in 1..full.len() - 1 {
        assert!(
            Document::from_binary(&full[..cut]).is_err(),
            "cut at {cut} should fail"
        );
    }
}

#[test]
fn unsupported_type_bytes() {
    for ty in [9u8, 11, 12, 255] {
        assert_eq!(
            bad(&[1, b'a', 0, b'b', 0, ty, b'k', 0]),
            Error::UnsupportedType {
                type_byte: ty,
                offset: 5
            }
        );
    }
}

#[test]
fn int64_is_signed_little_endian() {
    let bytes = [
        10, b'k', 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 8,
    ];
    let d = Document::from_binary(&bytes).unwrap();
    assert_eq!(d.roots, vec![Entry::int64("k", -1)]);
    assert_eq!(d.to_binary().unwrap(), bytes);
}

#[test]
fn windows_1252_strings_are_preserved() {
    let bytes = [1, b'k', 0xe9, 0, b'c', b'a', b'f', 0xe9, 0x80, 0, 8];
    let d = Document::from_binary(&bytes).unwrap();
    assert_eq!(d.encoding, Encoding::Windows1252);
    assert_eq!(d.roots[0].key, "k\u{e9}");
    assert_eq!(d.to_binary().unwrap(), bytes);
}

#[test]
fn utf8_strings_stay_utf8() {
    let bytes = [1, b'k', 0, b'c', b'a', b'f', 0xc3, 0xa9, 0, 8];
    let d = Document::from_binary(&bytes).unwrap();
    assert_eq!(d.encoding, Encoding::Utf8);
    assert_eq!(d.to_binary().unwrap(), bytes);
}

#[test]
fn unterminated_key_and_string() {
    assert!(matches!(bad(&[1, b'k']), Error::MalformedBinary(_)));
    assert!(matches!(
        bad(&[1, b'k', 0, b'v']),
        Error::MalformedBinary(_)
    ));
}

#[test]
fn invalid_utf8_and_utf16() {
    assert!(matches!(
        bad(&[5, b'w', 0, 1, 0, 0x00, 0xd8]),
        Error::MalformedBinary(_)
    ));
}

#[test]
fn trailing_data_after_end_marker() {
    assert!(matches!(
        bad(&[8, 1, b'k', 0, 0]),
        Error::MalformedBinary(_)
    ));
}

#[test]
fn hostile_nesting_is_an_error() {
    let mut bytes = Vec::new();
    for _ in 0..200_000 {
        bytes.extend_from_slice(&[0, b'a', 0]);
    }
    assert_eq!(
        Document::from_binary(&bytes),
        Err(Error::TooDeep { limit: 128 })
    );
}

#[test]
fn writer_rejects_what_binary_cannot_hold() {
    let cond = doc(vec![Entry::string("k", "v").with_condition("$A")]);
    assert!(matches!(cond.to_binary(), Err(Error::InvalidInput(_))));

    let dir = Document::default().with_directive(Directive::base("x"));
    assert!(matches!(dir.to_binary(), Err(Error::InvalidInput(_))));

    let nul = doc(vec![Entry::string("k", "a\0b")]);
    assert!(matches!(nul.to_binary(), Err(Error::InvalidInput(_))));

    let long = doc(vec![Entry::new("w", Value::WString("a".repeat(70_000)))]);
    assert!(matches!(long.to_binary(), Err(Error::InvalidInput(_))));
}

#[test]
fn writing_too_deep_is_an_error() {
    let mut e = Entry::section("leaf", vec![]);
    for _ in 0..10 {
        e = Entry::section("n", vec![e]);
    }
    let opts = Options {
        max_depth: 5,
        ..Options::default()
    };
    assert_eq!(
        doc(vec![e]).to_binary_with(&opts),
        Err(Error::TooDeep { limit: 5 })
    );
}
