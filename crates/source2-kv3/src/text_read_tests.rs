use crate::value::MAX_DEPTH;
use crate::{Object, Value, parse_text};

const HEADER: &str = "<!-- kv3 encoding:text:version{e21c7f3c-8a33-41c5-9977-a76d3a32aa0d} format:generic:version{7412167c-06e9-4698-aff2-e63eb59037e7} -->";

fn obj(members: &[(&str, Value)]) -> Value {
    let mut o = Object::default();
    for (k, v) in members {
        o.insert((*k).to_string(), v.clone());
    }
    Value::Object(o)
}

fn s(v: &str) -> Value {
    Value::String(v.to_string())
}

fn ok(input: &str) -> Value {
    parse_text(input).unwrap_or_else(|e| panic!("{input:?} should parse: {e}"))
}

fn rejects(input: &str) {
    assert!(parse_text(input).is_err(), "{input:?} should be rejected");
}

#[test]
fn empty_object_with_header() {
    assert_eq!(ok(&format!("{HEADER}\n{{\n}}\n")), obj(&[]));
}

#[test]
fn header_is_optional() {
    assert_eq!(ok("{ a = 1 }"), obj(&[("a", Value::Int(1))]));
}

#[test]
fn header_accepts_any_guids() {
    let h = "<!-- kv3 encoding:text:version{00000000-0000-0000-0000-000000000000} format:vdata:version{ffffffff-ffff-ffff-ffff-ffffffffffff} -->";
    assert_eq!(ok(&format!("{h}\n{{}}")), obj(&[]));
}

#[test]
fn malformed_headers_are_rejected() {
    rejects("<!-- kv3 encoding:text:version{e21c7f3c} format:generic:version{7412167c} {}");
    rejects("<!-- kv3 encoding:text:version{e21c7f3c} -->\n{}");
    rejects("<!-- kv3 -->\n{}");
    rejects("<!-- kv3 encoding:text format:generic -->\n{}");
    rejects("<!-- not kv3 -->\n{}");
}

#[test]
fn byte_order_mark_is_skipped() {
    assert_eq!(ok("\u{feff}{ a = 1 }"), obj(&[("a", Value::Int(1))]));
}

#[test]
fn scalars_in_an_object() {
    let input = r#"{
        t = true
        f = false
        n = null
        i = 42
        neg = -7
        d = 1.5
        e = 2e3
        ne = -1.25E-2
        str = "hi"
    }"#;
    assert_eq!(
        ok(input),
        obj(&[
            ("t", Value::Bool(true)),
            ("f", Value::Bool(false)),
            ("n", Value::Null),
            ("i", Value::Int(42)),
            ("neg", Value::Int(-7)),
            ("d", Value::Double(1.5)),
            ("e", Value::Double(2000.0)),
            ("ne", Value::Double(-0.0125)),
            ("str", s("hi")),
        ])
    );
}

#[test]
fn integers_follow_the_binary_reader_mapping() {
    let input = "{ max = 9223372036854775807 min = -9223372036854775808 big = 9223372036854775808 top = 18446744073709551615 hex = 0xFF hexbig = 0xFFFFFFFFFFFFFFFF neghex = -0x10 }";
    assert_eq!(
        ok(input),
        obj(&[
            ("max", Value::Int(i64::MAX)),
            ("min", Value::Int(i64::MIN)),
            ("big", Value::UInt(1 << 63)),
            ("top", Value::UInt(u64::MAX)),
            ("hex", Value::Int(255)),
            ("hexbig", Value::UInt(u64::MAX)),
            ("neghex", Value::Int(-16)),
        ])
    );
}

#[test]
fn integers_out_of_range_are_rejected() {
    rejects("{ a = 18446744073709551616 }");
    rejects("{ a = -9223372036854775809 }");
    rejects("{ a = 0x1FFFFFFFFFFFFFFFF }");
}

#[test]
fn quoted_and_bare_keys() {
    assert_eq!(
        ok(r#"{ bare_key.x-1 = 1 "quoted key" = 2 "" = 3 }"#),
        obj(&[
            ("bare_key.x-1", Value::Int(1)),
            ("quoted key", Value::Int(2)),
            ("", Value::Int(3)),
        ])
    );
}

#[test]
fn members_may_be_separated_by_commas_or_newlines() {
    let expected = obj(&[("a", Value::Int(1)), ("b", Value::Int(2))]);
    assert_eq!(ok("{ a = 1, b = 2 }"), expected);
    assert_eq!(ok("{ a = 1,\n b = 2, }"), expected);
    assert_eq!(ok("{\n a = 1\n b = 2\n}"), expected);
}

#[test]
fn duplicate_keys_are_kept_in_order() {
    let v = ok("{ a = 1 a = 2 }");
    let o = v.as_object().unwrap();
    assert_eq!(o.len(), 2);
    assert_eq!(o.get("a"), Some(&Value::Int(1)));
    assert_eq!(
        o.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>(),
        vec![Value::Int(1), Value::Int(2)]
    );
}

#[test]
fn arrays() {
    assert_eq!(
        ok("{ a = [ 1, 2, 3 ] }"),
        obj(&[(
            "a",
            Value::Array(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
        )])
    );
    assert_eq!(ok("{ a = [] }"), obj(&[("a", Value::Array(vec![]))]));
    assert_eq!(
        ok("{ a = [ \"x\", \"y\", ] }"),
        obj(&[("a", Value::Array(vec![s("x"), s("y")]))])
    );
    assert_eq!(
        ok("{ a = [\n 1\n 2\n] }"),
        obj(&[("a", Value::Array(vec![Value::Int(1), Value::Int(2)]))])
    );
}

#[test]
fn nested_containers() {
    assert_eq!(
        ok("{ a = { b = [ { c = null } [ ] ] } }"),
        obj(&[(
            "a",
            obj(&[(
                "b",
                Value::Array(vec![obj(&[("c", Value::Null)]), Value::Array(vec![])])
            )])
        )])
    );
}

#[test]
fn root_may_be_any_value() {
    assert_eq!(ok("[ 1 ]"), Value::Array(vec![Value::Int(1)]));
    assert_eq!(ok("7"), Value::Int(7));
}

#[test]
fn string_escapes() {
    assert_eq!(
        ok(r#"{ a = "line\nbreak\ttab \"q\" back\\slash \/ \u0041" }"#),
        obj(&[("a", s("line\nbreak\ttab \"q\" back\\slash / A"))])
    );
}

#[test]
fn unknown_escapes_keep_the_backslash() {
    assert_eq!(
        ok(r#"{ a = "C:\dir\x\y" }"#),
        obj(&[("a", s(r"C:\dir\x\y"))])
    );
}

#[test]
fn non_ascii_strings_survive() {
    assert_eq!(ok("{ a = \"héllo ✓\" }"), obj(&[("a", s("héllo ✓"))]));
}

#[test]
fn multiline_strings() {
    assert_eq!(
        ok("{ a = \"\"\"\nfirst\n  second \"quoted\" \\n raw\n\"\"\" }"),
        obj(&[("a", s("first\n  second \"quoted\" \\n raw"))])
    );
    assert_eq!(ok("{ a = \"\"\"inline\"\"\" }"), obj(&[("a", s("inline"))]));
    assert_eq!(ok("{ a = \"\"\"\"\"\" }"), obj(&[("a", s(""))]));
}

#[test]
fn comments_are_ignored() {
    let input = "// leading\n{ // after brace\n a = 1 /* inline */ b = /* before value */ 2\n /* multi\nline */\n c = [ 1 // tail\n ]\n} // trailing";
    assert_eq!(
        ok(input),
        obj(&[
            ("a", Value::Int(1)),
            ("b", Value::Int(2)),
            ("c", Value::Array(vec![Value::Int(1)])),
        ])
    );
}

#[test]
fn comment_markers_inside_strings_are_text() {
    assert_eq!(
        ok(r#"{ a = "http://x /* y */" }"#),
        obj(&[("a", s("http://x /* y */"))])
    );
}

#[test]
fn resource_flags_are_dropped() {
    let input = r#"{
        m = resource:"models/a.vmdl"
        p = panorama:"file://{images}/x.png"
        snd = soundevent:"Hero.Attack"
        sub = subclass:"abilities/x"
        ml = resource:"""
path
"""
    }"#;
    assert_eq!(
        ok(input),
        obj(&[
            ("m", s("models/a.vmdl")),
            ("p", s("file://{images}/x.png")),
            ("snd", s("Hero.Attack")),
            ("sub", s("abilities/x")),
            ("ml", s("path")),
        ])
    );
}

#[test]
fn byte_blobs() {
    assert_eq!(
        ok("{ a = #[ 01 02 ff ] b = #[] c = #[0A0b] }"),
        obj(&[
            ("a", Value::Blob(vec![1, 2, 255])),
            ("b", Value::Blob(vec![])),
            ("c", Value::Blob(vec![0x0a, 0x0b])),
        ])
    );
    assert_eq!(
        ok("{ a = #[\n 00 11\n 22 33\n] }"),
        obj(&[("a", Value::Blob(vec![0, 0x11, 0x22, 0x33]))])
    );
}

#[test]
fn bad_blobs_are_rejected() {
    rejects("{ a = #[ 0 ] }");
    rejects("{ a = #[ 012 ] }");
    rejects("{ a = #[ zz ] }");
    rejects("{ a = #[ 01 02 }");
}

#[test]
fn unterminated_inputs_are_rejected() {
    rejects("{ a = \"open }");
    rejects("{ a = \"\"\"open }");
    rejects("{ a = 1");
    rejects("{");
    rejects("{ a = [ 1, 2 }");
    rejects("[ 1, 2");
    rejects("{ a = 1 /* never closed }");
    rejects("{ a = \"bad \\");
    rejects("{ a = ");
    rejects("{ a }");
    rejects("{ a = }");
    rejects("");
    rejects("   \n ");
    rejects("<!-- kv3 encoding:text:version{a} format:generic:version{b} -->");
}

#[test]
fn stray_tokens_are_rejected() {
    rejects("{ a = 1 } }");
    rejects("{ a = 1 } 2");
    rejects("{ a = 1 ] }");
    rejects("[ 1 }");
    rejects("{ a = [ 1 , , 2 ] }");
    rejects("{ , a = 1 }");
    rejects("{ = 1 }");
    rejects("{ a = b }");
    rejects("{ a == 1 }");
    rejects("{ a = 1.2.3 }");
    rejects("{ a = 12abc }");
    rejects("{ a = 0x }");
    rejects("{ a = - }");
    rejects("{ a = resource: }");
    rejects("{ 1 }");
    rejects("{ a = \"\\u00\" }");
}

#[test]
fn nesting_at_the_limit_parses_and_past_it_fails() {
    let nest = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
    assert!(parse_text(&nest(MAX_DEPTH as usize + 1)).is_ok());
    assert!(parse_text(&nest(MAX_DEPTH as usize + 2)).is_err());
}

#[test]
fn hostile_nesting_is_an_error_not_a_stack_overflow() {
    let deep = "[".repeat(200_000);
    assert!(parse_text(&deep).is_err());
    let deep_obj = "{ a = ".repeat(200_000);
    assert!(parse_text(&deep_obj).is_err());
}

#[test]
fn errors_name_the_line() {
    let e = parse_text("{\n a = 1\n b = ?\n}").unwrap_err().to_string();
    assert!(e.contains("line 3"), "{e}");
}
