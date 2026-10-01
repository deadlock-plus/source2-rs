use crate::{Document, Entry, Value};

fn sample() -> Document {
    Document::parse(
        "Root { Name Bob count 12 big -3 ratio 0.5 on 1 off 0 yes TRUE no false dup a dup b sub { x y } }",
    )
    .unwrap()
}

#[test]
fn lookup_is_case_insensitive_and_first_wins() {
    let d = sample();
    let root = d.get("root").unwrap();
    assert_eq!(root.get_str("NAME"), Some("Bob"));
    assert_eq!(root.get_str("dup"), Some("a"));
    assert_eq!(
        root.get_all("DUP")
            .filter_map(Entry::as_str)
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert!(root.get("missing").is_none());
    assert!(d.get("nope").is_none());
}

#[test]
fn typed_getters() {
    let d = sample();
    let root = d.get("Root").unwrap();
    assert_eq!(root.get_int("count"), Some(12));
    assert_eq!(root.get_int("big"), Some(-3));
    assert_eq!(root.get_int("Name"), None);
    assert_eq!(root.get_float("ratio"), Some(0.5));
    assert_eq!(root.get_float("count"), Some(12.0));
    assert_eq!(root.get_bool("on"), Some(true));
    assert_eq!(root.get_bool("off"), Some(false));
    assert_eq!(root.get_bool("yes"), Some(true));
    assert_eq!(root.get_bool("no"), Some(false));
    assert_eq!(root.get_bool("Name"), None);
}

#[test]
fn sections_and_strings_are_told_apart() {
    let d = sample();
    let root = d.get("Root").unwrap();
    assert_eq!(root.get("sub").unwrap().children().len(), 1);
    assert_eq!(root.get("sub").unwrap().as_str(), None);
    assert!(root.get("Name").unwrap().children().is_empty());
    assert_eq!(root.get_str("sub"), None);
}

#[test]
fn typed_binary_values_feed_the_getters() {
    let e = Entry::section(
        "r",
        vec![
            Entry::new("i", Value::Int(7)),
            Entry::new("u", Value::UInt64(9)),
            Entry::new("f", Value::Float(2.5)),
        ],
    );
    assert_eq!(e.get_int("i"), Some(7));
    assert_eq!(e.get_int("u"), Some(9));
    assert_eq!(e.get_float("f"), Some(2.5));
    assert_eq!(e.get_bool("i"), Some(true));
}

#[test]
fn document_get_mut_edits_a_root_in_place() {
    let mut d = Document::parse("Root { a 1 }\nOther { b 2 }").unwrap();
    d.get_mut("ROOT").unwrap().push(Entry::string("c", "3"));
    assert_eq!(d.get("Root").unwrap().get_str("c"), Some("3"));
    assert!(d.get_mut("missing").is_none());
}
