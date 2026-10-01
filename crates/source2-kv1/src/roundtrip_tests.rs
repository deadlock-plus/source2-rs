use crate::{Document, Options};

fn same(text: &str) {
    let doc = Document::parse(text).unwrap_or_else(|e| panic!("{e}: {text:?}"));
    assert_eq!(doc.to_text().unwrap(), text);
}

#[test]
fn comments_survive() {
    same("// head\n\"a\" // after key\n{ // after brace\n\tk v // tail\n\t// alone\n}\n// end\n");
}

#[test]
fn bare_and_quoted_spellings_survive() {
    same("a\n{\n\tk v\n\t\"k2\" v2\n\tk3 \"v3\"\n\t\"k4\"\t\t\"v4\"\n}\n");
}

#[test]
fn blank_lines_and_layout_survive() {
    same("a\r\n{\r\n\r\n\tk   v  \r\n\t\r\n\t\tj\t\"w\"\r\n}\r\n\r\n");
}

#[test]
fn directive_position_and_case_survive() {
    same("#Include \"a.vdf\"\nroot { }\n#BASE b.vdf\nother { }\n");
}

#[test]
fn no_trailing_newline_survives() {
    same("a { k v }");
}

#[test]
fn bom_survives() {
    same("\u{feff}a { k v }\n");
}

#[test]
fn condition_positions_survive() {
    same("a [$X] { k v [ $Y ] }\nb { } [$Z]\n");
}

#[test]
fn escape_spellings_survive() {
    same(r#"a { k "x\q\?\'\a\v" j "C:\new" }"#);
}

#[test]
fn escapes_off_file_round_trips() {
    let opts = Options {
        escape_sequences: false,
        ..Options::default()
    };
    let text = "a { k \"C:\new\\\" }";
    let doc = Document::parse_with(text, &opts).unwrap();
    assert_eq!(doc.to_text().unwrap(), text);
}

#[test]
fn typed_values_are_not_stringified_by_default() {
    use crate::{Entry, Value};
    let d = Document::new(vec![Entry::new("i", Value::Int(1))]);
    assert!(d.to_text().is_err());
}

#[test]
fn binary_end_marker_is_recorded() {
    let bytes = [1, b'k', 0, b'v', 0];
    let doc = Document::from_binary(&bytes).unwrap();
    assert_eq!(doc.to_binary().unwrap(), bytes);
}

#[test]
fn binary_windows_1252_strings_keep_their_bytes() {
    let bytes = [1, b'k', 0, b'c', b'a', b'f', 0xe9, 0, 8];
    let doc = Document::from_binary(&bytes).unwrap();
    assert_eq!(doc.to_binary().unwrap(), bytes);
}

#[test]
fn odd_inputs_survive() {
    for text in [
        "",
        "   ",
        "// only a comment",
        "\n\n\n",
        "a b // eof comment",
        "a b\r\n// crlf comment\r\nc d\r\n",
        "a b\nc d\r\ne f\n",
        "a\t{\t}\t// x\n",
        "\"#include\" \"x\" [$A] // c\n\"#BASE\" y\n",
        "#include x\n\n\n#base y\n",
        "a { b { } }\n\n\n",
        "a{b c}d e",
        "k \"multi\nline\" // c\n",
        "k v\u{a0}w\n",
        "a [ $X ]\t{ }\n",
    ] {
        same(text);
    }
}

#[test]
fn edits_on_a_parsed_document_write_cleanly() {
    let mut doc = Document::parse("// head\r\nroot\r\n{\r\n\ta 1 // one\r\n}\r\n").unwrap();
    doc.roots[0].push(crate::Entry::string("b", "2"));
    assert_eq!(
        doc.to_text().unwrap(),
        "// head\r\nroot\r\n{\r\n\ta 1 // one\r\n\t\"b\"\t\t\"2\"\r\n}\r\n"
    );
    doc.roots[0].get_mut("a").unwrap().value = crate::Value::String("x y".into());
    assert!(doc.to_text().unwrap().contains("a \"x y\" // one"));
}

#[test]
fn windows_1252_text_round_trips() {
    let bytes = b"k \"caf\xe9 \x93q\x94 \x81\"\r\n";
    let doc = Document::parse_bytes(bytes).unwrap();
    assert_eq!(doc.to_text_bytes().unwrap(), bytes);
}

#[test]
fn trailing_nul_survives() {
    same("\"a\"\n{\n\tk v\n}\n\0");
    same("a { k v }\0");
    same("a { k v }\n// end\n\0");
}

#[test]
fn trailing_nul_is_recorded() {
    let d = Document::parse("a { k v }\n\0").unwrap();
    assert!(d.layout.trailing_nul);
    assert!(!Document::parse("a { k v }\n").unwrap().layout.trailing_nul);
}

#[test]
fn embedded_nul_is_an_error() {
    assert!(Document::parse("a { k v }\0\0").is_err());
    assert!(Document::parse("a { k v }\0 b { k v }").is_err());
    assert!(Document::parse("a { k \0 }").is_err());
    assert!(Document::parse("\0a { k v }").is_err());
}
