//! Text spellings of types and values.

use crate::{Color, ElementRef, FloatFormat, Time, Uuid, Value, ValueType};
use std::fmt::Write;

const ARRAY_SUFFIX: &str = "_array";

/// Type name as written, without the array suffix.
pub(crate) fn type_name(t: ValueType) -> &'static str {
    match t {
        ValueType::Element => "element",
        ValueType::Int => "int",
        ValueType::Float => "float",
        ValueType::Bool => "bool",
        ValueType::String => "string",
        ValueType::Binary => "binary",
        ValueType::ObjectId => "objectid",
        ValueType::Time => "time",
        ValueType::Color => "color",
        ValueType::Vector2 => "vector2",
        ValueType::Vector3 => "vector3",
        ValueType::Vector4 => "vector4",
        ValueType::QAngle => "qangle",
        ValueType::Quaternion => "quaternion",
        ValueType::Matrix => "matrix",
        ValueType::UInt64 => "uint64",
        ValueType::UInt8 => "uint8",
    }
}

/// Resolves a type word to its value type and whether it names an array.
pub(crate) fn parse_type(word: &str) -> Option<(ValueType, bool)> {
    let (base, array) = match word.strip_suffix(ARRAY_SUFFIX) {
        Some(b) => (b, true),
        None => (word, false),
    };
    let t = match base {
        "element" => ValueType::Element,
        "int" => ValueType::Int,
        "float" => ValueType::Float,
        "bool" => ValueType::Bool,
        "string" => ValueType::String,
        "binary" => ValueType::Binary,
        "objectid" => ValueType::ObjectId,
        "time" => ValueType::Time,
        "color" => ValueType::Color,
        "vector2" => ValueType::Vector2,
        "vector3" => ValueType::Vector3,
        "vector4" => ValueType::Vector4,
        "qangle" | "angle" => ValueType::QAngle,
        "quaternion" => ValueType::Quaternion,
        "matrix" | "vmatrix" => ValueType::Matrix,
        "uint64" => ValueType::UInt64,
        "uint8" | "byte" => ValueType::UInt8,
        _ => return None,
    };
    Some((t, array))
}

pub(crate) fn array_type_name(t: ValueType) -> String {
    format!("{}{ARRAY_SUFFIX}", type_name(t))
}

/// Parses a float. Rust's own spellings (`inf`, `-inf`, `NaN`, `-0`) work as they are;
/// the Windows C runtime's `1.#INF`, `-1.#IND` and `1.#QNAN` families are accepted too.
fn parse_f32(w: &str) -> Option<f32> {
    if let Ok(x) = w.parse() {
        return Some(x);
    }
    let lower = w.to_ascii_lowercase();
    let (neg, rest) = match lower.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, lower.strip_prefix('+').unwrap_or(&lower)),
    };
    let word = rest.strip_prefix("1.#")?.trim_end_matches('0');
    let x = match word {
        "inf" => f32::INFINITY,
        "ind" | "qnan" | "snan" | "nan" => f32::NAN,
        _ => return None,
    };
    Some(if neg { -x } else { x })
}

fn floats<const N: usize>(s: &str) -> Result<[f32; N], String> {
    let mut out = [0f32; N];
    let mut it = s.split_ascii_whitespace();
    for slot in &mut out {
        let w = it.next().ok_or_else(|| format!("expected {N} numbers"))?;
        *slot = parse_f32(w).ok_or_else(|| format!("`{w}` is not a number"))?;
    }
    if it.next().is_some() {
        return Err(format!("expected {N} numbers"));
    }
    Ok(out)
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn time_ticks(s: &str) -> Result<i32, String> {
    let secs: f64 = s
        .trim()
        .parse()
        .map_err(|_| format!("`{s}` is not a time"))?;
    let ticks = (secs * 10_000.0).round();
    if !ticks.is_finite() || ticks < f64::from(i32::MIN) || ticks > f64::from(i32::MAX) {
        return Err(format!("time `{s}` is out of range"));
    }
    #[allow(clippy::cast_possible_truncation)]
    Ok(ticks as i32)
}

/// Parses one scalar from its text form. The message is for [`crate::Error::Syntax`].
pub(crate) fn parse_value(t: ValueType, s: &str) -> Result<Value, String> {
    let bad = |what: &str| format!("`{s}` is not {what}");
    Ok(match t {
        ValueType::Element => {
            if s.is_empty() {
                Value::Element(ElementRef::Null)
            } else {
                Value::Element(ElementRef::External(
                    Uuid::parse(s).ok_or_else(|| bad("an element id"))?,
                ))
            }
        }
        ValueType::Int => Value::Int(s.trim().parse().map_err(|_| bad("an int"))?),
        ValueType::Float => Value::Float(parse_f32(s.trim()).ok_or_else(|| bad("a float"))?),
        ValueType::Bool => match s.trim() {
            "0" | "false" => Value::Bool(false),
            "1" | "true" => Value::Bool(true),
            _ => return Err(bad("a bool")),
        },
        ValueType::String => Value::String(s.to_string()),
        ValueType::Binary => {
            let b = s.trim().as_bytes();
            if b.len() & 1 == 1 {
                return Err(bad("hex with an even digit count"));
            }
            let mut out = Vec::with_capacity(b.len() / 2);
            for pair in b.chunks(2) {
                let hi = hex_digit(pair[0]).ok_or_else(|| bad("hex"))?;
                let lo = hex_digit(pair[1]).ok_or_else(|| bad("hex"))?;
                out.push(hi << 4 | lo);
            }
            Value::Binary(out)
        }
        ValueType::ObjectId => Value::ObjectId(Uuid::parse(s).ok_or_else(|| bad("an id"))?),
        ValueType::Time => Value::Time(Time(time_ticks(s)?)),
        ValueType::Color => {
            let mut c = [0u8; 4];
            let mut it = s.split_ascii_whitespace();
            for slot in &mut c {
                let w = it.next().ok_or_else(|| bad("a color of 4 bytes"))?;
                *slot = w.parse().map_err(|_| bad("a color of 4 bytes"))?;
            }
            if it.next().is_some() {
                return Err(bad("a color of 4 bytes"));
            }
            Value::Color(Color {
                r: c[0],
                g: c[1],
                b: c[2],
                a: c[3],
            })
        }
        ValueType::Vector2 => Value::Vector2(floats(s)?),
        ValueType::Vector3 => Value::Vector3(floats(s)?),
        ValueType::Vector4 => Value::Vector4(floats(s)?),
        ValueType::QAngle => Value::QAngle(floats(s)?),
        ValueType::Quaternion => Value::Quaternion(floats(s)?),
        ValueType::Matrix => Value::Matrix(floats(s)?),
        ValueType::UInt64 => Value::UInt64(s.trim().parse().map_err(|_| bad("a uint64"))?),
        ValueType::UInt8 => Value::UInt8(s.trim().parse().map_err(|_| bad("a uint8"))?),
    })
}

fn float(out: &mut String, x: f32, fmt: FloatFormat) {
    match fmt {
        FloatFormat::Shortest => {
            let _ = write!(out, "{x}");
        }
        FloatFormat::Fixed10 if !x.is_finite() => {
            let _ = write!(out, "{x}");
        }
        FloatFormat::Fixed10 => {
            let s = format!("{x:.10}");
            out.push_str(s.trim_end_matches('0').trim_end_matches('.'));
        }
    }
}

fn join(out: &mut String, f: &[f32], fmt: FloatFormat) {
    for (i, x) in f.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        float(out, *x, fmt);
    }
}

fn time_text(out: &mut String, ticks: i32) {
    let a = i64::from(ticks).abs();
    if ticks < 0 {
        out.push('-');
    }
    let (whole, frac) = (a / 10_000, a % 10_000);
    let _ = write!(out, "{whole}");
    if frac != 0 {
        let digits = format!("{frac:04}");
        out.push('.');
        out.push_str(digits.trim_end_matches('0'));
    }
}

/// Escapes a string for use between quotes.
pub(crate) fn escape(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{0b}' => out.push_str("\\v"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\u{07}' => out.push_str("\\a"),
            c => out.push(c),
        }
    }
}

/// Writes one scalar as it appears between quotes. Element values are the caller's job.
pub(crate) fn format_value(out: &mut String, v: &Value, fmt: FloatFormat) {
    match v {
        Value::Element(ElementRef::External(u)) | Value::ObjectId(u) => {
            let _ = write!(out, "{u}");
        }
        Value::Element(_) | Value::Array(..) => {}
        Value::Int(i) => {
            let _ = write!(out, "{i}");
        }
        Value::Float(x) => float(out, *x, fmt),
        Value::Bool(b) => out.push(if *b { '1' } else { '0' }),
        Value::String(s) => escape(out, s),
        Value::Binary(b) => {
            for byte in b {
                let _ = write!(out, "{byte:02X}");
            }
        }
        Value::Time(t) => time_text(out, t.0),
        Value::Color(c) => {
            let _ = write!(out, "{} {} {} {}", c.r, c.g, c.b, c.a);
        }
        Value::Vector2(f) => join(out, f, fmt),
        Value::Vector3(f) | Value::QAngle(f) => join(out, f, fmt),
        Value::Vector4(f) | Value::Quaternion(f) => join(out, f, fmt),
        Value::Matrix(m) => {
            for (i, row) in m.chunks(4).enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                join(out, row, fmt);
            }
        }
        Value::UInt64(n) => {
            let _ = write!(out, "{n}");
        }
        Value::UInt8(n) => {
            let _ = write!(out, "{n}");
        }
    }
}
