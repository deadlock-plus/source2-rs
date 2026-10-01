//! Text round trips: what the header keeps, and what text can and cannot hold.

use crate::guid::{format_guid, parse_guid};
use crate::{
    Compression, Document, Error, GENERIC_FORMAT, Kind, Object, Tag, TextHeader, Value, Version,
    WriteOptions, flag, parse, parse_text, write_text,
};

const CUSTOM_HEADER: &str = "<!-- kv3 encoding:text:version{e21c7f3c-8a33-41c5-9977-a76d3a32aa0d} format:layout2:version{26288658-411e-4f14-b698-2e1e5d00dec6} -->";

fn root(members: Vec<(&str, Value)>) -> Value {
    Value::from(Object::from(members))
}

fn text_of(v: Value) -> String {
    Document::new(v).to_text().expect("write text")
}

fn round(v: &Value) -> Value {
    parse_text(&text_of(v.clone())).expect("parse text").root
}

#[test]
fn the_header_guids_and_names_survive_a_text_round_trip() {
    let doc = parse_text(&format!("{CUSTOM_HEADER}\n{{ a = 1 }}")).unwrap();
    assert_eq!(doc.text.format_name, "layout2");
    assert_eq!(doc.text.encoding.name, "text");
    assert_eq!(
        format_guid(&doc.options.format),
        "26288658-411e-4f14-b698-2e1e5d00dec6"
    );

    let again = doc.to_text().unwrap();
    assert!(again.starts_with(CUSTOM_HEADER), "{again}");
    let back = parse_text(&again).unwrap();
    assert_eq!(back.options.format, doc.options.format);
    assert_eq!(back.text, doc.text);
}

#[test]
fn a_text_format_guid_reaches_the_binary_header_and_back() {
    let doc = parse_text(&format!("{CUSTOM_HEADER}\n{{ a = 1 }}")).unwrap();
    let block = doc.to_bytes().unwrap();
    assert_eq!(
        crate::Header::parse(&block).unwrap().format,
        doc.options.format
    );
    let from_binary = parse(&block).unwrap();
    assert_eq!(from_binary.options.format, doc.options.format);
}

#[test]
fn a_non_text_encoding_name_and_guid_are_kept() {
    let input = "<!-- kv3 encoding:other:version{00112233-4455-6677-8899-aabbccddeeff} format:generic:version{7412167c-06e9-4698-aff2-e63eb59037e7} -->\n{}";
    let doc = parse_text(input).unwrap();
    assert_eq!(doc.text.encoding.name, "other");
    assert_eq!(
        doc.text.encoding.guid_string(),
        "00112233-4455-6677-8899-aabbccddeeff"
    );
    assert!(
        doc.to_text()
            .unwrap()
            .contains("encoding:other:version{00112233-4455-6677-8899-aabbccddeeff}")
    );
}

#[test]
fn a_document_without_a_header_is_the_generic_format_and_gains_one() {
    let doc = parse_text("{ a = 1 }").unwrap();
    assert_eq!(doc.options.format, GENERIC_FORMAT);
    assert_eq!(doc.text, TextHeader::default());
    let text = doc.to_text().unwrap();
    assert!(
        text.starts_with("<!-- kv3 encoding:text:version{"),
        "{text}"
    );
    assert_eq!(parse_text(&text).unwrap().root, doc.root);
}

#[test]
fn a_binary_document_of_another_format_is_written_as_text_under_a_placeholder_name() {
    let mut doc = Document::new(root(vec![("a", Value::int(1))]));
    doc.options.format = [9; 16];
    doc.options.compression = Compression::None;
    let from_binary = parse(&doc.to_bytes().unwrap()).unwrap();
    assert_eq!(from_binary.text.format_name, "");
    let text = from_binary.to_text().unwrap();
    assert!(
        text.contains("format:unnamed:version{09090909-0909-0909-0909-090909090909}"),
        "{text}"
    );
    assert_eq!(parse_text(&text).unwrap().options.format, [9; 16]);
}

#[test]
fn a_binary_document_of_the_generic_format_is_named_generic() {
    let doc = Document::new(root(vec![("a", Value::int(1))]));
    let from_binary = parse(&doc.to_bytes().unwrap()).unwrap();
    assert_eq!(from_binary.text.format_name, "generic");
    assert!(
        from_binary
            .to_text()
            .unwrap()
            .contains("format:generic:version{7412167c-")
    );
}

#[test]
fn guids_round_trip_between_text_and_bytes() {
    assert_eq!(
        parse_guid("7412167c-06e9-4698-aff2-e63eb59037e7"),
        Some(GENERIC_FORMAT)
    );
    assert_eq!(
        format_guid(&GENERIC_FORMAT),
        "7412167c-06e9-4698-aff2-e63eb59037e7"
    );
    for bad in [
        "",
        "7412167c06e946 98aff2e63eb59037e7",
        "zz12167c-06e9-4698-aff2-e63eb59037e7",
        "7412167c-06e9-4698-aff2-e63eb59037e7-00",
        "7412167c-06e9-4698-aff2",
    ] {
        assert_eq!(parse_guid(bad), None, "{bad:?}");
    }
}

#[test]
fn a_malformed_header_guid_is_a_syntax_error() {
    let input = "<!-- kv3 encoding:text:version{nope} format:generic:version{7412167c-06e9-4698-aff2-e63eb59037e7} -->\n{}";
    let err = parse_text(input).unwrap_err();
    assert!(matches!(err, Error::Syntax { line: 1, .. }), "{err}");
}

#[test]
fn unsigned_integers_stay_unsigned_through_text() {
    let tree = root(vec![
        ("small", Value::uint(5)),
        ("big", Value::uint(u64::MAX)),
        ("signed", Value::int(5)),
        ("neg", Value::int(-5)),
    ]);
    let text = text_of(tree.clone());
    assert!(text.contains("small = 0x5"), "{text}");
    assert!(text.contains("signed = 5"), "{text}");
    let back = round(&tree);
    assert_eq!(back, tree);
    assert_eq!(back.get("small").unwrap().kind(), &Kind::UInt(5));
    assert_eq!(back.get("signed").unwrap().kind(), &Kind::Int(5));
}

#[test]
fn multiline_strings_keep_their_form() {
    let tree = root(vec![
        ("plain", Value::from("one line")),
        ("block", Value::multiline("line one\nline two")),
    ]);
    let text = text_of(tree.clone());
    assert!(
        text.contains("block = \"\"\"\nline one\nline two\n\"\"\""),
        "{text}"
    );
    assert!(text.contains("plain = \"one line\""), "{text}");
    let back = round(&tree);
    assert_eq!(back, tree);
    assert!(back.get("block").unwrap().is_multiline());
    assert!(!back.get("plain").unwrap().is_multiline());
    assert_eq!(text_of(back), text);
}

#[test]
fn multiline_strings_are_raw_so_backslashes_survive() {
    let tree = root(vec![("p", Value::multiline("C:\\dir\\file\n\\n stays"))]);
    assert_eq!(round(&tree), tree);
}

#[test]
fn multiline_edge_contents_survive() {
    for text in [
        "",
        "\n",
        "\n\n",
        "a\n",
        "\nb",
        "\r\nx",
        "x\r\ny",
        "tab\there",
        "\"quoted\"",
        "\"\"two",
        "end\"",
    ] {
        let tree = root(vec![("s", Value::multiline(text))]);
        let back = round(&tree);
        assert_eq!(
            back.get("s").and_then(Value::as_str),
            Some(text),
            "{text:?}"
        );
    }
}

#[test]
fn a_multiline_string_that_cannot_sit_raw_is_written_quoted_and_still_round_trips() {
    for text in ["has \"\"\" inside", "ends with cr\r", "\"\"\""] {
        let tree = root(vec![("s", Value::multiline(text))]);
        let written = text_of(tree.clone());
        assert!(!written.contains("s = \"\"\"\n"), "{written}");
        let back = round(&tree);
        assert_eq!(
            back.get("s").and_then(Value::as_str),
            Some(text),
            "{text:?}"
        );
    }
}

#[test]
fn a_multiline_value_keeps_its_flags() {
    let tree = root(vec![(
        "s",
        Value::multiline("a\nb").with_flags(flag::RESOURCE),
    )]);
    let back = round(&tree);
    assert_eq!(back.get("s").unwrap().flags(), flag::RESOURCE);
    assert!(back.get("s").unwrap().is_multiline());
}

#[test]
fn a_flag_bit_without_a_text_spelling_is_an_error_not_a_silent_drop() {
    for bits in [0x20, 0x40, 0x80, flag::RESOURCE | 0x20] {
        let tree = root(vec![("a", Value::from("x").with_flags(bits))]);
        let err = Document::new(tree).to_text().unwrap_err();
        assert!(matches!(err, Error::Invalid(_)), "{bits:#x}: {err}");
    }
}

#[test]
fn every_spelled_flag_combination_round_trips() {
    for bits in 0..=flag::SPELLED {
        let tree = root(vec![("a", Value::from("x").with_flags(bits))]);
        assert_eq!(round(&tree), tree, "{bits:#x}");
    }
}

#[test]
fn an_unknown_flag_prefix_is_a_syntax_error() {
    let err = parse_text("{ a = bogus:\"x\" }").unwrap_err();
    assert!(matches!(err, Error::Syntax { line: 1, .. }), "{err}");
}

#[test]
fn non_finite_doubles_are_invalid_values_for_the_text_writer() {
    for d in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let tree = root(vec![("x", Value::double(d))]);
        let err = Document::new(tree).to_text().unwrap_err();
        assert!(matches!(err, Error::Invalid(_)), "{d}: {err}");
    }
}

#[test]
fn nan_and_infinity_have_no_text_spelling_to_read_either() {
    for word in ["nan", "NaN", "inf", "-inf", "infinity", "1e999"] {
        assert!(parse_text(&format!("{{ x = {word} }}")).is_err(), "{word}");
    }
}

#[test]
fn comments_are_read_past_and_not_kept() {
    let input = "// leading\n{\n\ta = 1 // trailing\n\t/* block */ b = 2\n}\n";
    let doc = parse_text(input).unwrap();
    assert_eq!(doc.root.as_object().unwrap().len(), 2);
    let text = doc.to_text().unwrap();
    assert!(!text.contains("//") && !text.contains("/*"), "{text}");
}

#[test]
fn syntax_errors_say_which_line() {
    let err = parse_text("{\n\ta = 1\n\tb = @\n}").unwrap_err();
    assert!(matches!(err, Error::Syntax { line: 3, .. }), "{err}");
    assert!(err.to_string().contains("line 3"), "{err}");
}

#[test]
fn binary_storage_hints_are_the_only_thing_text_drops_from_a_value() {
    let tree = root(vec![
        (
            "byte",
            Value::stored(
                Kind::Int(5),
                0,
                crate::value::Storage::Scalar(crate::node::INT32_AS_BYTE),
            ),
        ),
        ("f", Value::float(0.5)),
        ("blob", Value::blob(vec![0, 255])),
        ("null", Value::null()),
        ("list", Value::array(vec![Value::int(1), Value::from("s")])),
        ("empty", Value::array(vec![])),
        ("nested", root(vec![("k", Value::from(true))])),
        ("dup", Value::int(1)),
        ("dup", Value::int(2)),
        ("quoted key!", Value::int(3)),
    ]);
    assert_eq!(round(&tree), tree);
}

#[test]
fn free_functions_and_methods_agree() {
    let doc = Document::new(root(vec![("a", Value::int(1))]));
    assert_eq!(write_text(&doc).unwrap(), doc.to_text().unwrap());
    assert_eq!(
        Document::from_text(&doc.to_text().unwrap()).unwrap().root,
        doc.root
    );
}

#[test]
fn text_documents_carry_default_binary_options_apart_from_the_format() {
    let doc = parse_text(&format!("{CUSTOM_HEADER}\n{{}}")).unwrap();
    let defaults = WriteOptions::default();
    assert_eq!(doc.options.version, defaults.version);
    assert_eq!(doc.options.compression, defaults.compression);
    assert_ne!(doc.options.format, defaults.format);
    assert_eq!(doc.options.version, Version::V5);
}

#[test]
fn a_tag_spells_its_guid_in_text_form() {
    let tag = Tag::text_encoding();
    assert_eq!(tag.name, "text");
    assert_eq!(tag.guid_string(), "e21c7f3c-8a33-41c5-9977-a76d3a32aa0d");
}
