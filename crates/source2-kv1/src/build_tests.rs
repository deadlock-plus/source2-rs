use crate::{Directive, DirectiveKind, Document, Entry, Value};

#[test]
fn constructors_build_the_matching_value() {
    assert_eq!(Entry::int("k", 1).value, Value::Int(1));
    assert_eq!(Entry::int64("k", -1).value, Value::Int64(-1));
    assert_eq!(Entry::uint64("k", 1).value, Value::UInt64(1));
    assert_eq!(Entry::float("k", 0.5).value, Value::Float(0.5));
    assert_eq!(Entry::ptr("k", 3).value, Value::Ptr(3));
    assert_eq!(Entry::wstring("k", "w").value, Value::WString("w".into()));
    assert_eq!(
        Entry::color("k", [1, 2, 3, 4]).value,
        Value::Color([1, 2, 3, 4])
    );
    assert_eq!(Entry::bool("k", true).value, Value::String("1".into()));
    assert_eq!(Entry::bool("k", false).value, Value::String("0".into()));
}

#[test]
fn from_impls_feed_entry_new() {
    assert_eq!(Entry::new("k", "v").value, Value::String("v".into()));
    assert_eq!(
        Entry::new("k", String::from("v")).value,
        Value::String("v".into())
    );
    assert_eq!(Entry::new("k", 5_i32).value, Value::Int(5));
    assert_eq!(Entry::new("k", 5_i64).value, Value::Int64(5));
    assert_eq!(Entry::new("k", 5_u64).value, Value::UInt64(5));
    assert_eq!(Entry::new("k", 0.5_f32).value, Value::Float(0.5));
    assert_eq!(
        Entry::new("k", [1_u8, 2, 3, 4]).value,
        Value::Color([1, 2, 3, 4])
    );
    assert_eq!(
        Entry::new("k", vec![Entry::string("a", "b")])
            .children()
            .len(),
        1
    );
}

#[test]
fn builders_chain() {
    let e = Entry::section("s", vec![])
        .with(Entry::string("a", "1"))
        .with(Entry::string("b", "2"));
    assert_eq!(e.children().len(), 2);
    let mut e = e;
    e.push(Entry::string("c", "3"));
    e.get_mut("A").unwrap().value = Value::String("9".into());
    assert_eq!(e.get_str("a"), Some("9"));
    assert_eq!(e.children().len(), 3);
}

#[test]
fn push_on_a_non_section_does_nothing() {
    let mut e = Entry::string("k", "v");
    e.push(Entry::string("a", "b"));
    assert!(e.children().is_empty());
}

#[test]
fn document_new_defaults() {
    let d = Document::new(vec![Entry::string("k", "v")]);
    assert!(d.directives.is_empty());
    assert!(d.escapes);
    assert_eq!(d, Document::new(vec![Entry::string("k", "v")]));
}

#[test]
fn directive_builders() {
    let d = Directive::include("x").after_roots(2).with_condition("$A");
    assert_eq!(d.kind, DirectiveKind::Include);
    assert_eq!((d.before_root, d.condition.as_deref()), (2, Some("$A")));
    assert_eq!(Directive::base("y").kind, DirectiveKind::Base);
}

#[test]
fn hand_built_documents_round_trip_through_text() {
    let d = Document::new(vec![Entry::section(
        "r",
        vec![
            Entry::string("a", "1").with_comment(" c"),
            Entry::section("s", vec![Entry::bool("b", true)]).with_trailing_comment(" t"),
        ],
    )])
    .with_directive(Directive::include("x.vdf"))
    .with_comment(" top");
    let text = d.to_text().unwrap();
    let back = Document::parse(&text).unwrap();
    assert_eq!(back.to_text().unwrap(), text);
    assert_eq!(back.without_layout(), d.without_layout());
}
