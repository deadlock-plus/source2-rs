use crate::{Directive, Document, Entry, Error, Options, Quote, Value};

fn doc(roots: Vec<Entry>) -> Document {
    Document::new(roots)
}

fn s(k: &str, v: &str) -> Entry {
    Entry::string(k, v)
}

#[test]
fn valve_layout() {
    let d = doc(vec![Entry::section(
        "root",
        vec![
            s("a", "1"),
            Entry::section("sub", vec![s("b", "two words")]),
            Entry::section("empty", vec![]),
        ],
    )]);
    assert_eq!(
        d.to_text().unwrap(),
        "\"root\"\n{\n\t\"a\"\t\t\"1\"\n\t\"sub\"\n\t{\n\t\t\"b\"\t\t\"two words\"\n\t}\n\t\"empty\"\n\t{\n\t}\n}\n"
    );
}

#[test]
fn multiple_roots_and_directives() {
    let d = Document::new(vec![Entry::section("x", vec![]), s("y", "z")])
        .with_directive(Directive::base("a.vdf"))
        .with_directive(Directive::include("b.vdf"));
    assert_eq!(
        d.to_text().unwrap(),
        "#base \"a.vdf\"\n#include \"b.vdf\"\n\n\"x\"\n{\n}\n\"y\"\t\t\"z\"\n"
    );
}

#[test]
fn directive_after_roots() {
    let d =
        Document::new(vec![s("y", "z")]).with_directive(Directive::include("b.vdf").after_roots(1));
    assert_eq!(d.to_text().unwrap(), "\"y\"\t\t\"z\"\n#include \"b.vdf\"\n");
}

#[test]
fn conditions() {
    let d = doc(vec![Entry::section(
        "r",
        vec![
            s("k", "v").with_condition("$WIN32"),
            Entry::section("s", vec![]).with_condition("!$X360"),
        ],
    )]);
    assert_eq!(
        d.to_text().unwrap(),
        "\"r\"\n{\n\t\"k\"\t\t\"v\" [$WIN32]\n\t\"s\" [!$X360]\n\t{\n\t}\n}\n"
    );
}

#[test]
fn comments_in_hand_built_documents() {
    let d = Document::new(vec![Entry::section(
        "r",
        vec![
            s("a", "1")
                .with_comment(" one")
                .with_trailing_comment(" tail"),
            s("b", "2"),
        ],
    )])
    .with_comment(" header");
    assert_eq!(
        d.to_text().unwrap(),
        "// header\n\"r\"\n{\n\t// one\n\t\"a\"\t\t\"1\" // tail\n\t\"b\"\t\t\"2\"\n}\n"
    );
}

#[test]
fn bare_tokens_are_written_bare_when_possible() {
    let d = doc(vec![s("key", "value").bare(), s("two words", "x y").bare()]);
    assert_eq!(
        d.to_text().unwrap(),
        "key\t\tvalue\n\"two words\"\t\t\"x y\"\n"
    );
    assert_eq!(d.roots[0].layout.key.quote, Quote::Bare);
}

#[test]
fn escapes_are_written() {
    let d = doc(vec![s("k", "a\nb\tc\"d\\e\u{7}")]);
    assert_eq!(
        d.to_text().unwrap(),
        "\"k\"\t\t\"a\\nb\\tc\\\"d\\\\e\\a\"\n"
    );
}

#[test]
fn edited_values_do_not_reuse_stale_spelling() {
    let mut d = Document::parse(r#"k "a\?b""#).unwrap();
    assert_eq!(d.to_text().unwrap(), r#"k "a\?b""#);
    d.roots[0].value = Value::String("zzz".into());
    assert_eq!(d.to_text().unwrap(), "k \"zzz\"");
}

#[test]
fn round_trips_awkward_strings() {
    let awkward = [
        "",
        " ",
        "a\nb",
        "tab\there",
        "q\"uote",
        "back\\slash",
        "C:\\games\\q",
        "ends\\",
        "//not a comment",
        "{}",
        "[$WIN32]",
        "caf\u{e9} \u{1f980}",
        "\\n literal",
        "\r\u{b}\u{8}\u{c}\u{7}",
    ];
    for v in awkward {
        let d = doc(vec![Entry::section(
            v,
            vec![s(v, v), s("x", v).with_condition("$A")],
        )]);
        let back = Document::parse(&d.to_text().unwrap()).unwrap();
        assert_eq!(back.without_layout(), d, "value {v:?}");
    }
}

#[test]
fn round_trips_with_escapes_off() {
    let opts = Options {
        escape_sequences: false,
        ..Options::default()
    };
    let mut d = doc(vec![s("k", "C:\\games\\q\\"), s("m", "two\nlines")]);
    d.escapes = false;
    let text = d.to_text().unwrap();
    assert_eq!(
        text,
        "\"k\"\t\t\"C:\\games\\q\\\"\n\"m\"\t\t\"two\nlines\"\n"
    );
    assert_eq!(
        Document::parse_with(&text, &opts).unwrap().without_layout(),
        d
    );
}

#[test]
fn escapes_off_cannot_hold_a_quote() {
    let mut d = doc(vec![s("k", "a\"b")]);
    d.escapes = false;
    assert!(matches!(d.to_text(), Err(Error::InvalidInput(_))));
}

#[test]
fn bad_conditions_are_rejected() {
    for bad in ["a]b", "a\nb"] {
        let d = doc(vec![s("k", "v").with_condition(bad)]);
        assert!(
            matches!(d.to_text(), Err(Error::InvalidInput(_))),
            "{bad:?}"
        );
    }
}

#[test]
fn typed_values_are_an_error_not_a_silent_string() {
    let typed = [
        Entry::int("i", -5),
        Entry::float("f", 1.5),
        Entry::ptr("p", 7),
        Entry::wstring("w", "wide"),
        Entry::color("c", [1, 2, 3, 4]),
        Entry::uint64("u", u64::MAX),
        Entry::int64("l", -1),
    ];
    for e in typed {
        let d = doc(vec![Entry::section("r", vec![e.clone()])]);
        assert!(
            matches!(d.to_text(), Err(Error::TypedValueInText { .. })),
            "{e:?}"
        );
    }
}

#[test]
fn stringified_is_the_explicit_conversion() {
    let d = doc(vec![Entry::section(
        "r",
        vec![
            Entry::int("i", -5),
            Entry::float("f", 1.5),
            Entry::ptr("p", 7),
            Entry::wstring("w", "wide"),
            Entry::color("c", [1, 2, 3, 4]),
            Entry::uint64("u", u64::MAX),
            Entry::int64("l", -1),
        ],
    )]);
    let back = Document::parse(&d.stringified().to_text().unwrap()).unwrap();
    let r = &back.roots[0];
    assert_eq!(r.get_str("i"), Some("-5"));
    assert_eq!(r.get_str("f"), Some("1.5"));
    assert_eq!(r.get_str("p"), Some("7"));
    assert_eq!(r.get_str("w"), Some("wide"));
    assert_eq!(r.get_str("c"), Some("1 2 3 4"));
    assert_eq!(r.get_str("u"), Some("18446744073709551615"));
    assert_eq!(r.get_str("l"), Some("-1"));
    assert!(matches!(d.roots[0].children()[0].value, Value::Int(-5)));
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
        doc(vec![e]).to_text_with(&opts),
        Err(Error::TooDeep { limit: 5 })
    );
}

#[test]
fn windows_1252_text_is_written_as_bytes() {
    let d = Document::parse_bytes(b"k caf\xe9\r\n").unwrap();
    assert_eq!(d.to_text_bytes().unwrap(), b"k caf\xe9\r\n");
    let mut bad = d.clone();
    bad.roots[0].value = Value::String("\u{1f980}".into());
    assert!(matches!(bad.to_text_bytes(), Err(Error::InvalidInput(_))));
}
