//! Backslash escapes inside quoted strings.
//!
//! The set is the one Valve's C-string conversion table uses:
//! `\n \t \v \b \r \f \a \\ \? \' \"`. Any other `\x` is left as written, backslash included.

fn decode_char(c: char) -> Option<char> {
    Some(match c {
        'n' => '\n',
        't' => '\t',
        'v' => '\u{0b}',
        'b' => '\u{08}',
        'r' => '\r',
        'f' => '\u{0c}',
        'a' => '\u{07}',
        '\\' => '\\',
        '?' => '?',
        '\'' => '\'',
        '"' => '"',
        _ => return None,
    })
}

fn encode_char(c: char) -> Option<char> {
    Some(match c {
        '\n' => 'n',
        '\t' => 't',
        '\u{0b}' => 'v',
        '\u{08}' => 'b',
        '\r' => 'r',
        '\u{0c}' => 'f',
        '\u{07}' => 'a',
        '\\' => '\\',
        '"' => '"',
        _ => return None,
    })
}

/// Byte offset of the closing quote in `rest` (the text after the opening quote).
pub(crate) fn quoted_end(rest: &str, escapes: bool) -> Option<usize> {
    let mut it = rest.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        match c {
            '"' => return Some(i),
            '\\' if escapes && it.peek().is_some_and(|&(_, n)| decode_char(n).is_some()) => {
                it.next();
            }
            _ => {}
        }
    }
    None
}

/// Decodes the text between quotes. `None` if it holds an unescaped closing quote.
pub(crate) fn decode(raw: &str, escapes: bool) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let mut it = raw.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' => return None,
            '\\' if escapes => match it.peek().copied().and_then(decode_char) {
                Some(d) => {
                    it.next();
                    out.push(d);
                }
                None => out.push('\\'),
            },
            c => out.push(c),
        }
    }
    Some(out)
}

/// Encodes for the inside of quotes. `None` if the text needs an escape that is switched off.
pub(crate) fn encode(s: &str, escapes: bool) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if escapes {
            match encode_char(c) {
                Some(e) => {
                    out.push('\\');
                    out.push(e);
                }
                None => out.push(c),
            }
        } else if c == '"' {
            return None;
        } else {
            out.push(c);
        }
    }
    Some(out)
}

/// ASCII whitespace as C's `isspace` sees it.
pub(crate) fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{0b}' | '\u{0c}')
}

/// Whether the text can be written as a bare token and read back unchanged.
pub(crate) fn is_bare(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('[')
        && !s.starts_with("//")
        && !s
            .chars()
            .any(|c| is_space(c) || matches!(c, '"' | '{' | '}'))
}
