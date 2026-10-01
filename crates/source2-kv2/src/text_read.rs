//! `KeyValues2` text reader.

use crate::value_text::{parse_type, parse_value};
use crate::{
    Attribute, Document, Element, ElementId, ElementRef, Encoding, Error, ReadOptions, Result,
    Uuid, Value, ValueType,
};
use std::collections::HashMap;

/// Deepest element nesting accepted. Real documents nest a few dozen levels at most; the
/// cap exists so hostile input cannot exhaust the stack.
pub(crate) const MAX_DEPTH: usize = 256;

const PREFIX_CLASS: &str = "$prefix_element$";

#[derive(Debug, PartialEq)]
enum Tok {
    Str(String),
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Eof,
}

struct Lexer<'a> {
    b: &'a [u8],
    pos: usize,
    line: usize,
}

impl<'a> Lexer<'a> {
    fn err(&self, line: usize, message: impl Into<String>) -> Error {
        Error::Syntax {
            line,
            message: message.into(),
        }
    }

    fn skip_trivia(&mut self) -> Result<()> {
        while let Some(&c) = self.b.get(self.pos) {
            match c {
                b'\n' => {
                    self.line += 1;
                    self.pos += 1;
                }
                b' ' | b'\t' | b'\r' => self.pos += 1,
                b'/' if self.b.get(self.pos + 1) == Some(&b'/') => {
                    while self.b.get(self.pos).is_some_and(|&c| c != b'\n') {
                        self.pos += 1;
                    }
                }
                b'/' if self.b.get(self.pos + 1) == Some(&b'*') => {
                    let start = self.line;
                    self.pos += 2;
                    loop {
                        match self.b.get(self.pos) {
                            None => return Err(self.err(start, "unterminated comment")),
                            Some(b'*') if self.b.get(self.pos + 1) == Some(&b'/') => {
                                self.pos += 2;
                                break;
                            }
                            Some(b'\n') => {
                                self.line += 1;
                                self.pos += 1;
                            }
                            Some(_) => self.pos += 1,
                        }
                    }
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn next(&mut self) -> Result<(Tok, usize)> {
        self.skip_trivia()?;
        let line = self.line;
        let Some(&c) = self.b.get(self.pos) else {
            return Ok((Tok::Eof, line));
        };
        let tok = match c {
            b'{' => Tok::LBrace,
            b'}' => Tok::RBrace,
            b'[' => Tok::LBracket,
            b']' => Tok::RBracket,
            b',' => Tok::Comma,
            b'"' => {
                self.pos += 1;
                return Ok((Tok::Str(self.string(line)?), line));
            }
            other => {
                return Err(self.err(line, format!("unexpected byte {other:#04x}")));
            }
        };
        self.pos += 1;
        Ok((tok, line))
    }

    fn string(&mut self, start_line: usize) -> Result<String> {
        let mut out = Vec::new();
        loop {
            let Some(&c) = self.b.get(self.pos) else {
                return Err(self.err(start_line, "unterminated string"));
            };
            self.pos += 1;
            match c {
                b'"' => break,
                b'\n' => {
                    self.line += 1;
                    out.push(c);
                }
                b'\\' => {
                    let Some(&e) = self.b.get(self.pos) else {
                        return Err(self.err(start_line, "unterminated string"));
                    };
                    self.pos += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'v' => out.push(0x0b),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0c),
                        b'a' => out.push(0x07),
                        b'\\' | b'"' | b'\'' | b'?' => out.push(e),
                        b'\n' => {
                            self.line += 1;
                            out.extend_from_slice(b"\\\n");
                        }
                        other => {
                            out.push(b'\\');
                            out.push(other);
                        }
                    }
                }
                _ => out.push(c),
            }
        }
        String::from_utf8(out).map_err(|_| self.err(start_line, "string is not valid UTF-8"))
    }
}

struct Parser<'a> {
    lex: Lexer<'a>,
    peeked: Option<(Tok, usize)>,
    doc: Document,
    ids: HashMap<Uuid, ElementId>,
    noids: bool,
    generated: u64,
}

struct Body {
    id: Option<Uuid>,
    name: Option<String>,
    attrs: Vec<Attribute>,
}

impl Parser<'_> {
    fn next(&mut self) -> Result<(Tok, usize)> {
        match self.peeked.take() {
            Some(t) => Ok(t),
            None => self.lex.next(),
        }
    }

    fn peek(&mut self) -> Result<&Tok> {
        if self.peeked.is_none() {
            self.peeked = Some(self.lex.next()?);
        }
        Ok(&self.peeked.as_ref().expect("filled above").0)
    }

    fn syntax(&self, line: usize, message: impl Into<String>) -> Error {
        self.lex.err(line, message)
    }

    fn describe(tok: &Tok) -> &'static str {
        match tok {
            Tok::Str(_) => "a string",
            Tok::LBrace => "`{`",
            Tok::RBrace => "`}`",
            Tok::LBracket => "`[`",
            Tok::RBracket => "`]`",
            Tok::Comma => "`,`",
            Tok::Eof => "end of input",
        }
    }

    fn expect(&mut self, want: &Tok) -> Result<()> {
        let (tok, line) = self.next()?;
        if tok == *want {
            Ok(())
        } else {
            Err(self.syntax(
                line,
                format!(
                    "expected {}, found {}",
                    Self::describe(want),
                    Self::describe(&tok)
                ),
            ))
        }
    }

    fn string(&mut self) -> Result<(String, usize)> {
        match self.next()? {
            (Tok::Str(s), line) => Ok((s, line)),
            (tok, line) => Err(self.syntax(
                line,
                format!("expected a string, found {}", Self::describe(&tok)),
            )),
        }
    }

    fn document(&mut self) -> Result<()> {
        loop {
            match self.next()? {
                (Tok::Eof, _) => return Ok(()),
                (Tok::Str(class), _) => {
                    self.expect(&Tok::LBrace)?;
                    if class == PREFIX_CLASS {
                        let body = self.body(0)?;
                        if body.attrs.iter().any(|a| match &a.value {
                            Value::Element(_) => true,
                            Value::Array(t, _) => *t == ValueType::Element,
                            _ => false,
                        }) {
                            return Err(
                                self.syntax(self.lex.line, "prefix elements hold no elements")
                            );
                        }
                        self.doc.prefix.push(body.attrs);
                    } else {
                        self.element(class, 0)?;
                    }
                }
                (tok, line) => {
                    return Err(self.syntax(
                        line,
                        format!("expected an element class, found {}", Self::describe(&tok)),
                    ));
                }
            }
        }
    }

    /// Parses an element whose class and opening brace are already consumed. The slot is
    /// reserved first so ids follow document order.
    fn element(&mut self, class: String, depth: usize) -> Result<ElementId> {
        if depth > MAX_DEPTH {
            return Err(Error::TooDeep);
        }
        let idx = self.doc.add_element(Element::new(class, "", Uuid::NIL));
        let body = self.body(depth)?;
        let id = match body.id {
            Some(id) => id,
            None if self.noids => {
                self.generated += 1;
                let mut b = [0u8; 16];
                b[0..4].copy_from_slice(b"noid");
                b[8..16].copy_from_slice(&self.generated.to_be_bytes());
                Uuid(b)
            }
            None => return Err(self.syntax(self.lex.line, "element has no `id`")),
        };
        if self.ids.insert(id, idx).is_some() {
            return Err(Error::DuplicateId(id));
        }
        let e = &mut self.doc.elements[idx.0 as usize];
        e.id = id;
        e.name = body.name.unwrap_or_default();
        e.attributes = body.attrs;
        Ok(idx)
    }

    fn body(&mut self, depth: usize) -> Result<Body> {
        let mut body = Body {
            id: None,
            name: None,
            attrs: Vec::new(),
        };
        loop {
            let key = match self.next()? {
                (Tok::RBrace, _) => return Ok(body),
                (Tok::Str(k), _) => k,
                (tok, line) => {
                    return Err(self.syntax(
                        line,
                        format!(
                            "expected an attribute name or `}}`, found {}",
                            Self::describe(&tok)
                        ),
                    ));
                }
            };
            let (ty, ty_line) = self.string()?;
            if key == "id" && ty == "elementid" {
                let (s, line) = self.string()?;
                body.id = Some(
                    Uuid::parse(&s)
                        .ok_or_else(|| self.syntax(line, format!("`{s}` is not an id")))?,
                );
                continue;
            }
            let value = match parse_type(&ty) {
                Some((t, false)) => {
                    let (s, line) = self.string()?;
                    parse_value(t, &s).map_err(|m| self.syntax(line, m))?
                }
                Some((t, true)) => self.array(t, depth)?,
                None => {
                    if ty.is_empty() {
                        return Err(self.syntax(ty_line, "empty type"));
                    }
                    self.expect(&Tok::LBrace)?;
                    let id = self.element(ty, depth + 1)?;
                    Value::Element(ElementRef::Element(id))
                }
            };
            match value {
                Value::String(s) if key == "name" && body.name.is_none() => body.name = Some(s),
                value => body.attrs.push(Attribute::new(key, value)),
            }
        }
    }

    fn array(&mut self, t: ValueType, depth: usize) -> Result<Value> {
        self.expect(&Tok::LBracket)?;
        let mut items = Vec::new();
        loop {
            if *self.peek()? == Tok::RBracket {
                self.next()?;
                return Ok(Value::Array(t, items));
            }
            let (s, line) = self.string()?;
            let item = if t == ValueType::Element && s != "element" {
                if s.is_empty() {
                    return Err(self.syntax(line, "empty element class"));
                }
                self.expect(&Tok::LBrace)?;
                Value::Element(ElementRef::Element(self.element(s, depth + 1)?))
            } else if t == ValueType::Element {
                let (id, line) = self.string()?;
                parse_value(t, &id).map_err(|m| self.syntax(line, m))?
            } else {
                parse_value(t, &s).map_err(|m| self.syntax(line, m))?
            };
            items.push(item);
            match self.next()? {
                (Tok::Comma, _) => {}
                (Tok::RBracket, _) => return Ok(Value::Array(t, items)),
                (tok, line) => {
                    return Err(self.syntax(
                        line,
                        format!("expected `,` or `]`, found {}", Self::describe(&tok)),
                    ));
                }
            }
        }
    }

    fn resolve(&mut self, allow_unresolved: bool) -> Result<()> {
        let ids = &self.ids;
        let fix = |v: &mut Value| -> Result<()> {
            if let Value::Element(r) = v
                && let ElementRef::External(u) = *r
            {
                match ids.get(&u) {
                    Some(&id) => *r = ElementRef::Element(id),
                    None if allow_unresolved => {}
                    None => return Err(Error::UnresolvedReference(u)),
                }
            }
            Ok(())
        };
        for e in &mut self.doc.elements {
            for a in &mut e.attributes {
                match &mut a.value {
                    Value::Array(ValueType::Element, items) => {
                        for item in items {
                            fix(item)?;
                        }
                    }
                    v => fix(v)?,
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn parse(doc: Document, body: &[u8], opts: &ReadOptions) -> Result<Document> {
    let noids = doc.encoding == Encoding::KeyValues2NoIds;
    let mut p = Parser {
        lex: Lexer {
            b: body,
            pos: 0,
            line: 2,
        },
        peeked: None,
        doc,
        ids: HashMap::new(),
        noids,
        generated: 0,
    };
    p.document()?;
    p.resolve(opts.allow_unresolved)?;
    Ok(p.doc)
}
