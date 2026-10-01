//! Reads the formatting habits of a text body.

use crate::{FloatFormat, Newline, TextStyle};

#[derive(Clone, Copy, PartialEq)]
enum Block {
    Top,
    Prefix,
    Attribute,
    ArrayItem,
}

/// More significant digits than an `f32` needs: shortest spelling never produces them.
const FIXED_EVIDENCE_DIGITS: usize = 10;

/// Infers a document's style from its body; the line ending comes from the header line.
/// A habit the body gives no evidence about keeps its default.
pub(crate) fn detect(body: &[u8], newline: Newline) -> TextStyle {
    let mut style = TextStyle {
        newline,
        ..TextStyle::default()
    };
    let mut lines: Vec<&[u8]> = body.split(|&b| b == b'\n').collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    } else if !body.is_empty() {
        style.final_newline = false;
    }
    let lines: Vec<&[u8]> = lines
        .into_iter()
        .map(|l| l.strip_suffix(b"\r").unwrap_or(l))
        .collect();
    let blank = |i: usize| {
        lines
            .get(i)
            .is_some_and(|l| l.iter().all(|&c| c == b'\t' || c == b' '))
    };
    let mut stack: Vec<Block> = Vec::new();
    let mut after_element = None;
    let mut after_block = None;
    let mut array_space = None;
    let mut inline_arrays = None;
    let mut comma_space = None;
    let mut fixed = false;
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_ascii();
        if array_space.is_none() && l.ends_with(b"_array\" ") {
            array_space = Some(true);
        } else if array_space.is_none() && l.ends_with(b"_array\"") {
            array_space = Some(false);
        }
        if inline_arrays.is_none() {
            if l.windows(9).any(|w| w == b"_array\" [") {
                inline_arrays = Some(true);
            } else if (l.ends_with(b"_array\" ") || l.ends_with(b"_array\""))
                && !l.ends_with(b"element_array\" ")
                && !l.ends_with(b"element_array\"")
            {
                inline_arrays = Some(false);
            }
        }
        if comma_space.is_none() && (t == b"}," || t.ends_with(b"\",")) {
            comma_space = Some(l.ends_with(b", "));
        }
        if t == b"{" {
            let prev = lines[..i].iter().rev().find(|p| !p.trim_ascii().is_empty());
            let kind = match prev {
                _ if !stack.is_empty() => {
                    let quotes = prev.map_or(0, |p| p.iter().filter(|&&c| c == b'"').count());
                    if quotes >= 4 {
                        Block::Attribute
                    } else {
                        Block::ArrayItem
                    }
                }
                Some(p) if p.windows(16).any(|w| w == b"$prefix_element$") => Block::Prefix,
                _ => Block::Top,
            };
            stack.push(kind);
        } else if t == b"}" || t == b"}," {
            let kind = stack.pop();
            if t == b"}" {
                match kind {
                    Some(Block::Attribute) => after_element = Some(blank(i + 1)),
                    Some(Block::Top) => after_block = Some(blank(i + 1)),
                    _ => {}
                }
            }
        } else if !fixed && has_long_number(t) {
            fixed = true;
        }
    }
    if let Some(b) = after_element {
        style.blank_line_after_element = b;
    }
    if let Some(b) = after_block {
        style.blank_line_after_block = b;
    }
    if let Some(a) = array_space {
        style.space_after_array_type = a;
    }
    if let Some(b) = inline_arrays {
        style.inline_arrays = b;
    }
    if let Some(b) = comma_space {
        style.space_after_comma = b;
    }
    if fixed {
        style.float_format = FloatFormat::Fixed10;
    }
    style
}

/// Whether a `"name" "float..." "value"` style line carries a number with more digits than
/// shortest `f32` formatting can produce.
fn has_long_number(line: &[u8]) -> bool {
    let Some(ty_start) = line.iter().position(|&c| c == b'"') else {
        return false;
    };
    let rest = &line[ty_start..];
    let is_float_line = [
        &b"\"float\""[..],
        b"\"vector",
        b"\"qangle\"",
        b"\"quaternion\"",
        b"\"matrix\"",
    ]
    .iter()
    .any(|ty| rest.windows(ty.len()).any(|w| w == *ty));
    if !is_float_line {
        return false;
    }
    let value = rest.rsplit(|&c| c == b'"').nth(1).unwrap_or(&[]);
    value
        .split(|c: &u8| c.is_ascii_whitespace())
        .any(|tok| significant_digits(tok) >= FIXED_EVIDENCE_DIGITS)
}

fn significant_digits(tok: &[u8]) -> usize {
    let mantissa = tok
        .iter()
        .take_while(|&&c| c != b'e' && c != b'E')
        .filter(|c| c.is_ascii_digit())
        .skip_while(|&&c| c == b'0');
    let digits: Vec<u8> = mantissa.copied().collect();
    digits.iter().rposition(|&c| c != b'0').map_or(0, |p| p + 1)
}
