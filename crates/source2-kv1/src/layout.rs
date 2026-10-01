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
#[non_exhaustive]
pub struct Spelling {
    /// Quoted or bare.
    pub quote: Quote,
    /// The exact text between the quotes when it differs from what the writer would produce
    /// for the decoded value (for example `\?` or an unknown escape). It is used only while
    /// it still decodes to the current value, so editing the value never writes stale text.
    pub raw: Option<String>,
}

impl Spelling {
    /// The conventional spelling: quoted, no raw text.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets quoted or bare.
    #[must_use]
    pub fn with_quote(mut self, quote: Quote) -> Self {
        self.quote = quote;
        self
    }

    /// Sets the raw text between the quotes.
    #[must_use]
    pub fn with_raw(mut self, raw: impl Into<String>) -> Self {
        self.raw = Some(raw.into());
        self
    }
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
#[non_exhaustive]
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
    /// The conventional layout, same as [`Layout::default`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the key spelling.
    #[must_use]
    pub fn with_key(mut self, key: Spelling) -> Self {
        self.key = key;
        self
    }

    /// Sets the value spelling.
    #[must_use]
    pub fn with_value(mut self, value: Spelling) -> Self {
        self.value = value;
        self
    }

    /// Sets the exact text inside the conditional tag's brackets.
    #[must_use]
    pub fn with_condition_raw(mut self, condition_raw: impl Into<String>) -> Self {
        self.condition_raw = Some(condition_raw.into());
        self
    }

    /// Sets the position of the conditional tag.
    #[must_use]
    pub fn with_condition_at(mut self, condition_at: ConditionAt) -> Self {
        self.condition_at = Some(condition_at);
        self
    }

    /// Sets the trivia before the key; an empty list means nothing.
    #[must_use]
    pub fn with_leading(mut self, leading: Vec<Trivia>) -> Self {
        self.leading = Some(leading);
        self
    }

    /// Sets the trivia after the key.
    #[must_use]
    pub fn with_key_gap(mut self, key_gap: Vec<Trivia>) -> Self {
        self.key_gap = Some(key_gap);
        self
    }

    /// Sets the trivia beside the conditional tag.
    #[must_use]
    pub fn with_condition_gap(mut self, condition_gap: Vec<Trivia>) -> Self {
        self.condition_gap = Some(condition_gap);
        self
    }

    /// Sets the trivia before a section's closing brace.
    #[must_use]
    pub fn with_before_close(mut self, before_close: Vec<Trivia>) -> Self {
        self.before_close = Some(before_close);
        self
    }

    /// Sets the same-line trivia after the entry.
    #[must_use]
    pub fn with_trailing(mut self, trailing: Vec<Trivia>) -> Self {
        self.trailing = trailing;
        self
    }

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
#[non_exhaustive]
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
    /// Text only: the input ended with a single NUL byte after the last token, as Valve's
    /// tools write it.
    pub trailing_nul: bool,
}

impl Default for DocumentLayout {
    fn default() -> Self {
        Self {
            bom: false,
            line_ending: LineEnding::Lf,
            leading: Vec::new(),
            trailing: None,
            end_marker: true,
            trailing_nul: false,
        }
    }
}

impl DocumentLayout {
    /// The conventional layout, same as [`DocumentLayout::default`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets whether the text begins with a UTF-8 byte order mark.
    #[must_use]
    pub fn with_bom(mut self, bom: bool) -> Self {
        self.bom = bom;
        self
    }

    /// Sets the line ending.
    #[must_use]
    pub fn with_line_ending(mut self, line_ending: LineEnding) -> Self {
        self.line_ending = line_ending;
        self
    }

    /// Sets the trivia before the first token.
    #[must_use]
    pub fn with_leading(mut self, leading: Vec<Trivia>) -> Self {
        self.leading = leading;
        self
    }

    /// Sets the trivia after the last token; an empty list means nothing.
    #[must_use]
    pub fn with_trailing(mut self, trailing: Vec<Trivia>) -> Self {
        self.trailing = Some(trailing);
        self
    }

    /// Sets whether binary output ends with the end marker byte.
    #[must_use]
    pub fn with_end_marker(mut self, end_marker: bool) -> Self {
        self.end_marker = end_marker;
        self
    }

    /// Sets whether text output ends with a NUL byte.
    #[must_use]
    pub fn with_trailing_nul(mut self, trailing_nul: bool) -> Self {
        self.trailing_nul = trailing_nul;
        self
    }

    /// The text of every header comment, in order.
    pub fn comments(&self) -> impl Iterator<Item = &str> {
        self.leading.iter().filter_map(|t| match t {
            Trivia::Comment(c) => Some(c.as_str()),
            _ => None,
        })
    }
}
