use crate::encoding::decode_1252;
use crate::escape::{self, is_space};
use crate::layout::{ConditionAt, DocumentLayout, Layout, LineEnding, Quote, Spelling, Trivia};
use crate::{Directive, DirectiveKind, Document, Encoding, Entry, Error, Options, Result, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Str(Spelling),
    Open,
    Close,
    Condition(Option<String>),
    Eof,
}

#[derive(Debug, Clone)]
struct Token {
    kind: Kind,
    text: String,
    trivia: Vec<Trivia>,
    line: usize,
    column: usize,
}

impl Token {
    fn unexpected(&self, expected: &'static str) -> Error {
        let found = match self.kind {
            Kind::Open => "{".to_owned(),
            Kind::Close => "}".to_owned(),
            Kind::Condition(_) => format!("[{}]", self.text),
            Kind::Str(_) => self.text.clone(),
            Kind::Eof => String::new(),
        };
        Error::UnexpectedToken {
            line: self.line,
            column: self.column,
            found,
            expected,
        }
    }

    fn spelling(&self) -> Spelling {
        match &self.kind {
            Kind::Str(s) => s.clone(),
            _ => Spelling::default(),
        }
    }
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
    line: usize,
    column: usize,
    escapes: bool,
    max_depth: usize,
    peeked: Option<Token>,
}

pub(crate) fn parse_bytes(bytes: &[u8], options: &Options) -> Result<Document> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        return Err(Error::UnsupportedEncoding("UTF-16"));
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => parse(text, Encoding::Utf8, options),
        Err(_) => parse(&decode_1252(bytes), Encoding::Windows1252, options),
    }
}

pub(crate) fn parse(text: &str, encoding: Encoding, options: &Options) -> Result<Document> {
    let (src, bom) = match text.strip_prefix('\u{feff}') {
        Some(rest) if encoding == Encoding::Utf8 => (rest, true),
        _ => (text, false),
    };
    let line_ending = match src.find('\n') {
        Some(i) if src[..i].ends_with('\r') => LineEnding::CrLf,
        _ => LineEnding::Lf,
    };
    let mut p = Parser {
        src,
        pos: 0,
        line: 1,
        column: 1,
        escapes: options.escape_sequences,
        max_depth: options.max_depth,
        peeked: None,
    };
    let doc = Document {
        escapes: options.escape_sequences,
        encoding,
        layout: DocumentLayout {
            bom,
            line_ending,
            ..DocumentLayout::default()
        },
        ..Document::default()
    };
    p.document(doc)
}

fn directive_kind(t: &Token) -> Option<DirectiveKind> {
    if !matches!(t.kind, Kind::Str(_)) {
        return None;
    }
    if t.text.eq_ignore_ascii_case("#include") {
        Some(DirectiveKind::Include)
    } else if t.text.eq_ignore_ascii_case("#base") {
        Some(DirectiveKind::Base)
    } else {
        None
    }
}

/// Splits the same-line comment off the front of `trivia`. Nothing is split unless there is
/// a comment before the first line break.
fn split_trailing(trivia: &mut Vec<Trivia>) -> Vec<Trivia> {
    let mut head = Vec::new();
    let mut rest = Vec::new();
    let mut broke = false;
    for (i, t) in trivia.iter().enumerate() {
        if let Trivia::Space(s) = t
            && let Some(p) = s.find(['\r', '\n'])
        {
            if p > 0 {
                head.push(Trivia::Space(s[..p].to_owned()));
            }
            rest.push(Trivia::Space(s[p..].to_owned()));
            rest.extend_from_slice(&trivia[i + 1..]);
            broke = true;
            break;
        }
        head.push(t.clone());
    }
    if !head.iter().any(|t| matches!(t, Trivia::Comment(_))) {
        return Vec::new();
    }
    *trivia = if broke { rest } else { Vec::new() };
    head
}

impl Parser<'_> {
    fn document(&mut self, mut doc: Document) -> Result<Document> {
        let mut first = true;
        loop {
            let mut t = self.next()?;
            if matches!(t.kind, Kind::Eof) {
                doc.layout.trailing = Some(t.trivia);
                return Ok(doc);
            }
            if first {
                doc.layout.leading = std::mem::take(&mut t.trivia);
                first = false;
            }
            if !matches!(t.kind, Kind::Str(_)) {
                return Err(t.unexpected("a key"));
            }
            if let Some(kind) = directive_kind(&t) {
                let d = self.directive(t, kind, doc.roots.len())?;
                doc.directives.push(d);
            } else {
                doc.roots.push(self.entry(t, 0)?);
            }
        }
    }

    fn directive(
        &mut self,
        kw: Token,
        kind: DirectiveKind,
        before_root: usize,
    ) -> Result<Directive> {
        let path = self.next()?;
        if matches!(path.kind, Kind::Eof) {
            return Err(Error::UnexpectedEof {
                line: self.line,
                expected: "a file path",
            });
        }
        let Kind::Str(path_spelling) = &path.kind else {
            return Err(path.unexpected("a file path"));
        };
        let mut keyword = kw.spelling();
        keyword.raw = (kw.text != kind.keyword()).then(|| kw.text.clone());
        let mut layout = Layout {
            key: keyword,
            value: path_spelling.clone(),
            leading: Some(kw.trivia),
            key_gap: Some(path.trivia),
            ..Layout::default()
        };
        let condition = self.trailing_condition(&mut layout)?;
        layout.trailing = self.take_trailing()?;
        Ok(Directive {
            kind,
            path: path.text,
            condition,
            before_root,
            layout,
        })
    }

    fn trailing_condition(&mut self, layout: &mut Layout) -> Result<Option<String>> {
        self.fill_peek()?;
        if !matches!(
            self.peeked.as_ref().map(|t| &t.kind),
            Some(Kind::Condition(_))
        ) {
            return Ok(None);
        }
        let c = self.next()?;
        layout.condition_at = Some(ConditionAt::AfterValue);
        layout.condition_gap = Some(c.trivia);
        if let Kind::Condition(raw) = c.kind {
            layout.condition_raw = raw;
        }
        Ok(Some(c.text))
    }

    fn entry(&mut self, key: Token, depth: usize) -> Result<Entry> {
        let mut layout = Layout {
            key: key.spelling(),
            leading: Some(key.trivia),
            ..Layout::default()
        };
        let mut condition = None;
        let mut t = self.next()?;
        if let Kind::Condition(raw) = &t.kind {
            condition = Some(t.text.clone());
            layout.condition_raw = raw.clone();
            layout.condition_at = Some(ConditionAt::AfterKey);
            layout.key_gap = Some(std::mem::take(&mut t.trivia));
            t = self.next()?;
            layout.condition_gap = Some(std::mem::take(&mut t.trivia));
        } else {
            layout.key_gap = Some(std::mem::take(&mut t.trivia));
        }
        let value = match &t.kind {
            Kind::Str(sp) => {
                layout.value = sp.clone();
                Value::String(t.text)
            }
            Kind::Open => Value::Section(self.section(depth + 1, &mut layout)?),
            Kind::Eof => {
                return Err(Error::UnexpectedEof {
                    line: self.line,
                    expected: "a value or '{'",
                });
            }
            _ => return Err(t.unexpected("a value or '{'")),
        };
        if condition.is_none() {
            condition = self.trailing_condition(&mut layout)?;
        }
        layout.trailing = self.take_trailing()?;
        Ok(Entry {
            key: key.text,
            value,
            condition,
            layout,
        })
    }

    fn section(&mut self, depth: usize, layout: &mut Layout) -> Result<Vec<Entry>> {
        if depth > self.max_depth {
            return Err(Error::TooDeep {
                limit: self.max_depth,
            });
        }
        let mut entries = Vec::new();
        loop {
            let t = self.next()?;
            match t.kind {
                Kind::Eof => {
                    return Err(Error::UnexpectedEof {
                        line: self.line,
                        expected: "'}'",
                    });
                }
                Kind::Close => {
                    layout.before_close = Some(t.trivia);
                    return Ok(entries);
                }
                Kind::Str(_) => entries.push(self.entry(t, depth)?),
                _ => return Err(t.unexpected("a key or '}'")),
            }
        }
    }

    fn take_trailing(&mut self) -> Result<Vec<Trivia>> {
        self.fill_peek()?;
        Ok(self
            .peeked
            .as_mut()
            .map(|t| split_trailing(&mut t.trivia))
            .unwrap_or_default())
    }

    fn fill_peek(&mut self) -> Result<()> {
        if self.peeked.is_none() {
            self.peeked = Some(self.lex()?);
        }
        Ok(())
    }

    fn next(&mut self) -> Result<Token> {
        match self.peeked.take() {
            Some(t) => Ok(t),
            None => self.lex(),
        }
    }

    fn cur(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.cur()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(c)
    }

    fn advance_to(&mut self, target: usize) {
        while self.pos < target {
            self.bump();
        }
    }

    fn trivia(&mut self) -> Vec<Trivia> {
        let src = self.src;
        let mut items = Vec::new();
        while let Some(c) = self.cur() {
            let start = self.pos;
            if is_space(c) {
                while self.cur().is_some_and(is_space) {
                    self.bump();
                }
                items.push(Trivia::Space(src[start..self.pos].to_owned()));
            } else if src[start..].starts_with("//") {
                let body = start + 2;
                let bytes = src.as_bytes();
                let mut end = body;
                while end < bytes.len()
                    && bytes[end] != b'\n'
                    && !(bytes[end] == b'\r' && bytes.get(end + 1) == Some(&b'\n'))
                {
                    end += 1;
                }
                items.push(Trivia::Comment(src[body..end].to_owned()));
                self.advance_to(end);
            } else {
                break;
            }
        }
        items
    }

    fn lex(&mut self) -> Result<Token> {
        let trivia = self.trivia();
        let (line, column) = (self.line, self.column);
        let src = self.src;
        let make = |kind, text: String| Token {
            kind,
            text,
            trivia,
            line,
            column,
        };
        let Some(c) = self.cur() else {
            return Ok(make(Kind::Eof, String::new()));
        };
        match c {
            '{' => {
                self.bump();
                Ok(make(Kind::Open, String::new()))
            }
            '}' => {
                self.bump();
                Ok(make(Kind::Close, String::new()))
            }
            '"' => {
                self.bump();
                let rest = &src[self.pos..];
                let Some(end) = escape::quoted_end(rest, self.escapes) else {
                    return Err(Error::UnterminatedString { line, column });
                };
                let body = &rest[..end];
                let text = escape::decode(body, self.escapes).unwrap_or_else(|| body.to_owned());
                let canonical = escape::encode(&text, self.escapes);
                let raw = (canonical.as_deref() != Some(body)).then(|| body.to_owned());
                self.advance_to(self.pos + end);
                self.bump();
                Ok(make(
                    Kind::Str(Spelling {
                        quote: Quote::Quoted,
                        raw,
                    }),
                    text,
                ))
            }
            '[' => {
                self.bump();
                let start = self.pos;
                while let Some(c) = self.cur() {
                    if c == ']' || c == '\n' {
                        break;
                    }
                    self.bump();
                }
                if self.cur() != Some(']') {
                    return Err(Error::UnterminatedCondition { line, column });
                }
                let inner = &src[start..self.pos];
                let text = inner.trim().to_owned();
                let raw = (inner != text).then(|| inner.to_owned());
                self.bump();
                Ok(make(Kind::Condition(raw), text))
            }
            _ => {
                let start = self.pos;
                while let Some(c) = self.cur() {
                    if is_space(c) || matches!(c, '"' | '{' | '}') {
                        break;
                    }
                    self.bump();
                }
                Ok(make(
                    Kind::Str(Spelling {
                        quote: Quote::Bare,
                        raw: None,
                    }),
                    src[start..self.pos].to_owned(),
                ))
            }
        }
    }
}
