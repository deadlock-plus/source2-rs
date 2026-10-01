//! Text KV3 writer.

use std::fmt::Write as _;

use crate::error::{Error, Result};
use crate::guid::{GENERIC_FORMAT, format_guid};
use crate::value::flag;
use crate::{Document, Kind, Object, Value};

/// Render a document as text KV3, header line included.
///
/// The header states the document's text encoding and format name with its format GUID, as
/// [`parse_text`](crate::parse_text) read them or as [`Document::new`] defaults them. A format
/// name that is empty (a document read from a binary block of a format other than the generic
/// one, which carries no names) is spelled `unnamed`.
///
/// Layout follows Valve's own text KV3: tab indentation, `{` and `[` on their own line under a
/// `key =`, and a trailing comma after every array element. Strings are written quoted and
/// escaped, except [`Value::multiline`] ones, which are written between `"""` when they can be
/// unchanged. A value's flags are written as prefixes, `resource:"path"`. Unsigned integers are
/// written in hexadecimal (`0x2A`) so they read back unsigned; signed ones in decimal.
///
/// # What text cannot hold
///
/// Binary storage hints - integer widths, float against double, array layouts - have no text
/// form and are dropped; the values themselves are kept. Comments are not part of a document.
/// The format has no spelling for a NaN or infinite double or for a flag bit outside
/// [`flag::SPELLED`], and a value that needs one is an error rather than silently changed.
///
/// # Errors
///
/// [`Error::Invalid`] if a `Double` is NaN or infinite, or a value carries a flag bit with no
/// text spelling.
pub fn write_text(doc: &Document) -> Result<String> {
    let name = match doc.text.format_name.as_str() {
        "" if doc.options.format == GENERIC_FORMAT => "generic",
        "" => "unnamed",
        name => name,
    };
    let mut out = format!(
        "<!-- kv3 encoding:{}:version{{{}}} format:{}:version{{{}}} -->\n",
        doc.text.encoding.name,
        doc.text.encoding.guid_string(),
        name,
        format_guid(&doc.options.format),
    );
    write_value(&mut out, &doc.root, 0)?;
    out.push('\n');
    Ok(out)
}

fn indent(out: &mut String, depth: usize) {
    out.extend(std::iter::repeat_n('\t', depth));
}

fn is_block(v: &Value) -> bool {
    match v.kind() {
        Kind::Object(_) => true,
        Kind::Array(a) => !a.is_empty(),
        _ => false,
    }
}

fn write_flags(out: &mut String, flags: u8) -> Result<()> {
    if flags & !flag::SPELLED != 0 {
        return Err(Error::Invalid(format!(
            "flag bits {:#04x} have no text spelling",
            flags & !flag::SPELLED
        )));
    }
    for (bit, name) in flag::NAMES {
        if flags & bit != 0 {
            out.push_str(name);
            out.push(':');
        }
    }
    Ok(())
}

/// Write a value at the start of a line already indented to `depth`.
fn write_value(out: &mut String, v: &Value, depth: usize) -> Result<()> {
    write_flags(out, v.flags())?;
    match v.kind() {
        Kind::Object(o) => write_object(out, o, depth),
        Kind::Array(a) if !a.is_empty() => {
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
        Kind::String(s) if v.is_multiline() && fits_multiline(s) => {
            out.push_str("\"\"\"\n");
            out.push_str(s);
            out.push_str("\n\"\"\"");
            Ok(())
        }
        kind => write_scalar(out, kind),
    }
}

/// Whether `s` survives between `"""`: the reader drops one newline after the opener and one
/// before the closer, so a closing quote run inside, or a trailing carriage return that would be
/// read as part of a `\r\n`, cannot be kept raw.
fn fits_multiline(s: &str) -> bool {
    !s.contains("\"\"\"") && !s.ends_with('\r')
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

fn write_scalar(out: &mut String, kind: &Kind) -> Result<()> {
    match kind {
        Kind::Null => out.push_str("null"),
        Kind::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Kind::Int(i) => write!(out, "{i}").expect("writing to a String cannot fail"),
        Kind::UInt(u) => write!(out, "{u:#X}").expect("writing to a String cannot fail"),
        Kind::Double(d) => {
            if !d.is_finite() {
                return Err(Error::Invalid(format!(
                    "text KV3 has no spelling for the non-finite double {d}"
                )));
            }
            // Debug is the shortest form that parses back bit-identical, and unlike Display it
            // always carries a `.` or an exponent, so it never reads as an int.
            write!(out, "{d:?}").expect("writing to a String cannot fail");
        }
        Kind::String(s) => write_string(out, s),
        Kind::Blob(b) => {
            out.push_str("#[");
            for byte in b {
                write!(out, " {byte:02X}").expect("writing to a String cannot fail");
            }
            out.push_str(" ]");
        }
        Kind::Array(_) => out.push_str("[ ]"),
        Kind::Object(_) => unreachable!("objects are written as blocks"),
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
