//! Everything the text and binary forms store that is not the key/value content: comments,
//! whitespace, quote style, spelling, line endings, encodings.
//!
//! Every field here is optional or defaulted. A hand-built document leaves them all at their
//! defaults and is written in Valve's conventional style; a parsed document fills them in so
//! that writing it back reproduces the input.

/// A piece of whitespace or comment between two tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Trivia {
    /// Whitespace exactly as it appeared, newlines included.
    Space(String),
    /// A `//` comment. The text is what follows the slashes, up to the line end, verbatim.
    Comment(String),
    /// The conventional indentation for the nesting depth: one tab per level.
    Indent,
    /// A line break in the document's [`LineEnding`], skipped when the output is already at the
    /// start of a line.
    Newline,
}

impl Trivia {
    /// A `//` comment with the given text. A leading space is not added.
    pub fn comment(text: impl Into<String>) -> Self {
        Self::Comment(text.into())
    }

    /// Whitespace.
    pub fn space(text: impl Into<String>) -> Self {
        Self::Space(text.into())
    }
}

/// How a string token was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Quote {
    /// `"text"`, Valve's usual form.
    #[default]
    Quoted,
    /// A bare token. Only written bare when the text allows it; otherwise it is quoted.
    Bare,
}

/// The source spelling of one string token.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Spelling {
    /// Quoted or bare.
    pub quote: Quote,
    /// The exact text between the quotes when it differs from what the writer would produce
    /// for the decoded value (for example `\?` or an unknown escape). It is used only while
    /// it still decodes to the current value, so editing the value never writes stale text.
    pub raw: Option<String>,
}

/// Where a conditional tag sits relative to its entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConditionAt {
    /// `"key" [$TAG] "value"` or `"key" [$TAG] {`.
    AfterKey,
    /// `"key" "value" [$TAG]` or `{ ... } [$TAG]`.
    AfterValue,
}

/// A text line ending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum LineEnding {
    /// `\n`.
    #[default]
    Lf,
    /// `\r\n`.
    CrLf,
}

impl LineEnding {
    /// The line ending as text.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }
}

/// How an entry (or directive) is laid out in text. `Default` writes Valve's conventional
/// layout: tab indent, `"key"<TAB><TAB>"value"`, the brace on its own line.
///
/// `None` in a trivia field means "conventional"; `Some(vec![])` means "nothing here".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Layout {
    /// Spelling of the key (the keyword for a directive).
    pub key: Spelling,
    /// Spelling of a string value (the path for a directive).
    pub value: Spelling,
    /// Exact text inside the brackets of the conditional tag when it is not just the tag.
    pub condition_raw: Option<String>,
    /// Position of the conditional tag. `None` is the conventional place: after the key for
    /// a section, after the value for a string.
    pub condition_at: Option<ConditionAt>,
    /// Everything between the previous token and this entry's key: blank lines, comments,
    /// indentation. `None` writes a line break and the conventional indent.
    pub leading: Option<Vec<Trivia>>,
    /// Between the key and what follows it (the value, the `{`, or the tag).
    pub key_gap: Option<Vec<Trivia>>,
    /// Between the conditional tag and its neighbour: after the tag when it follows the key,
    /// before it when it follows the value.
    pub condition_gap: Option<Vec<Trivia>>,
    /// Sections only: between the last child and the closing brace.
    pub before_close: Option<Vec<Trivia>>,
    /// Same-line trivia after the entry, normally a comment. Empty by default.
    pub trailing: Vec<Trivia>,
}

impl Layout {
    /// The text of every leading `//` comment, in order.
    pub fn comments(&self) -> impl Iterator<Item = &str> {
        self.leading.iter().flatten().filter_map(|t| match t {
            Trivia::Comment(c) => Some(c.as_str()),
            _ => None,
        })
    }

    /// The text of the same-line comment after the entry, if any.
    #[must_use]
    pub fn trailing_comment(&self) -> Option<&str> {
        self.trailing.iter().find_map(|t| match t {
            Trivia::Comment(c) => Some(c.as_str()),
            _ => None,
        })
    }

    pub(crate) fn add_comment(&mut self, text: String) {
        let lead = self
            .leading
            .get_or_insert_with(|| vec![Trivia::Newline, Trivia::Indent]);
        lead.extend([Trivia::Comment(text), Trivia::Newline, Trivia::Indent]);
    }
}

/// Document-wide layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentLayout {
    /// The text began with a UTF-8 byte order mark.
    pub bom: bool,
    /// Line ending used where the writer adds a line break itself. Parsing sets it from the
    /// first line break in the input.
    pub line_ending: LineEnding,
    /// Trivia before the first token: a header comment, blank lines.
    pub leading: Vec<Trivia>,
    /// Trivia after the last token. `None` ends the output with a line break.
    pub trailing: Option<Vec<Trivia>>,
    /// Binary only: the closing end marker byte was present. Some files end without it.
    pub end_marker: bool,
}

impl Default for DocumentLayout {
    fn default() -> Self {
        Self {
            bom: false,
            line_ending: LineEnding::Lf,
            leading: Vec::new(),
            trailing: None,
            end_marker: true,
        }
    }
}

impl DocumentLayout {
    /// The text of every header comment, in order.
    pub fn comments(&self) -> impl Iterator<Item = &str> {
        self.leading.iter().filter_map(|t| match t {
            Trivia::Comment(c) => Some(c.as_str()),
            _ => None,
        })
    }
}
