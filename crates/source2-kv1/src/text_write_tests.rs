use crate::{Directive, DirectiveKind, Document, Entry, Error, Options, Value};

fn doc(roots: Vec<Entry>) -> Document {
    Document {
        directives: vec![],
        roots,
    }
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
    let d = Document {
        directives: vec![
            Directive {
                kind: DirectiveKind::Base,
                path: "a.vdf".into(),
            },
            Directive {
                kind: DirectiveKind::Include,
                path: "b.vdf".into(),
            },
        ],
        roots: vec![Entry::section("x", vec![]), s("y", "z")],
    };
    assert_eq!(
        d.to_text().unwrap(),
        "#base \"a.vdf\"\n#include \"b.vdf\"\n\n\"x\"\n{\n}\n\"y\"\t\t\"z\"\n"
    );
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
fn escapes_are_written() {
    let d = doc(vec![s("k", "a\nb\tc\"d\\e")]);
    assert_eq!(d.to_text().unwrap(), "\"k\"\t\t\"a\\nb\\tc\\\"d\\\\e\"\n");
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
    ];
    for v in awkward {
        let d = doc(vec![Entry::section(
            v,
            vec![s(v, v), s("x", v).with_condition("$A")],
        )]);
        let back = Document::parse(&d.to_text().unwrap()).unwrap();
        assert_eq!(back, d, "value {v:?}");
    }
}

#[test]
fn round_trips_with_escapes_off() {
    let opts = Options {
        escape_sequences: false,
        ..Options::default()
    };
    let d = doc(vec![s("k", "C:\\games\\q\\"), s("m", "two\nlines")]);
    let text = d.to_text_with(&opts).unwrap();
    assert_eq!(
        text,
        "\"k\"\t\t\"C:\\games\\q\\\"\n\"m\"\t\t\"two\nlines\"\n"
    );
    assert_eq!(Document::parse_with(&text, &opts).unwrap(), d);
}

#[test]
fn escapes_off_cannot_hold_a_quote() {
    let opts = Options {
        escape_sequences: false,
        ..Options::default()
    };
    let d = doc(vec![s("k", "a\"b")]);
    assert!(matches!(d.to_text_with(&opts), Err(Error::InvalidInput(_))));
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
fn typed_values_write_as_their_string_form() {
    let d = doc(vec![Entry::section(
        "r",
        vec![
            Entry::new("i", Value::Int(-5)),
            Entry::new("f", Value::Float(1.5)),
            Entry::new("p", Value::Ptr(7)),
            Entry::new("w", Value::WString("wide".into())),
            Entry::new("c", Value::Color([1, 2, 3, 4])),
            Entry::new("u", Value::UInt64(u64::MAX)),
        ],
    )]);
    let text = d.to_text().unwrap();
    let back = Document::parse(&text).unwrap();
    let r = &back.roots[0];
    assert_eq!(r.get_str("i"), Some("-5"));
    assert_eq!(r.get_str("f"), Some("1.5"));
    assert_eq!(r.get_str("p"), Some("7"));
    assert_eq!(r.get_str("w"), Some("wide"));
    assert_eq!(r.get_str("c"), Some("1 2 3 4"));
    assert_eq!(r.get_str("u"), Some("18446744073709551615"));
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
