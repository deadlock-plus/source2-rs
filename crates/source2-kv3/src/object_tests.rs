//! `Object`: lookup, editing and the index staying consistent through it.

use crate::{Object, Value};

fn abc() -> Object {
    Object::from(vec![
        ("a", Value::int(1)),
        ("b", Value::int(2)),
        ("c", Value::int(3)),
    ])
}

fn keys(o: &Object) -> Vec<&str> {
    o.keys().collect()
}

#[test]
fn insert_appends_new_keys_and_replaces_existing_ones() {
    let mut o = Object::new();
    assert_eq!(o.insert("a", 1), None);
    assert_eq!(o.insert("b", 2), None);
    assert_eq!(o.insert("a", 10), Some(Value::int(1)));
    assert_eq!(keys(&o), ["a", "b"]);
    assert_eq!(o.get("a").and_then(Value::as_i64), Some(10));
}

#[test]
fn insert_replaces_only_the_first_of_a_repeated_key() {
    let mut o = Object::new();
    o.push("k", 1);
    o.push("k", 2);
    o.insert("k", 9);
    let all: Vec<_> = o.get_all("k").filter_map(Value::as_i64).collect();
    assert_eq!(all, [9, 2]);
}

#[test]
fn push_keeps_repeated_keys_and_lookups_find_the_first() {
    let mut o = Object::new();
    o.push("k", 1);
    o.push("k", 2);
    assert_eq!(o.len(), 2);
    assert_eq!(o.get("k").and_then(Value::as_i64), Some(1));
    assert_eq!(o.get_all("k").count(), 2);
}

#[test]
fn get_mut_edits_in_place() {
    let mut o = abc();
    *o.get_mut("b").unwrap() = Value::from("two");
    assert_eq!(o.get("b").and_then(Value::as_str), Some("two"));
    assert!(o.get_mut("missing").is_none());
}

#[test]
fn iter_mut_visits_members_in_order_and_edits_them() {
    let mut o = abc();
    for (key, value) in o.iter_mut() {
        if key != "b" {
            *value = Value::from(key);
        }
    }
    assert_eq!(o["a"].as_str(), Some("a"));
    assert_eq!(o["b"].as_i64(), Some(2));
    assert_eq!(o["c"].as_str(), Some("c"));
    assert_eq!(keys(&o), ["a", "b", "c"]);
}

#[test]
fn remove_returns_the_value_and_keeps_the_order_of_the_rest() {
    let mut o = abc();
    assert_eq!(o.remove("b"), Some(Value::int(2)));
    assert_eq!(keys(&o), ["a", "c"]);
    assert_eq!(o.remove("b"), None);
    assert_eq!(o.get("c").and_then(Value::as_i64), Some(3));
}

#[test]
fn remove_keeps_the_index_pointing_at_the_right_members() {
    let mut o = abc();
    o.remove("a");
    assert_eq!(o.get("b").and_then(Value::as_i64), Some(2));
    assert_eq!(o.get("c").and_then(Value::as_i64), Some(3));
    assert!(!o.contains_key("a"));
    o.insert("a", 7);
    assert_eq!(keys(&o), ["b", "c", "a"]);
    assert_eq!(o.get("a").and_then(Value::as_i64), Some(7));
}

#[test]
fn removing_a_repeated_key_promotes_the_next_one() {
    let mut o = Object::new();
    o.push("k", 1);
    o.push("other", 0);
    o.push("k", 2);
    assert_eq!(o.remove("k"), Some(Value::int(1)));
    assert_eq!(o.get("k").and_then(Value::as_i64), Some(2));
    assert_eq!(keys(&o), ["other", "k"]);
}

#[test]
fn clear_empties_the_object_and_its_index() {
    let mut o = abc();
    o.clear();
    assert!(o.is_empty());
    assert!(o.get("a").is_none());
    o.insert("a", 1);
    assert_eq!(o.len(), 1);
}

#[test]
fn from_iterator_and_extend_append_every_pair() {
    let o: Object = [("a", 1), ("a", 2), ("b", 3)].into_iter().collect();
    assert_eq!(o.len(), 3);
    assert_eq!(o.get("a").and_then(Value::as_i64), Some(1));

    let mut o = Object::new();
    o.extend([("x", Value::int(1)), ("y", Value::int(2))]);
    o.extend(vec![("x".to_string(), Value::int(3))]);
    assert_eq!(keys(&o), ["x", "y", "x"]);
}

#[test]
fn from_a_vec_of_pairs_keeps_order_and_repeats() {
    let o = Object::from(vec![("b", Value::int(1)), ("a", Value::int(2))]);
    assert_eq!(keys(&o), ["b", "a"]);
    let o = Object::from(vec![("k".to_string(), Value::null())]);
    assert!(o["k"].is_null());
}

#[test]
fn indexing_by_name_finds_the_member() {
    let o = abc();
    assert_eq!(o["c"].as_i64(), Some(3));
}

#[test]
#[should_panic(expected = "no member named \"zz\"")]
fn indexing_a_missing_name_panics() {
    let o = abc();
    let _ = &o["zz"];
}

#[test]
fn iteration_forms_agree() {
    let mut o = abc();
    let by_ref: Vec<_> = (&o).into_iter().map(|(k, _)| k.to_string()).collect();
    assert_eq!(by_ref, ["a", "b", "c"]);
    for (_, v) in &mut o {
        *v = Value::null();
    }
    assert!(o.values().all(Value::is_null));
    let owned: Vec<_> = o.into_iter().map(|(k, _)| k).collect();
    assert_eq!(owned, ["a", "b", "c"]);
}

#[test]
fn equality_compares_members_in_order_not_the_index() {
    let mut a = abc();
    a.remove("a");
    a.insert("a", 1);
    let b = Object::from(vec![
        ("b", Value::int(2)),
        ("c", Value::int(3)),
        ("a", Value::int(1)),
    ]);
    assert_eq!(a, b);
    assert_ne!(a, abc());
}

#[test]
fn len_counts_repeated_keys_each_time() {
    let mut o = Object::new();
    o.push("k", 1);
    o.push("k", 2);
    assert_eq!(o.len(), 2);
    assert_eq!(o.keys().len(), 2);
}

#[test]
fn debug_lists_the_members_in_order_without_the_lookup_index() {
    let mut o = abc();
    o.push("a", 9);
    let shown = format!("{o:?}");
    assert!(!shown.contains("index"), "{shown}");
    assert!(!shown.contains("entries"), "{shown}");
    let a = shown.find("\"a\"").unwrap();
    let b = shown.find("\"b\"").unwrap();
    let c = shown.find("\"c\"").unwrap();
    assert!(a < b && b < c, "{shown}");
    assert_eq!(shown.matches("\"a\"").count(), 2, "{shown}");
}
