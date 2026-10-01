use crate::{Document, Object, Value, flag};

const HEADER: &str = "<!-- kv3 encoding:text:version{e21c7f3c-8a33-41c5-9977-a76d3a32aa0d} format:generic:version{7412167c-06e9-4698-aff2-e63eb59037e7} -->\n";

fn obj(members: Vec<(&str, Value)>) -> Value {
    let mut o = Object::default();
    for (k, v) in members {
        o.push(k.to_string(), v);
    }
    Value::from(o)
}

fn out(v: &Value) -> String {
    Document::new(v.clone()).to_text().unwrap()
}

fn scalar(v: Value) -> String {
    out(&obj(vec![("k", v)]))
        .strip_prefix(HEADER)
        .unwrap()
        .to_string()
}

#[test]
fn empty_object_document() {
    assert_eq!(out(&obj(vec![])), format!("{HEADER}{{\n}}\n"));
}

#[test]
fn scalars_render_as_members() {
    let v = obj(vec![
        ("a", Value::null()),
        ("b", Value::from(true)),
        ("c", Value::from(false)),
        ("d", Value::int(-5)),
        ("e", Value::uint(u64::MAX)),
        ("f", Value::from("hi")),
    ]);
    assert_eq!(
        out(&v),
        format!(
            "{HEADER}{{\n\ta = null\n\tb = true\n\tc = false\n\td = -5\n\te = 0xFFFFFFFFFFFFFFFF\n\tf = \"hi\"\n}}\n"
        )
    );
}

#[test]
fn integer_extremes() {
    assert_eq!(
        scalar(Value::int(i64::MIN)),
        "{\n\tk = -9223372036854775808\n}\n"
    );
}

#[test]
fn nested_object_and_arrays() {
    let v = obj(vec![
        (
            "inner",
            obj(vec![("x", Value::int(1)), ("list", Value::array(vec![]))]),
        ),
        (
            "arr",
            Value::array(vec![
                Value::int(1),
                Value::array(vec![Value::from(true)]),
                obj(vec![("y", Value::null())]),
            ]),
        ),
        ("empty", obj(vec![])),
    ]);
    let expected = "{\n\
\tinner =\n\
\t{\n\
\t\tx = 1\n\
\t\tlist = [ ]\n\
\t}\n\
\tarr =\n\
\t[\n\
\t\t1,\n\
\t\t[\n\
\t\t\ttrue,\n\
\t\t],\n\
\t\t{\n\
\t\t\ty = null\n\
\t\t},\n\
\t]\n\
\tempty =\n\
\t{\n\
\t}\n\
}\n";
    assert_eq!(out(&v), format!("{HEADER}{expected}"));
}

#[test]
fn blob_renders_as_hex_bytes() {
    assert_eq!(
        scalar(Value::blob(vec![0x01, 0xAB, 0xff])),
        "{\n\tk = #[ 01 AB FF ]\n}\n"
    );
    assert_eq!(scalar(Value::blob(vec![])), "{\n\tk = #[ ]\n}\n");
}

#[test]
fn keys_are_quoted_only_when_not_identifiers() {
    let v = obj(vec![
        ("plain_1", Value::int(1)),
        ("has space", Value::int(2)),
        ("1lead", Value::int(3)),
        ("", Value::int(4)),
        ("q\"uote", Value::int(5)),
        ("m_flag.x", Value::int(6)),
    ]);
    assert_eq!(
        out(&v),
        format!(
            "{HEADER}{{\n\tplain_1 = 1\n\t\"has space\" = 2\n\t\"1lead\" = 3\n\t\"\" = 4\n\t\"q\\\"uote\" = 5\n\tm_flag.x = 6\n}}\n"
        )
    );
}

#[test]
fn string_escapes() {
    assert_eq!(
        scalar(Value::from("a\"b\\c\nd\re\tf")),
        "{\n\tk = \"a\\\"b\\\\c\\nd\\re\\tf\"\n}\n"
    );
    assert_eq!(
        scalar(Value::from("\u{0}\u{1f}\u{7f}")),
        "{\n\tk = \"\\u0000\\u001f\\u007f\"\n}\n"
    );
    assert_eq!(
        scalar(Value::from("héllo ✓ 😀")),
        "{\n\tk = \"héllo ✓ 😀\"\n}\n"
    );
    assert_eq!(scalar(Value::from(String::new())), "{\n\tk = \"\"\n}\n");
}

#[test]
fn doubles_always_look_like_doubles() {
    assert_eq!(scalar(Value::double(1.0)), "{\n\tk = 1.0\n}\n");
    assert_eq!(scalar(Value::double(0.0)), "{\n\tk = 0.0\n}\n");
    assert_eq!(scalar(Value::double(-0.0)), "{\n\tk = -0.0\n}\n");
    assert_eq!(scalar(Value::double(0.1)), "{\n\tk = 0.1\n}\n");
    assert_eq!(scalar(Value::double(-2.5)), "{\n\tk = -2.5\n}\n");
    assert_eq!(scalar(Value::double(1e300)), "{\n\tk = 1e300\n}\n");
    assert_eq!(scalar(Value::double(1e-7)), "{\n\tk = 1e-7\n}\n");
    assert_eq!(
        scalar(Value::double(f64::MAX)),
        "{\n\tk = 1.7976931348623157e308\n}\n"
    );
    assert_eq!(scalar(Value::double(5e-324)), "{\n\tk = 5e-324\n}\n");
}

#[test]
fn doubles_parse_back_bit_identical() {
    for d in [
        0.1,
        1.0 / 3.0,
        123_456_789.123_456_79,
        f64::MIN_POSITIVE,
        f64::EPSILON,
        -1e22,
        1e21,
        9007199254740993.0,
    ] {
        let s = scalar(Value::double(d));
        let text = s.trim().strip_prefix("{\n\tk = ").unwrap();
        let text = text.strip_suffix("\n}").unwrap();
        assert!(text.contains(['.', 'e']), "{text}");
        assert_eq!(text.parse::<f64>().unwrap().to_bits(), d.to_bits());
    }
}

#[test]
fn non_finite_doubles_are_errors() {
    for d in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            Document::new(obj(vec![("k", Value::double(d))]))
                .to_text()
                .is_err()
        );
    }
}

#[test]
fn root_may_be_a_scalar_or_array() {
    assert_eq!(out(&Value::int(7)), format!("{HEADER}7\n"));
    assert_eq!(
        out(&Value::array(vec![Value::int(1)])),
        format!("{HEADER}[\n\t1,\n]\n")
    );
}

#[test]
fn repeated_keys_are_all_written() {
    let v = obj(vec![("a", Value::int(1)), ("a", Value::int(2))]);
    assert_eq!(out(&v), format!("{HEADER}{{\n\ta = 1\n\ta = 2\n}}\n"));
}

#[test]
fn written_text_parses_back_to_the_same_tree() {
    let tree = obj(vec![
        ("null", Value::null()),
        ("yes", Value::from(true)),
        ("neg", Value::int(i64::MIN)),
        ("big", Value::uint(u64::MAX)),
        ("pi", Value::double(std::f64::consts::PI)),
        ("tiny", Value::double(5e-324)),
        ("whole", Value::double(1.0)),
        ("text", Value::from("q\" b\\ n\n t\t \u{1} é")),
        ("empty", Value::from(String::new())),
        ("blob", Value::blob(vec![0, 1, 0xAB, 0xFF])),
        ("a key with spaces", Value::int(1)),
        (
            "arr",
            Value::array(vec![Value::int(1), Value::array(vec![])]),
        ),
        (
            "nested",
            obj(vec![
                ("inner", obj(vec![])),
                ("dup", Value::int(1)),
                ("dup", Value::int(2)),
            ]),
        ),
    ]);
    let text = out(&tree);
    assert_eq!(crate::parse_text(&text).unwrap().root, tree);
}

#[test]
fn flags_are_written_as_prefixes() {
    let v = obj(vec![
        ("a", Value::from("x").with_flags(flag::RESOURCE)),
        (
            "b",
            Value::from("y").with_flags(flag::SOUND_EVENT | flag::RESOURCE_NAME),
        ),
        (
            "c",
            Value::array(vec![Value::from("z").with_flags(flag::PANORAMA)]),
        ),
        (
            "d",
            obj(vec![("n", Value::int(1))]).with_flags(flag::SUBCLASS),
        ),
        ("e", Value::blob(vec![1]).with_flags(flag::RESOURCE)),
    ]);
    assert_eq!(
        out(&v),
        format!(
            "{HEADER}{{\n\ta = resource:\"x\"\n\tb = resource_name:soundevent:\"y\"\n\tc =\n\t[\n\t\tpanorama:\"z\",\n\t]\n\td =\n\tsubclass:{{\n\t\tn = 1\n\t}}\n\te = resource:#[ 01 ]\n}}\n"
        )
    );
}
