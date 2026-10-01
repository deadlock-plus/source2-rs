use crate::{Object, Value, text_write::write_text};

const HEADER: &str = "<!-- kv3 encoding:text:version{e21c7f3c-8a33-41c5-9977-a76d3a32aa0d} format:generic:version{7412167c-06e9-4698-aff2-e63eb59037e7} -->\n";

fn obj(members: Vec<(&str, Value)>) -> Value {
    let mut o = Object::default();
    for (k, v) in members {
        o.insert(k.to_string(), v);
    }
    Value::Object(o)
}

fn out(v: &Value) -> String {
    write_text(v).unwrap()
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
        ("a", Value::Null),
        ("b", Value::Bool(true)),
        ("c", Value::Bool(false)),
        ("d", Value::Int(-5)),
        ("e", Value::UInt(u64::MAX)),
        ("f", Value::String("hi".into())),
    ]);
    assert_eq!(
        out(&v),
        format!(
            "{HEADER}{{\n\ta = null\n\tb = true\n\tc = false\n\td = -5\n\te = 18446744073709551615\n\tf = \"hi\"\n}}\n"
        )
    );
}

#[test]
fn integer_extremes() {
    assert_eq!(
        scalar(Value::Int(i64::MIN)),
        "{\n\tk = -9223372036854775808\n}\n"
    );
}

#[test]
fn nested_object_and_arrays() {
    let v = obj(vec![
        (
            "inner",
            obj(vec![("x", Value::Int(1)), ("list", Value::Array(vec![]))]),
        ),
        (
            "arr",
            Value::Array(vec![
                Value::Int(1),
                Value::Array(vec![Value::Bool(true)]),
                obj(vec![("y", Value::Null)]),
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
        scalar(Value::Blob(vec![0x01, 0xAB, 0xff])),
        "{\n\tk = #[ 01 AB FF ]\n}\n"
    );
    assert_eq!(scalar(Value::Blob(vec![])), "{\n\tk = #[ ]\n}\n");
}

#[test]
fn keys_are_quoted_only_when_not_identifiers() {
    let v = obj(vec![
        ("plain_1", Value::Int(1)),
        ("has space", Value::Int(2)),
        ("1lead", Value::Int(3)),
        ("", Value::Int(4)),
        ("q\"uote", Value::Int(5)),
        ("m_flag.x", Value::Int(6)),
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
        scalar(Value::String("a\"b\\c\nd\re\tf".into())),
        "{\n\tk = \"a\\\"b\\\\c\\nd\\re\\tf\"\n}\n"
    );
    assert_eq!(
        scalar(Value::String("\u{0}\u{1f}\u{7f}".into())),
        "{\n\tk = \"\\u0000\\u001f\\u007f\"\n}\n"
    );
    assert_eq!(
        scalar(Value::String("héllo ✓ 😀".into())),
        "{\n\tk = \"héllo ✓ 😀\"\n}\n"
    );
    assert_eq!(scalar(Value::String(String::new())), "{\n\tk = \"\"\n}\n");
}

#[test]
fn doubles_always_look_like_doubles() {
    assert_eq!(scalar(Value::Double(1.0)), "{\n\tk = 1.0\n}\n");
    assert_eq!(scalar(Value::Double(0.0)), "{\n\tk = 0.0\n}\n");
    assert_eq!(scalar(Value::Double(-0.0)), "{\n\tk = -0.0\n}\n");
    assert_eq!(scalar(Value::Double(0.1)), "{\n\tk = 0.1\n}\n");
    assert_eq!(scalar(Value::Double(-2.5)), "{\n\tk = -2.5\n}\n");
    assert_eq!(scalar(Value::Double(1e300)), "{\n\tk = 1e300\n}\n");
    assert_eq!(scalar(Value::Double(1e-7)), "{\n\tk = 1e-7\n}\n");
    assert_eq!(
        scalar(Value::Double(f64::MAX)),
        "{\n\tk = 1.7976931348623157e308\n}\n"
    );
    assert_eq!(scalar(Value::Double(5e-324)), "{\n\tk = 5e-324\n}\n");
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
        let s = scalar(Value::Double(d));
        let text = s.trim().strip_prefix("{\n\tk = ").unwrap();
        let text = text.strip_suffix("\n}").unwrap();
        assert!(text.contains(['.', 'e']), "{text}");
        assert_eq!(text.parse::<f64>().unwrap().to_bits(), d.to_bits());
    }
}

#[test]
fn non_finite_doubles_are_errors() {
    for d in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(write_text(&obj(vec![("k", Value::Double(d))])).is_err());
    }
}

#[test]
fn root_may_be_a_scalar_or_array() {
    assert_eq!(out(&Value::Int(7)), format!("{HEADER}7\n"));
    assert_eq!(
        out(&Value::Array(vec![Value::Int(1)])),
        format!("{HEADER}[\n\t1,\n]\n")
    );
}

#[test]
fn repeated_keys_are_all_written() {
    let v = obj(vec![("a", Value::Int(1)), ("a", Value::Int(2))]);
    assert_eq!(out(&v), format!("{HEADER}{{\n\ta = 1\n\ta = 2\n}}\n"));
}

#[test]
fn written_text_parses_back_to_the_same_tree() {
    let tree = obj(vec![
        ("null", Value::Null),
        ("yes", Value::Bool(true)),
        ("neg", Value::Int(i64::MIN)),
        ("big", Value::UInt(u64::MAX)),
        ("pi", Value::Double(std::f64::consts::PI)),
        ("tiny", Value::Double(5e-324)),
        ("whole", Value::Double(1.0)),
        ("text", Value::String("q\" b\\ n\n t\t \u{1} é".into())),
        ("empty", Value::String(String::new())),
        ("blob", Value::Blob(vec![0, 1, 0xAB, 0xFF])),
        ("a key with spaces", Value::Int(1)),
        (
            "arr",
            Value::Array(vec![Value::Int(1), Value::Array(vec![])]),
        ),
        (
            "nested",
            obj(vec![
                ("inner", obj(vec![])),
                ("dup", Value::Int(1)),
                ("dup", Value::Int(2)),
            ]),
        ),
    ]);
    let text = write_text(&tree).unwrap();
    assert_eq!(crate::parse_text(&text).unwrap(), tree);
}
