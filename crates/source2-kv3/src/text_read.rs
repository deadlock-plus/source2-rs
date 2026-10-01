//! Text KV3 reader.

use crate::error::{Error, Result};
use crate::value::{MAX_DEPTH, Object, Value};

/// Parse a text KV3 document into its root value.
///
/// The `<!-- kv3 ... -->` header is optional, but one that is present must name both an
/// `encoding` and a `format`. Resource flags such as `resource:"path"` are accepted and
/// dropped, since [`Value`] has nowhere to keep them. Integers map as the binary reader
/// does: [`Value::Int`] when they fit an `i64`, [`Value::UInt`] only above that.
///
/// # Errors
///
/// On any syntax the reader does not accept: an unterminated string, comment, object,
/// array or blob, a stray token, an unparseable or out-of-range number, a malformed
/// header, or nesting deeper than the binary reader allows.
pub fn parse_text(input: &str) -> Result<Value> {
    let mut p = Parser {
        src: input.strip_prefix('\u{feff}').unwrap_or(input),
        pos: 0,
    };
    p.header()?;
    p.skip_trivia()?;
    let root = p.value(0)?;
    p.skip_trivia()?;
    if !p.at_end() {
        return Err(p.err("unexpected content after the root value"));
    }
    Ok(root)
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
        Error::Malformed(format!("text KV3 line {line}: {msg}"))
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

    fn header(&mut self) -> Result<()> {
        let start = self.pos;
        self.skip_whitespace();
        if !self.rest().starts_with("<!--") {
            self.pos = start;
            return Ok(());
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
        let (mut encoding, mut format) = (false, false);
        for word in words {
            let mut parts = word.splitn(3, ':');
            let (key, name, version) = (parts.next(), parts.next(), parts.next());
            let well_formed = name.is_some_and(|n| !n.is_empty())
                && version.is_some_and(|v| {
                    v.strip_prefix("version{")
                        .is_some_and(|g| g.ends_with('}') && g.len() > 1)
                });
            if !well_formed {
                return Err(self.err(&format!(
                    "header field {word:?} is not name:version{{guid}}"
                )));
            }
            match key {
                Some("encoding") => encoding = true,
                Some("format") => format = true,
                _ => return Err(self.err(&format!("unknown header field {word:?}"))),
            }
        }
        if !(encoding && format) {
            return Err(self.err("header must name both an encoding and a format"));
        }
        self.pos += end + 3;
        Ok(())
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
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(_) => self.word(depth),
        }
    }

    fn object(&mut self, depth: u32) -> Result<Value> {
        let mut object = Object::default();
        loop {
            self.skip_trivia()?;
            match self.peek() {
                None => return Err(self.err("object is not closed")),
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Value::Object(object));
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
            object.insert(key, value);
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
                    return Ok(Value::Array(items));
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
                    return Ok(Value::Blob(bytes));
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

    /// A bare token: a keyword, a number, or the flag in front of another value.
    fn word(&mut self, depth: u32) -> Result<Value> {
        let start = self.pos;
        while self.peek().is_some_and(|b| !is_delimiter(b)) {
            self.pos += 1;
        }
        let token = &self.src[start..self.pos];
        if token.is_empty() {
            return Err(self.err("unexpected character"));
        }
        match token {
            "true" => return Ok(Value::Bool(true)),
            "false" => return Ok(Value::Bool(false)),
            "null" => return Ok(Value::Null),
            _ => {}
        }
        if let Some(flag) = token.strip_suffix(':')
            && !flag.is_empty()
            && flag.bytes().all(is_bare_key)
        {
            self.skip_trivia()?;
            return self.value(depth);
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
        return integer(u64::from_str_radix(hex, 16).ok()?, negative);
    }
    if digits.bytes().all(|b| b.is_ascii_digit()) {
        return integer(digits.parse::<u64>().ok()?, negative);
    }
    let float = token.parse::<f64>().ok()?;
    float.is_finite().then_some(Value::Double(float))
}

fn integer(magnitude: u64, negative: bool) -> Option<Value> {
    if negative {
        let value = i64::try_from(i128::from(magnitude).checked_neg()?).ok()?;
        Some(Value::Int(value))
    } else {
        Some(match i64::try_from(magnitude) {
            Ok(v) => Value::Int(v),
            Err(_) => Value::UInt(magnitude),
        })
    }
}
