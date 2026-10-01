use crate::layout::{DocumentLayout, Layout, Trivia};
use crate::{Encoding, Result, binary, text_read, text_write};

/// Default for [`Options::max_depth`].
pub const DEFAULT_MAX_DEPTH: usize = 128;

/// Knobs for reading and writing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// Reading text: decode backslash escapes in quoted strings. The choice is stored in
    /// [`Document::escapes`] and the writer follows that, so a file read with escapes off is
    /// written with escapes off. See the crate docs.
    pub escape_sequences: bool,
    /// Deepest section nesting accepted when reading and allowed when writing. The first
    /// level of sections is depth 1.
    pub max_depth: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            escape_sequences: true,
            max_depth: DEFAULT_MAX_DEPTH,
        }
    }
}

/// A KV1 file: directives plus one or more root entries.
///
/// Build one by hand with [`Document::new`]; read one with [`Document::parse`] or
/// [`Document::from_binary`]. Writing a document that was read gives back the input; a
/// hand-built one is written in Valve's conventional layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// `#include` / `#base` lines, in file order.
    pub directives: Vec<Directive>,
    /// Top-level entries, in file order. Usually one section.
    pub roots: Vec<Entry>,
    /// Text: backslash escapes are decoded when reading and encoded when writing.
    /// Valve's engine default is off; tools expect on, so hand-built documents start on.
    pub escapes: bool,
    /// How the strings map to bytes. Anything that is not valid UTF-8 is Windows-1252.
    pub encoding: Encoding,
    /// Comments, whitespace and other fidelity data. Defaults write Valve's layout.
    pub layout: DocumentLayout,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            directives: Vec::new(),
            roots: Vec::new(),
            escapes: true,
            encoding: Encoding::Utf8,
            layout: DocumentLayout::default(),
        }
    }
}

/// Which directive a [`Directive`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DirectiveKind {
    /// `#include "file"`
    Include,
    /// `#base "file"`
    Base,
}

impl DirectiveKind {
    pub(crate) fn keyword(self) -> &'static str {
        match self {
            Self::Include => "#include",
            Self::Base => "#base",
        }
    }
}

/// A `#include` or `#base` line. The path is reported as written, never opened.
///
/// Directives are only recognised at the top level of a document. Inside a section the same
/// word is an ordinary key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive {
    /// Which directive it is.
    pub kind: DirectiveKind,
    /// The file it names.
    pub path: String,
    /// Conditional tag after the path, without brackets.
    pub condition: Option<String>,
    /// How many of [`Document::roots`] come before this directive. Past the end means after
    /// the last root. New directives default to 0, ahead of the roots.
    pub before_root: usize,
    /// Keyword case and quoting, spacing, comments.
    pub layout: Layout,
}

impl Directive {
    /// A directive with the given kind and path.
    pub fn new(kind: DirectiveKind, path: impl Into<String>) -> Self {
        let mut layout = Layout::default();
        layout.key.quote = crate::Quote::Bare;
        Self {
            kind,
            path: path.into(),
            condition: None,
            before_root: 0,
            layout,
        }
    }

    /// `#include "path"`.
    pub fn include(path: impl Into<String>) -> Self {
        Self::new(DirectiveKind::Include, path)
    }

    /// `#base "path"`.
    pub fn base(path: impl Into<String>) -> Self {
        Self::new(DirectiveKind::Base, path)
    }

    /// Places the directive after the first `n` roots.
    #[must_use]
    pub fn after_roots(mut self, n: usize) -> Self {
        self.before_root = n;
        self
    }

    /// Sets the conditional tag, without brackets.
    #[must_use]
    pub fn with_condition(mut self, condition: impl Into<String>) -> Self {
        self.condition = Some(condition.into());
        self
    }
}

/// One key with its value and optional conditional tag.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The key.
    pub key: String,
    /// The value.
    pub value: Value,
    /// Conditional tag without brackets, e.g. `$WIN32` or `!$X360`. Kept, never evaluated.
    pub condition: Option<String>,
    /// Comments, spacing and quote style. The default writes Valve's layout.
    pub layout: Layout,
}

/// A value. Text KV1 has no types, so reading text gives [`Value::String`] and
/// [`Value::Section`] only. The others come from the binary form (or from hand), and writing
/// them as text is an explicit step, see [`Document::stringified`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Value {
    /// A string.
    String(String),
    /// A nested section; keys may repeat.
    Section(Vec<Entry>),
    /// Binary type 2.
    Int(i32),
    /// Binary type 3.
    Float(f32),
    /// Binary type 4, a pointer-sized value stored as 32 bits.
    Ptr(u32),
    /// Binary type 5, UTF-16 on the wire.
    WString(String),
    /// Binary type 6, red, green, blue, alpha.
    Color([u8; 4]),
    /// Binary type 7.
    UInt64(u64),
    /// Binary type 10 (signed 64-bit), as written by newer Steam tools.
    Int64(i64),
}

impl Value {
    /// The string form text uses for a typed value, as an explicit lossy conversion: the
    /// type is gone afterwards. Sections convert their children.
    #[must_use]
    pub fn stringified(&self) -> Value {
        match self {
            Value::String(_) => self.clone(),
            Value::Section(children) => {
                Value::Section(children.iter().map(Entry::stringified).collect())
            }
            Value::Int(i) => Value::String(i.to_string()),
            Value::Float(f) => Value::String(f.to_string()),
            Value::Ptr(p) => Value::String(p.to_string()),
            Value::WString(s) => Value::String(s.clone()),
            Value::Color([r, g, b, a]) => Value::String(format!("{r} {g} {b} {a}")),
            Value::UInt64(u) => Value::String(u.to_string()),
            Value::Int64(i) => Value::String(i.to_string()),
        }
    }

    pub(crate) fn kind_name(&self) -> &'static str {
        match self {
            Value::String(_) => "string",
            Value::Section(_) => "section",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Ptr(_) => "ptr",
            Value::WString(_) => "wstring",
            Value::Color(_) => "color",
            Value::UInt64(_) => "uint64",
            Value::Int64(_) => "int64",
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(s.to_owned())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(s)
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Int(v)
    }
}

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Float(v)
    }
}

impl From<u64> for Value {
    fn from(v: u64) -> Self {
        Value::UInt64(v)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int64(v)
    }
}

impl From<[u8; 4]> for Value {
    fn from(v: [u8; 4]) -> Self {
        Value::Color(v)
    }
}

impl From<Vec<Entry>> for Value {
    fn from(v: Vec<Entry>) -> Self {
        Value::Section(v)
    }
}

impl Document {
    /// A document with the given roots, no directives and conventional layout.
    pub fn new(roots: Vec<Entry>) -> Self {
        Self {
            roots,
            ..Self::default()
        }
    }

    /// Adds a directive.
    #[must_use]
    pub fn with_directive(mut self, directive: Directive) -> Self {
        self.directives.push(directive);
        self
    }

    /// Adds a `//` comment line at the top of the file. A leading space is not added.
    #[must_use]
    pub fn with_comment(mut self, text: impl Into<String>) -> Self {
        self.layout
            .leading
            .extend([Trivia::Comment(text.into()), Trivia::Newline]);
        self
    }

    /// Parses KV1 text with default [`Options`].
    pub fn parse(text: &str) -> Result<Self> {
        Self::parse_with(text, &Options::default())
    }

    /// Parses KV1 text. `options.escape_sequences` is stored in [`Document::escapes`].
    pub fn parse_with(text: &str, options: &Options) -> Result<Self> {
        text_read::parse(text, Encoding::Utf8, options)
    }

    /// Parses KV1 text from bytes with default [`Options`]. Valid UTF-8 is read as UTF-8;
    /// anything else is read as Windows-1252 and written back as such.
    pub fn parse_bytes(bytes: &[u8]) -> Result<Self> {
        Self::parse_bytes_with(bytes, &Options::default())
    }

    /// Parses KV1 text from bytes, see [`Document::parse_bytes`]. UTF-16 is an error.
    pub fn parse_bytes_with(bytes: &[u8], options: &Options) -> Result<Self> {
        text_read::parse_bytes(bytes, options)
    }

    /// Writes KV1 text with the default depth limit.
    ///
    /// Typed values ([`Value::Int`] and the rest) are an error here: text cannot hold a
    /// type, and turning it into a string is a loss. Call [`Document::stringified`] first
    /// to do that on purpose. The result includes a leading U+FEFF if the document had a
    /// byte order mark; for a document that is not UTF-8 use [`Document::to_text_bytes`].
    pub fn to_text(&self) -> Result<String> {
        self.to_text_with(&Options::default())
    }

    /// Writes KV1 text. Only `options.max_depth` applies; escapes follow [`Document::escapes`].
    pub fn to_text_with(&self, options: &Options) -> Result<String> {
        text_write::write(self, options)
    }

    /// Writes KV1 text as bytes in the document's [`Encoding`].
    pub fn to_text_bytes(&self) -> Result<Vec<u8>> {
        self.to_text_bytes_with(&Options::default())
    }

    /// Writes KV1 text as bytes, see [`Document::to_text_with`].
    pub fn to_text_bytes_with(&self, options: &Options) -> Result<Vec<u8>> {
        text_write::write_bytes(self, options)
    }

    /// Parses binary KV1 with default [`Options`].
    pub fn from_binary(data: &[u8]) -> Result<Self> {
        Self::from_binary_with(data, &Options::default())
    }

    /// Parses binary KV1. Only `max_depth` applies.
    pub fn from_binary_with(data: &[u8], options: &Options) -> Result<Self> {
        binary::read(data, options)
    }

    /// Writes binary KV1 with default [`Options`].
    pub fn to_binary(&self) -> Result<Vec<u8>> {
        self.to_binary_with(&Options::default())
    }

    /// Writes binary KV1. Fails if the tree has directives or conditions, which the format
    /// cannot hold. Only `max_depth` applies.
    pub fn to_binary_with(&self, options: &Options) -> Result<Vec<u8>> {
        binary::write(self, options)
    }

    /// A copy with every typed value turned into its text string, so it can be written as
    /// text. This loses the types: an [`Value::Int`] becomes the string `"5"`.
    #[must_use]
    pub fn stringified(&self) -> Document {
        Document {
            roots: self.roots.iter().map(Entry::stringified).collect(),
            ..self.clone()
        }
    }

    /// A copy with every comment, spelling and spacing reset to the defaults, keeping only
    /// the content: keys, values, conditions, directives and their order.
    #[must_use]
    pub fn without_layout(&self) -> Document {
        Document {
            directives: self
                .directives
                .iter()
                .map(|d| Directive {
                    layout: Layout::default(),
                    ..d.clone()
                })
                .collect(),
            roots: self.roots.iter().map(Entry::without_layout).collect(),
            layout: DocumentLayout::default(),
            ..self.clone()
        }
    }

    /// First root whose key matches, ASCII case-insensitively.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Entry> {
        find(&self.roots, key)
    }

    /// First root whose key matches, mutably.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Entry> {
        self.roots
            .iter_mut()
            .find(|e| e.key.eq_ignore_ascii_case(key))
    }
}

fn find<'a>(entries: &'a [Entry], key: &str) -> Option<&'a Entry> {
    entries.iter().find(|e| e.key.eq_ignore_ascii_case(key))
}

impl Entry {
    /// An entry with conventional layout and no conditional tag.
    pub fn new(key: impl Into<String>, value: impl Into<Value>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            condition: None,
            layout: Layout::default(),
        }
    }

    /// A string entry.
    pub fn string(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(key, Value::String(value.into()))
    }

    /// A boolean as the string `1` or `0`, the KV1 convention.
    pub fn bool(key: impl Into<String>, value: bool) -> Self {
        Self::string(key, if value { "1" } else { "0" })
    }

    /// A section entry.
    pub fn section(key: impl Into<String>, children: Vec<Entry>) -> Self {
        Self::new(key, Value::Section(children))
    }

    /// A binary `int` entry. Text cannot hold it, see [`Value`].
    pub fn int(key: impl Into<String>, value: i32) -> Self {
        Self::new(key, Value::Int(value))
    }

    /// A binary `int64` entry.
    pub fn int64(key: impl Into<String>, value: i64) -> Self {
        Self::new(key, Value::Int64(value))
    }

    /// A binary `uint64` entry.
    pub fn uint64(key: impl Into<String>, value: u64) -> Self {
        Self::new(key, Value::UInt64(value))
    }

    /// A binary `float` entry.
    pub fn float(key: impl Into<String>, value: f32) -> Self {
        Self::new(key, Value::Float(value))
    }

    /// A binary `ptr` entry.
    pub fn ptr(key: impl Into<String>, value: u32) -> Self {
        Self::new(key, Value::Ptr(value))
    }

    /// A binary `wstring` entry.
    pub fn wstring(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(key, Value::WString(value.into()))
    }

    /// A binary `color` entry, red, green, blue, alpha.
    pub fn color(key: impl Into<String>, rgba: [u8; 4]) -> Self {
        Self::new(key, Value::Color(rgba))
    }

    /// Sets the conditional tag, without brackets.
    #[must_use]
    pub fn with_condition(mut self, condition: impl Into<String>) -> Self {
        self.condition = Some(condition.into());
        self
    }

    /// Appends a child. Does nothing unless the value is a section.
    #[must_use]
    pub fn with(mut self, child: Entry) -> Self {
        self.push(child);
        self
    }

    /// Appends a child. Does nothing unless the value is a section.
    pub fn push(&mut self, child: Entry) {
        if let Value::Section(c) = &mut self.value {
            c.push(child);
        }
    }

    /// Adds a `//` comment line above the entry. A leading space is not added.
    #[must_use]
    pub fn with_comment(mut self, text: impl Into<String>) -> Self {
        self.layout.add_comment(text.into());
        self
    }

    /// Adds a `//` comment after the entry on the same line.
    #[must_use]
    pub fn with_trailing_comment(mut self, text: impl Into<String>) -> Self {
        self.layout.trailing = vec![Trivia::space(" "), Trivia::comment(text)];
        self
    }

    /// Writes the key and string value as bare tokens where the text allows it.
    #[must_use]
    pub fn bare(mut self) -> Self {
        self.layout.key.quote = crate::Quote::Bare;
        self.layout.value.quote = crate::Quote::Bare;
        self
    }

    /// A copy with every typed value turned into its text string, see
    /// [`Document::stringified`].
    #[must_use]
    pub fn stringified(&self) -> Entry {
        Entry {
            value: self.value.stringified(),
            ..self.clone()
        }
    }

    /// A copy with comments, spelling and spacing reset to the defaults.
    #[must_use]
    pub fn without_layout(&self) -> Entry {
        let value = match &self.value {
            Value::Section(c) => Value::Section(c.iter().map(Entry::without_layout).collect()),
            other => other.clone(),
        };
        Entry {
            key: self.key.clone(),
            value,
            condition: self.condition.clone(),
            layout: Layout::default(),
        }
    }

    /// The text of a string or wide-string value.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match &self.value {
            Value::String(s) | Value::WString(s) => Some(s),
            _ => None,
        }
    }

    /// The entries of a section, empty for anything else.
    #[must_use]
    pub fn children(&self) -> &[Entry] {
        match &self.value {
            Value::Section(c) => c,
            _ => &[],
        }
    }

    /// First child whose key matches, ASCII case-insensitively.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Entry> {
        find(self.children(), key)
    }

    /// First child whose key matches, mutably.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Entry> {
        match &mut self.value {
            Value::Section(c) => c.iter_mut().find(|e| e.key.eq_ignore_ascii_case(key)),
            _ => None,
        }
    }

    /// Every child whose key matches, in order.
    pub fn get_all<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a Entry> {
        self.children()
            .iter()
            .filter(move |e| e.key.eq_ignore_ascii_case(key))
    }

    /// The string value of the first matching child.
    #[must_use]
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key)?.as_str()
    }

    /// The first matching child as an integer: a decimal string, or a binary integer.
    #[must_use]
    pub fn get_int(&self, key: &str) -> Option<i64> {
        match &self.get(key)?.value {
            Value::String(s) | Value::WString(s) => s.trim().parse().ok(),
            Value::Int(i) => Some(i64::from(*i)),
            Value::Int64(i) => Some(*i),
            Value::UInt64(u) => i64::try_from(*u).ok(),
            _ => None,
        }
    }

    /// The first matching child as a float.
    #[must_use]
    pub fn get_float(&self, key: &str) -> Option<f64> {
        match &self.get(key)?.value {
            Value::String(s) | Value::WString(s) => s.trim().parse().ok(),
            Value::Float(f) => Some(f64::from(*f)),
            _ => self.get_int(key).map(|i| i as f64),
        }
    }

    /// The first matching child as a bool: `true`/`false`, or an integer where non-zero is true.
    #[must_use]
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        if let Some(s) = self.get_str(key) {
            if s.eq_ignore_ascii_case("true") {
                return Some(true);
            }
            if s.eq_ignore_ascii_case("false") {
                return Some(false);
            }
        }
        self.get_int(key).map(|i| i != 0)
    }
}
