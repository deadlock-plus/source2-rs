//! Text KV3 reader.

use crate::error::{Error, Result};
use crate::guid::{GENERIC_FORMAT, parse_guid};
use crate::read::MAX_DEPTH;
use crate::value::flag;
use crate::{Document, Object, Tag, TextHeader, Value, WriteOptions};

/// Parse a text KV3 document.
///
/// The `<!-- kv3 ... -->` header is optional, but one that is present must name both an
/// `encoding` and a `format`, each as `name:version{guid}`; both are kept in the returned
/// [`Document`] so [`write_text`](crate::write_text) can state them again. Without a header the
/// document is the generic format.
///
/// Flag prefixes such as `resource:"path"` set the matching [`flag`] bits on the value they
/// precede. A prefix that names no flag is an error, since keeping the value would drop the
/// prefix silently. Decimal integers are [`Kind::Int`](crate::Kind::Int) when they fit an `i64`
/// and [`Kind::UInt`](crate::Kind::UInt) above that; hexadecimal ones (`0x...`) are always
/// `UInt`, which is how [`write_text`](crate::write_text) spells an unsigned value. `"""`
/// strings read as [`Value::multiline`].
///
/// # What text drops
///
/// Comments (`//` and `/* */`) are read past and not kept, key quoting and layout are
/// regenerated on writing, and a text file has no integer widths, float widths or array layouts
/// to remember. A document read from text carries [`WriteOptions::default`] apart from the
/// format GUID.
///
/// # Errors
///
/// [`Error::Syntax`] on any syntax the reader does not accept: an unterminated string, comment,
/// object, array or blob, a stray token, an unparseable or out-of-range number (`nan` and `inf`
/// have no spelling and are refused), a malformed header, an unknown flag prefix, or nesting
/// deeper than the binary reader allows.
pub fn parse_text(input: &str) -> Result<Document> {
    let mut p = Parser {
        src: input.strip_prefix('\u{feff}').unwrap_or(input),
        pos: 0,
    };
    let (text, format) = p.header()?;
    p.skip_trivia()?;
    let root = p.value(0)?;
    p.skip_trivia()?;
    if !p.at_end() {
        return Err(p.err("unexpected content after the root value"));
    }
    Ok(Document {
        root,
        options: WriteOptions {
            format,
            ..WriteOptions::default()
        },
        text,
    })
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
}

fn is_bare_key(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'$')
}

fn is_delimiter(b: u8) -> bool {
    b.is_ascii_whitespace() || matches!(b, b',' | b']' | b'}' | b'[' | b'{' | b'=' | b'"')
}

impl<'a> Parser<'a> {
    fn err(&self, msg: &str) -> Error {
        let line = 1 + self.src.as_bytes()[..self.pos.min(self.src.len())]
            .iter()
            .filter(|&&b| b == b'\n')
            .count();
        Error::Syntax {
            line,
            message: msg.to_string(),
        }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.src.len()
    }

    fn peek(&self) -> Option<u8> {
        self.src.as_bytes().get(self.pos).copied()
    }

    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn header(&mut self) -> Result<(TextHeader, [u8; 16])> {
        let start = self.pos;
        self.skip_whitespace();
        if !self.rest().starts_with("<!--") {
            self.pos = start;
            return Ok((TextHeader::default(), GENERIC_FORMAT));
        }
        let body_start = self.pos + 4;
        let end = self
            .rest()
            .find("-->")
            .ok_or_else(|| self.err("header comment is not closed"))?;
        let body = &self.src[body_start..self.pos + end];
        let mut words = body.split_whitespace();
        if words.next() != Some("kv3") {
            return Err(self.err("header comment is not a kv3 header"));
        }
        let (mut encoding, mut format) = (None, None);
        for word in words {
            let mut parts = word.splitn(3, ':');
            let (key, name, version) = (parts.next(), parts.next(), parts.next());
            let guid = version
                .and_then(|v| v.strip_prefix("version{"))
                .and_then(|g| g.strip_suffix('}'))
                .and_then(parse_guid);
            let (Some(name), Some(guid)) = (name.filter(|n| !n.is_empty()), guid) else {
                return Err(self.err(&format!(
                    "header field {word:?} is not name:version{{guid}}"
                )));
            };
            let tag = Tag {
                name: name.to_string(),
                guid,
            };
            match key {
                Some("encoding") => encoding = Some(tag),
                Some("format") => format = Some(tag),
                _ => return Err(self.err(&format!("unknown header field {word:?}"))),
            }
        }
        let (Some(encoding), Some(format)) = (encoding, format) else {
            return Err(self.err("header must name both an encoding and a format"));
        };
        self.pos += end + 3;
        Ok((
            TextHeader {
                encoding,
                format_name: format.name,
            },
            format.guid,
        ))
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    fn skip_trivia(&mut self) -> Result<()> {
        loop {
            self.skip_whitespace();
            if self.rest().starts_with("//") {
                self.pos += self.rest().find('\n').unwrap_or(self.rest().len());
            } else if self.rest().starts_with("/*") {
                let open = self.pos;
                match self.rest()[2..].find("*/") {
                    Some(i) => self.pos += 2 + i + 2,
                    None => {
                        self.pos = open;
                        return Err(self.err("comment is not closed"));
                    }
                }
            } else {
                return Ok(());
            }
        }
    }

    fn value(&mut self, depth: u32) -> Result<Value> {
        if depth > MAX_DEPTH {
            return Err(self.err(&format!("nesting deeper than {MAX_DEPTH}")));
        }
        let flags = self.flags()?;
        let value = self.unflagged(depth)?;
        let flags = flags | value.flags();
        Ok(value.with_flags(flags))
    }

    /// Consume any `name:` prefixes in front of a value and return the flag bits they name.
    fn flags(&mut self) -> Result<u8> {
        let mut bits = 0;
        loop {
            let start = self.pos;
            while self.peek().is_some_and(is_bare_key) {
                self.pos += 1;
            }
            if self.pos == start || !self.eat(b':') {
                self.pos = start;
                return Ok(bits);
            }
            let name = &self.src[start..self.pos - 1];
            let Some((bit, _)) = flag::NAMES.iter().find(|(_, n)| *n == name) else {
                self.pos = start;
                return Err(self.err(&format!("`{name}:` is not a flag prefix")));
            };
            bits |= bit;
            self.skip_trivia()?;
        }
    }

    fn unflagged(&mut self, depth: u32) -> Result<Value> {
        match self.peek() {
            None => Err(self.err("expected a value, found the end of the input")),
            Some(b'{') => {
                self.pos += 1;
                self.object(depth)
            }
            Some(b'[') => {
                self.pos += 1;
                self.array(depth)
            }
            Some(b'#') if self.rest().starts_with("#[") => {
                self.pos += 2;
                self.blob()
            }
            Some(b'"') if self.rest().starts_with("\"\"\"") => Ok(Value::multiline(self.string()?)),
            Some(b'"') => Ok(Value::from(self.string()?)),
            Some(_) => self.word(),
        }
    }

    fn object(&mut self, depth: u32) -> Result<Value> {
        let mut object = Object::new();
        loop {
            self.skip_trivia()?;
            match self.peek() {
                None => return Err(self.err("object is not closed")),
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Value::from(object));
                }
                _ => {}
            }
            let key = self.key()?;
            self.skip_trivia()?;
            if !self.eat(b'=') {
                return Err(self.err(&format!("expected `=` after key {key:?}")));
            }
            self.skip_trivia()?;
            let value = self.value(depth + 1)?;
            object.push(key, value);
            self.skip_trivia()?;
            self.eat(b',');
        }
    }

    fn array(&mut self, depth: u32) -> Result<Value> {
        let mut items = Vec::new();
        loop {
            self.skip_trivia()?;
            match self.peek() {
                None => return Err(self.err("array is not closed")),
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Value::from(items));
                }
                _ => {}
            }
            items.push(self.value(depth + 1)?);
            self.skip_trivia()?;
            self.eat(b',');
        }
    }

    fn key(&mut self) -> Result<String> {
        if self.peek() == Some(b'"') {
            return self.string();
        }
        let start = self.pos;
        while self.peek().is_some_and(is_bare_key) {
            self.pos += 1;
        }
        if start == self.pos {
            return Err(self.err("expected a key"));
        }
        Ok(self.src[start..self.pos].to_string())
    }

    /// A quoted or triple-quoted string, positioned on the opening quote.
    fn string(&mut self) -> Result<String> {
        if self.rest().starts_with("\"\"\"") {
            return self.multiline();
        }
        let open = self.pos;
        self.pos += 1;
        let mut out = String::new();
        loop {
            let run = self
                .rest()
                .find(['"', '\\'])
                .ok_or_else(|| self.unterminated(open))?;
            out.push_str(&self.rest()[..run]);
            self.pos += run;
            if self.eat(b'"') {
                return Ok(out);
            }
            self.pos += 1;
            self.escape(&mut out, open)?;
        }
    }

    fn unterminated(&mut self, open: usize) -> Error {
        self.pos = open;
        self.err("string is not closed")
    }

    /// Decode one escape, the backslash already consumed.
    ///
    /// Unknown escapes keep their backslash: shipped data carries Windows-style paths
    /// where a strict reader would reject the whole file.
    fn escape(&mut self, out: &mut String, open: usize) -> Result<()> {
        let c = self
            .rest()
            .chars()
            .next()
            .ok_or_else(|| self.unterminated(open))?;
        self.pos += c.len_utf8();
        match c {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            '0' => out.push('\0'),
            'a' => out.push('\x07'),
            'b' => out.push('\x08'),
            'f' => out.push('\x0c'),
            'v' => out.push('\x0b'),
            '\\' | '"' | '\'' | '/' => out.push(c),
            'u' => {
                let hex = self
                    .rest()
                    .get(..4)
                    .filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))
                    .ok_or_else(|| self.err("\\u needs four hex digits"))?;
                let code = u32::from_str_radix(hex, 16).expect("four hex digits");
                let ch = char::from_u32(code)
                    .ok_or_else(|| self.err("\\u escape is not a scalar value"))?;
                out.push(ch);
                self.pos += 4;
            }
            other => {
                out.push('\\');
                out.push(other);
            }
        }
        Ok(())
    }

    /// Triple-quoted text is raw. One newline right after the opener and one right before
    /// the closer are layout, not content.
    fn multiline(&mut self) -> Result<String> {
        let open = self.pos;
        self.pos += 3;
        let len = self
            .rest()
            .find("\"\"\"")
            .ok_or_else(|| self.unterminated(open))?;
        let mut text = &self.rest()[..len];
        self.pos += len + 3;
        text = text
            .strip_prefix("\r\n")
            .or_else(|| text.strip_prefix('\n'))
            .unwrap_or(text);
        text = text
            .strip_suffix("\r\n")
            .or_else(|| text.strip_suffix('\n'))
            .unwrap_or(text);
        Ok(text.to_string())
    }

    fn blob(&mut self) -> Result<Value> {
        let mut bytes = Vec::new();
        loop {
            self.skip_trivia()?;
            match self.peek() {
                None => return Err(self.err("byte array is not closed")),
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Value::blob(bytes));
                }
                Some(_) => {}
            }
            let start = self.pos;
            while self.peek().is_some_and(|b| b.is_ascii_hexdigit()) {
                self.pos += 1;
            }
            let digits = &self.src[start..self.pos];
            let next_ends_token = self
                .peek()
                .is_none_or(|b| b.is_ascii_whitespace() || b == b']');
            if digits.is_empty() || digits.len() & 1 == 1 || !next_ends_token {
                self.pos = start;
                return Err(self.err("byte arrays hold pairs of hex digits"));
            }
            for pair in digits.as_bytes().chunks(2) {
                let pair = std::str::from_utf8(pair).expect("hex digits are ASCII");
                bytes.push(u8::from_str_radix(pair, 16).expect("checked hex digits"));
            }
        }
    }

    /// A bare token: a keyword or a number.
    fn word(&mut self) -> Result<Value> {
        let start = self.pos;
        while self.peek().is_some_and(|b| !is_delimiter(b)) {
            self.pos += 1;
        }
        let token = &self.src[start..self.pos];
        if token.is_empty() {
            return Err(self.err("unexpected character"));
        }
        match token {
            "true" => return Ok(Value::from(true)),
            "false" => return Ok(Value::from(false)),
            "null" => return Ok(Value::null()),
            _ => {}
        }
        number(token).ok_or_else(|| {
            self.pos = start;
            self.err(&format!("{token:?} is not a value"))
        })
    }
}

fn number(token: &str) -> Option<Value> {
    let (negative, digits) = match token.as_bytes().first()? {
        b'-' => (true, &token[1..]),
        b'+' => (false, &token[1..]),
        _ => (false, token),
    };
    if !digits
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_digit() || b == b'.')
    {
        return None;
    }
    if let Some(hex) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        let magnitude = u64::from_str_radix(hex, 16).ok()?;
        return if negative {
            integer(magnitude, true)
        } else {
            Some(Value::uint(magnitude))
        };
    }
    if digits.bytes().all(|b| b.is_ascii_digit()) {
        return integer(digits.parse::<u64>().ok()?, negative);
    }
    let float = token.parse::<f64>().ok()?;
    float.is_finite().then_some(Value::double(float))
}

fn integer(magnitude: u64, negative: bool) -> Option<Value> {
    if negative {
        let value = i64::try_from(i128::from(magnitude).checked_neg()?).ok()?;
        Some(Value::int(value))
    } else {
        Some(match i64::try_from(magnitude) {
            Ok(v) => Value::int(v),
            Err(_) => Value::uint(magnitude),
        })
    }
}
