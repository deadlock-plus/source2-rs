use crate::{Directive, DirectiveKind, Document, Entry, Error, Options, Result, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Str,
    Open,
    Close,
    Condition,
}

#[derive(Debug, Clone)]
struct Token {
    kind: Kind,
    text: String,
    line: usize,
    column: usize,
}

impl Token {
    fn unexpected(&self, expected: &'static str) -> Error {
        let found = match self.kind {
            Kind::Open => "{".to_owned(),
            Kind::Close => "}".to_owned(),
            Kind::Condition => format!("[{}]", self.text),
            Kind::Str => self.text.clone(),
        };
        Error::UnexpectedToken {
            line: self.line,
            column: self.column,
            found,
            expected,
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
    peeked: Option<Option<Token>>,
}

pub(crate) fn parse(text: &str, options: &Options) -> Result<Document> {
    let src = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut p = Parser {
        src,
        pos: 0,
        line: 1,
        column: 1,
        escapes: options.escape_sequences,
        max_depth: options.max_depth,
        peeked: None,
    };
    p.document()
}

fn directive_kind(t: &Token) -> Option<DirectiveKind> {
    if !matches!(t.kind, Kind::Str) {
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

impl Parser<'_> {
    fn document(&mut self) -> Result<Document> {
        let mut doc = Document::default();
        while let Some(t) = self.next()? {
            match t.kind {
                Kind::Str => {
                    if let Some(kind) = directive_kind(&t) {
                        match self.next()? {
                            Some(Token {
                                kind: Kind::Str,
                                text: path,
                                ..
                            }) => doc.directives.push(Directive { kind, path }),
                            Some(other) => return Err(other.unexpected("a file path")),
                            None => {
                                return Err(Error::UnexpectedEof {
                                    line: self.line,
                                    expected: "a file path",
                                });
                            }
                        }
                        continue;
                    }
                    doc.roots.push(self.entry(t, 0)?);
                }
                _ => return Err(t.unexpected("a key")),
            }
        }
        Ok(doc)
    }

    fn entry(&mut self, key: Token, depth: usize) -> Result<Entry> {
        let mut condition = None;
        let mut t = self.next()?;
        if let Some(Token {
            kind: Kind::Condition,
            text,
            ..
        }) = &t
        {
            condition = Some(text.clone());
            t = self.next()?;
        }
        let Some(t) = t else {
            return Err(Error::UnexpectedEof {
                line: self.line,
                expected: "a value or '{'",
            });
        };
        let value = match t.kind {
            Kind::Str => Value::String(t.text),
            Kind::Open => Value::Section(self.section(depth + 1)?),
            _ => return Err(t.unexpected("a value or '{'")),
        };
        if condition.is_none()
            && let Some(Token {
                kind: Kind::Condition,
                ..
            }) = self.peek()?
        {
            condition = self.next()?.map(|c| c.text);
        }
        Ok(Entry {
            key: key.text,
            value,
            condition,
        })
    }

    fn section(&mut self, depth: usize) -> Result<Vec<Entry>> {
        if depth > self.max_depth {
            return Err(Error::TooDeep {
                limit: self.max_depth,
            });
        }
        let mut entries = Vec::new();
        loop {
            let Some(t) = self.next()? else {
                return Err(Error::UnexpectedEof {
                    line: self.line,
                    expected: "'}'",
                });
            };
            match t.kind {
                Kind::Close => return Ok(entries),
                Kind::Str => entries.push(self.entry(t, depth)?),
                _ => return Err(t.unexpected("a key or '}'")),
            }
        }
    }

    fn peek(&mut self) -> Result<Option<&Token>> {
        if self.peeked.is_none() {
            self.peeked = Some(self.lex()?);
        }
        Ok(self.peeked.as_ref().and_then(Option::as_ref))
    }

    fn next(&mut self) -> Result<Option<Token>> {
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

    fn skip_trivia(&mut self) {
        while let Some(c) = self.cur() {
            if c.is_whitespace() {
                self.bump();
            } else if self.src[self.pos..].starts_with("//") {
                while let Some(c) = self.cur() {
                    if c == '\n' {
                        break;
                    }
                    self.bump();
                }
            } else {
                break;
            }
        }
    }

    fn lex(&mut self) -> Result<Option<Token>> {
        self.skip_trivia();
        let (line, column) = (self.line, self.column);
        let Some(c) = self.cur() else {
            return Ok(None);
        };
        let make = |kind, text: String| {
            Ok(Some(Token {
                kind,
                text,
                line,
                column,
            }))
        };
        match c {
            '{' => {
                self.bump();
                make(Kind::Open, String::new())
            }
            '}' => {
                self.bump();
                make(Kind::Close, String::new())
            }
            '"' => {
                self.bump();
                let text = self.quoted(line, column)?;
                make(Kind::Str, text)
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
                let text = self.src[start..self.pos].trim().to_owned();
                self.bump();
                make(Kind::Condition, text)
            }
            _ => {
                let start = self.pos;
                while let Some(c) = self.cur() {
                    if c.is_whitespace() || matches!(c, '"' | '{' | '}') {
                        break;
                    }
                    self.bump();
                }
                make(Kind::Str, self.src[start..self.pos].to_owned())
            }
        }
    }

    fn quoted(&mut self, line: usize, column: usize) -> Result<String> {
        let mut out = String::new();
        loop {
            let Some(c) = self.bump() else {
                return Err(Error::UnterminatedString { line, column });
            };
            match c {
                '"' => return Ok(out),
                '\\' if self.escapes => match self.cur() {
                    Some('n') => {
                        self.bump();
                        out.push('\n');
                    }
                    Some('t') => {
                        self.bump();
                        out.push('\t');
                    }
                    Some(e @ ('\\' | '"')) => {
                        self.bump();
                        out.push(e);
                    }
                    _ => out.push('\\'),
                },
                c => out.push(c),
            }
        }
    }
}
