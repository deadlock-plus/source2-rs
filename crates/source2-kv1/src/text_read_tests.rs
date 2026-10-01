use crate::{Directive, DirectiveKind, Document, Entry, Error, Options, Value};

fn parse(text: &str) -> Document {
    Document::parse(text).unwrap_or_else(|e| panic!("{e}: {text:?}"))
}

fn roots(text: &str) -> Vec<Entry> {
    parse(text).roots
}

fn s(k: &str, v: &str) -> Entry {
    Entry::string(k, v)
}

#[test]
fn quoted_section() {
    let got = roots("\"root\"\n{\n\t\"a\"\t\t\"1\"\n\t\"b\" \"two words\"\n}\n");
    assert_eq!(
        got,
        vec![Entry::section(
            "root",
            vec![s("a", "1"), s("b", "two words")]
        )]
    );
}

#[test]
fn unquoted_tokens() {
    let got = roots("GameInfo\n{\n\tgame citadel\n\tnomodels 1\n}");
    assert_eq!(
        got,
        vec![Entry::section(
            "GameInfo",
            vec![s("game", "citadel"), s("nomodels", "1")]
        )]
    );
}

#[test]
fn brace_on_same_line_and_compact() {
    let got = roots("a { b c }\nd{e f}");
    assert_eq!(
        got,
        vec![
            Entry::section("a", vec![s("b", "c")]),
            Entry::section("d", vec![s("e", "f")]),
        ]
    );
}

#[test]
fn nested_and_empty_sections() {
    let got = roots("a { b { c d } e { } }");
    assert_eq!(
        got,
        vec![Entry::section(
            "a",
            vec![
                Entry::section("b", vec![s("c", "d")]),
                Entry::section("e", vec![]),
            ]
        )]
    );
}

#[test]
fn duplicate_keys_keep_order() {
    let got = roots("a { k 1 j 2 k 3 } a { k 4 }");
    assert_eq!(
        got,
        vec![
            Entry::section("a", vec![s("k", "1"), s("j", "2"), s("k", "3")]),
            Entry::section("a", vec![s("k", "4")]),
        ]
    );
}

#[test]
fn multiple_roots_including_plain_pairs() {
    let got = roots("one { } \"two\" \"x\" three { }");
    assert_eq!(
        got,
        vec![
            Entry::section("one", vec![]),
            s("two", "x"),
            Entry::section("three", vec![]),
        ]
    );
}

#[test]
fn comments_are_dropped() {
    let got =
        roots("// head\na // after key\n{ // after brace\n\tk v // tail\n\t// alone\n}\n// end");
    assert_eq!(got, vec![Entry::section("a", vec![s("k", "v")])]);
}

#[test]
fn comment_markers_survive_inside_quotes_and_urls() {
    let got = roots("a { \"k\" \"http://x // y\" u http://z }");
    assert_eq!(
        got,
        vec![Entry::section(
            "a",
            vec![s("k", "http://x // y"), s("u", "http://z")]
        )]
    );
}

#[test]
fn escapes_decode_by_default() {
    let got = roots(r#"a { k "l1\nl2\tq\"\\z" }"#);
    assert_eq!(
        got,
        vec![Entry::section("a", vec![s("k", "l1\nl2\tq\"\\z")])]
    );
}

#[test]
fn unknown_escape_is_kept_verbatim() {
    let got = roots(r#"a { k "C:\games\q" }"#);
    assert_eq!(got, vec![Entry::section("a", vec![s("k", "C:\\games\\q")])]);
}

#[test]
fn escapes_off_keeps_backslashes_and_ends_at_quote() {
    let opts = Options {
        escape_sequences: false,
        ..Options::default()
    };
    let doc = Document::parse_with(r#"a { k "x\n\" y "C:\" }"#, &opts).unwrap();
    assert_eq!(
        doc.roots,
        vec![Entry::section("a", vec![s("k", "x\\n\\"), s("y", "C:\\")])]
    );
}

#[test]
fn quoted_strings_may_span_lines() {
    let got = roots("a { k \"one\ntwo\" }");
    assert_eq!(got, vec![Entry::section("a", vec![s("k", "one\ntwo")])]);
}

#[test]
fn empty_key_and_value() {
    let got = roots("a { \"\" \"\" }");
    assert_eq!(got, vec![Entry::section("a", vec![s("", "")])]);
}

#[test]
fn bom_and_crlf() {
    let got = roots("\u{feff}a\r\n{\r\n\tk v\r\n}\r\n");
    assert_eq!(got, vec![Entry::section("a", vec![s("k", "v")])]);
}

#[test]
fn condition_after_value() {
    let got = roots("a { k v [$WIN32] j \"w\" [!$X360] }");
    assert_eq!(
        got,
        vec![Entry::section(
            "a",
            vec![
                s("k", "v").with_condition("$WIN32"),
                s("j", "w").with_condition("!$X360"),
            ]
        )]
    );
}

#[test]
fn condition_on_sections_after_key_or_after_brace() {
    let got = roots("a [$WIN32] { } b { } [$POSIX]");
    assert_eq!(
        got,
        vec![
            Entry::section("a", vec![]).with_condition("$WIN32"),
            Entry::section("b", vec![]).with_condition("$POSIX"),
        ]
    );
}

#[test]
fn condition_with_operators_and_spaces() {
    let got = roots("k v [$WIN32 || $POSIX]");
    assert_eq!(got, vec![s("k", "v").with_condition("$WIN32 || $POSIX")]);
}

#[test]
fn quoted_bracket_text_is_a_value() {
    let got = roots("k \"[$WIN32]\"");
    assert_eq!(got, vec![s("k", "[$WIN32]")]);
}

#[test]
fn directives_are_surfaced_not_followed() {
    let doc =
        parse("#base \"a.vdf\"\n#include b.vdf\n\"#base\" \"c.vdf\"\n#INCLUDE \"d.vdf\"\nroot { }");
    assert_eq!(
        doc.directives,
        vec![
            Directive {
                kind: DirectiveKind::Base,
                path: "a.vdf".into()
            },
            Directive {
                kind: DirectiveKind::Include,
                path: "b.vdf".into()
            },
            Directive {
                kind: DirectiveKind::Base,
                path: "c.vdf".into()
            },
            Directive {
                kind: DirectiveKind::Include,
                path: "d.vdf".into()
            },
        ]
    );
    assert_eq!(doc.roots, vec![Entry::section("root", vec![])]);
}

#[test]
fn directive_name_inside_a_section_is_an_ordinary_key() {
    let doc = parse("a { #include x }");
    assert!(doc.directives.is_empty());
    assert_eq!(
        doc.roots,
        vec![Entry::section("a", vec![s("#include", "x")])]
    );
}

#[test]
fn empty_input_is_empty_document() {
    assert_eq!(parse(" \n// nothing\n"), Document::default());
}

#[test]
fn value_is_string_variant() {
    let got = roots("k v");
    assert!(matches!(got[0].value, Value::String(_)));
}

fn err(text: &str) -> Error {
    Document::parse(text).expect_err(text)
}

#[test]
fn unterminated_quote_reports_position() {
    assert_eq!(
        err("a {\n  k \"never closed }"),
        Error::UnterminatedString { line: 2, column: 5 }
    );
}

#[test]
fn unterminated_brace() {
    assert!(matches!(
        err("a { k v"),
        Error::UnexpectedEof {
            expected: "'}'",
            ..
        }
    ));
}

#[test]
fn stray_close_brace_at_top_level() {
    assert!(matches!(
        err("a { } }"),
        Error::UnexpectedToken {
            line: 1,
            column: 7,
            ..
        }
    ));
}

#[test]
fn stray_open_brace_at_top_level() {
    assert!(matches!(err("{ a b }"), Error::UnexpectedToken { .. }));
}

#[test]
fn stray_open_brace_where_key_expected() {
    assert!(matches!(err("a { { } }"), Error::UnexpectedToken { .. }));
}

#[test]
fn key_without_value_at_eof() {
    assert!(matches!(err("a"), Error::UnexpectedEof { .. }));
}

#[test]
fn key_followed_by_close_brace() {
    assert!(matches!(err("a { k }"), Error::UnexpectedToken { .. }));
}

#[test]
fn stray_condition_where_key_expected() {
    assert!(matches!(err("[$WIN32] a b"), Error::UnexpectedToken { .. }));
    assert!(matches!(
        err("a { k v [$A] [$B] }"),
        Error::UnexpectedToken { .. }
    ));
}

#[test]
fn condition_with_no_value() {
    assert!(matches!(err("a { k [$A] }"), Error::UnexpectedToken { .. }));
}

#[test]
fn unterminated_condition() {
    assert_eq!(
        err("k v [$WIN32\n"),
        Error::UnterminatedCondition { line: 1, column: 5 }
    );
}

#[test]
fn directive_without_path() {
    assert!(matches!(err("#base"), Error::UnexpectedEof { .. }));
    assert!(matches!(err("#base {"), Error::UnexpectedToken { .. }));
}

#[test]
fn hostile_nesting_is_an_error_not_a_stack_overflow() {
    let text = "a {".repeat(200_000);
    assert_eq!(Document::parse(&text), Err(Error::TooDeep { limit: 128 }));
}

#[test]
fn depth_limit_boundary() {
    let opts = Options {
        max_depth: 3,
        ..Options::default()
    };
    assert!(Document::parse_with("a { b { c { } } }", &opts).is_ok());
    assert_eq!(
        Document::parse_with("a { b { c { d { } } } }", &opts),
        Err(Error::TooDeep { limit: 3 })
    );
}
