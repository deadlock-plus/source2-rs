//! Text KV3 writer.

use std::fmt::Write as _;

use crate::{Object, Result, Value, error::Error};

const HEADER: &str = "<!-- kv3 encoding:text:version{e21c7f3c-8a33-41c5-9977-a76d3a32aa0d} format:generic:version{7412167c-06e9-4698-aff2-e63eb59037e7} -->\n";

/// Render a value tree as a text KV3 document, header line included.
///
/// Layout follows Valve's own text KV3: tab indentation, `{` and `[` on their own line
/// under a `key =`, and a trailing comma after every array element. Strings are always
/// written in the escaped single-line form, never `"""`, so any string round-trips.
///
/// # Errors
///
/// If a `Double` is NaN or infinite: the format has no spelling for either.
pub fn write_text(root: &Value) -> Result<String> {
    let mut out = String::from(HEADER);
    write_value(&mut out, root, 0)?;
    out.push('\n');
    Ok(out)
}

fn indent(out: &mut String, depth: usize) {
    out.extend(std::iter::repeat_n('\t', depth));
}

fn is_block(v: &Value) -> bool {
    match v {
        Value::Object(_) => true,
        Value::Array(a) => !a.is_empty(),
        _ => false,
    }
}

/// Write a value at the start of a line already indented to `depth`.
fn write_value(out: &mut String, v: &Value, depth: usize) -> Result<()> {
    match v {
        Value::Object(o) => write_object(out, o, depth),
        Value::Array(a) if !a.is_empty() => {
            out.push_str("[\n");
            for item in a {
                indent(out, depth + 1);
                write_value(out, item, depth + 1)?;
                out.push_str(",\n");
            }
            indent(out, depth);
            out.push(']');
            Ok(())
        }
        _ => write_scalar(out, v),
    }
}

fn write_object(out: &mut String, o: &Object, depth: usize) -> Result<()> {
    out.push_str("{\n");
    for (key, value) in o.iter() {
        indent(out, depth + 1);
        write_key(out, key);
        if is_block(value) {
            out.push_str(" =\n");
            indent(out, depth + 1);
        } else {
            out.push_str(" = ");
        }
        write_value(out, value, depth + 1)?;
        out.push('\n');
    }
    indent(out, depth);
    out.push('}');
    Ok(())
}

fn write_scalar(out: &mut String, v: &Value) -> Result<()> {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => write!(out, "{i}").expect("writing to a String cannot fail"),
        Value::UInt(u) => write!(out, "{u}").expect("writing to a String cannot fail"),
        Value::Double(d) => {
            if !d.is_finite() {
                return Err(Error::Malformed(format!(
                    "text KV3 has no spelling for the non-finite double {d}"
                )));
            }
            // Debug is the shortest form that parses back bit-identical, and unlike
            // Display it always carries a `.` or an exponent, so it never reads as an int.
            write!(out, "{d:?}").expect("writing to a String cannot fail");
        }
        Value::String(s) => write_string(out, s),
        Value::Blob(b) => {
            out.push_str("#[");
            for byte in b {
                write!(out, " {byte:02X}").expect("writing to a String cannot fail");
            }
            out.push_str(" ]");
        }
        Value::Array(_) => out.push_str("[ ]"),
        Value::Object(_) => unreachable!("objects are written as blocks"),
    }
    Ok(())
}

fn write_key(out: &mut String, key: &str) {
    let mut chars = key.chars();
    let bare = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    if bare {
        out.push_str(key);
    } else {
        write_string(out, key);
    }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                write!(out, "\\u{:04x}", u32::from(c)).expect("writing to a String cannot fail");
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
