use crate::escape;
use crate::layout::{ConditionAt, Layout, Quote, Spelling, Trivia};
use crate::{Directive, Document, Encoding, Entry, Error, Options, Result, Value};

enum GapDefault {
    Text(&'static str),
    LineIndent,
}

struct Writer {
    out: String,
    newline: &'static str,
    escapes: bool,
    max_depth: usize,
}

pub(crate) fn write(doc: &Document, options: &Options) -> Result<String> {
    let mut w = Writer {
        out: String::new(),
        newline: doc.layout.line_ending.as_str(),
        escapes: doc.escapes,
        max_depth: options.max_depth,
    };
    w.document(doc)?;
    if doc.layout.bom {
        w.out.insert(0, '\u{feff}');
    }
    Ok(w.out)
}

pub(crate) fn write_bytes(doc: &Document, options: &Options) -> Result<Vec<u8>> {
    let text = write(doc, options)?;
    match doc.encoding {
        Encoding::Utf8 => Ok(text.into_bytes()),
        Encoding::Windows1252 => doc
            .encoding
            .encode(&text)
            .map_err(|c| Error::InvalidInput(format!("{c:?} cannot be written in Windows-1252"))),
    }
}

impl Writer {
    fn ensure_newline(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push_str(self.newline);
        }
    }

    fn indent(&mut self, depth: usize) {
        for _ in 0..depth {
            self.out.push('\t');
        }
    }

    fn trivia(&mut self, items: &[Trivia], depth: usize) {
        for t in items {
            match t {
                Trivia::Space(s) => self.out.push_str(s),
                Trivia::Comment(c) => {
                    self.out.push_str("//");
                    self.out.push_str(c);
                }
                Trivia::Indent => self.indent(depth),
                Trivia::Newline => self.ensure_newline(),
            }
        }
    }

    fn gap(&mut self, slot: &Option<Vec<Trivia>>, depth: usize, default: &GapDefault) {
        match (slot, default) {
            (Some(items), _) => self.trivia(items, depth),
            (None, GapDefault::Text(s)) => self.out.push_str(s),
            (None, GapDefault::LineIndent) => {
                self.ensure_newline();
                self.indent(depth);
            }
        }
    }

    fn leading(&mut self, layout: &Layout, depth: usize) {
        match &layout.leading {
            Some(items) => self.trivia(items, depth),
            None => {
                self.ensure_newline();
                self.indent(depth);
            }
        }
    }

    fn string(&mut self, sp: &Spelling, text: &str) -> Result<()> {
        if sp.quote == Quote::Bare && escape::is_bare(text) {
            self.out.push_str(text);
            return Ok(());
        }
        self.out.push('"');
        match &sp.raw {
            Some(raw) if escape::decode(raw, self.escapes).as_deref() == Some(text) => {
                self.out.push_str(raw);
            }
            _ => {
                let enc = escape::encode(text, self.escapes).ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "{text:?} contains '\"', which needs escape sequences"
                    ))
                })?;
                self.out.push_str(&enc);
            }
        }
        self.out.push('"');
        Ok(())
    }

    fn condition(&mut self, cond: &str, raw: Option<&str>) -> Result<()> {
        if cond.contains([']', '\n']) {
            return Err(Error::InvalidInput(format!(
                "conditional tag {cond:?} contains ']' or a newline"
            )));
        }
        let inner = match raw {
            Some(r) if r.trim() == cond && !r.contains([']', '\n']) => r,
            _ => cond,
        };
        self.out.push('[');
        self.out.push_str(inner);
        self.out.push(']');
        Ok(())
    }

    fn document(&mut self, doc: &Document) -> Result<()> {
        self.trivia(&doc.layout.leading, 0);
        let mut order: Vec<usize> = (0..doc.directives.len()).collect();
        order.sort_by_key(|&i| doc.directives[i].before_root.min(doc.roots.len()));
        let mut dirs = order.into_iter().peekable();
        let mut prev_directive = false;
        for i in 0..=doc.roots.len() {
            while let Some(&d) = dirs.peek() {
                if doc.directives[d].before_root.min(doc.roots.len()) > i {
                    break;
                }
                dirs.next();
                self.directive(&doc.directives[d])?;
                prev_directive = true;
            }
            if let Some(root) = doc.roots.get(i) {
                if prev_directive && root.layout.leading.is_none() {
                    self.ensure_newline();
                    self.out.push_str(self.newline);
                }
                prev_directive = false;
                self.entry(root, 0)?;
            }
        }
        match &doc.layout.trailing {
            Some(items) => self.trivia(items, 0),
            None => self.ensure_newline(),
        }
        Ok(())
    }

    fn directive(&mut self, d: &Directive) -> Result<()> {
        let l = &d.layout;
        self.leading(l, 0);
        let canonical = d.kind.keyword();
        let keyword = match &l.key.raw {
            Some(raw) if raw.eq_ignore_ascii_case(canonical) => raw.as_str(),
            _ => canonical,
        };
        if l.key.quote == Quote::Bare {
            self.out.push_str(keyword);
        } else {
            self.out.push('"');
            self.out.push_str(keyword);
            self.out.push('"');
        }
        self.gap(&l.key_gap, 0, &GapDefault::Text(" "));
        self.string(&l.value, &d.path)?;
        if let Some(c) = &d.condition {
            self.gap(&l.condition_gap, 0, &GapDefault::Text(" "));
            self.condition(c, l.condition_raw.as_deref())?;
        }
        self.trivia(&l.trailing, 0);
        Ok(())
    }

    fn entry(&mut self, e: &Entry, depth: usize) -> Result<()> {
        let l = &e.layout;
        let children = match &e.value {
            Value::String(_) => None,
            Value::Section(c) => Some(c),
            other => {
                return Err(Error::TypedValueInText {
                    key: e.key.clone(),
                    kind: other.kind_name(),
                });
            }
        };
        if children.is_some() && depth >= self.max_depth {
            return Err(Error::TooDeep {
                limit: self.max_depth,
            });
        }
        let at = l.condition_at.unwrap_or(if children.is_some() {
            ConditionAt::AfterKey
        } else {
            ConditionAt::AfterValue
        });
        let cond = e.condition.as_deref();
        let line_default = || -> GapDefault {
            if children.is_some() {
                GapDefault::LineIndent
            } else {
                GapDefault::Text("\t\t")
            }
        };

        self.leading(l, depth);
        self.string(&l.key, &e.key)?;
        match (cond, at) {
            (Some(c), ConditionAt::AfterKey) => {
                self.gap(&l.key_gap, depth, &GapDefault::Text(" "));
                self.condition(c, l.condition_raw.as_deref())?;
                let after = if children.is_some() {
                    GapDefault::LineIndent
                } else {
                    GapDefault::Text(" ")
                };
                self.gap(&l.condition_gap, depth, &after);
            }
            _ => self.gap(&l.key_gap, depth, &line_default()),
        }
        match (&e.value, children) {
            (Value::String(s), _) => self.string(&l.value, s)?,
            (_, Some(children)) => {
                self.out.push('{');
                for c in children {
                    self.entry(c, depth + 1)?;
                }
                self.gap(&l.before_close, depth, &GapDefault::LineIndent);
                self.out.push('}');
            }
            _ => {}
        }
        if let (Some(c), ConditionAt::AfterValue) = (cond, at) {
            self.gap(&l.condition_gap, depth, &GapDefault::Text(" "));
            self.condition(c, l.condition_raw.as_deref())?;
        }
        self.trivia(&l.trailing, depth);
        Ok(())
    }
}
