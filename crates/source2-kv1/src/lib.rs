//! Reader and writer for Valve's KeyValues 1 (KV1), text and binary.
//!
//! KV1 is the older Valve key/value format: `gameinfo.gi`, `.vcfg` and `.vdf` files. It is an
//! ordered tree. Every [`Entry`] has a key and either a string or a nested section, and
//! keys may repeat.
//!
//! ```no_run
//! use source2_kv1::Document;
//!
//! let doc = Document::parse(&std::fs::read_to_string("gameinfo.gi")?)?;
//! let game = doc.get("GameInfo").and_then(|e| e.get_str("game"));
//! println!("{}", doc.to_text()?);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Escape sequences
//!
//! Valve's parser only honours escapes when a caller opts in with
//! `KeyValues::UsesEscapeSequences`, and the engine default is off. Tools and Steam treat
//! backslashes as escapes, so [`Options::escape_sequences`] defaults to **on**. When on,
//! `\n`, `\t`, `\\` and `\"` are decoded and any other `\x` is kept verbatim, backslash
//! included, so Windows paths survive. When off, a backslash is an ordinary character and a
//! value cannot contain `"`.
//!
//! # Other choices
//!
//! - Keys are matched ASCII case-insensitively by the accessors, as Valve does. The tree
//!   itself keeps keys exactly as written.
//! - `#include` and `#base` at the top level become [`Directive`]s. Nothing is read from disk.
//!   Their position relative to the roots is not kept; the writer emits them first.
//! - A conditional tag such as `[$WIN32]` or `[!$X360]` is stored without its brackets and is
//!   never evaluated. It is accepted after the key, or after the value or closing brace.
//! - `/* */` comments are not part of KV1 and are not supported.
//! - The binary form has no directives or conditions and carries typed values, see [`Value`].
//!   The appinfo variant that replaces key names with string-table indices is not supported.

#![forbid(unsafe_code)]

mod binary;
pub mod error;
mod text_read;
mod text_write;

#[cfg(test)]
mod binary_tests;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod text_read_tests;
#[cfg(test)]
mod text_write_tests;

pub use error::{Error, Result};

/// Default for [`Options::max_depth`].
pub const DEFAULT_MAX_DEPTH: usize = 128;

/// Knobs shared by the readers and writers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// Decode and emit `\n \t \\ \"` in quoted text. See the crate docs.
    pub escape_sequences: bool,
    /// Deepest section nesting accepted. The first level of sections is depth 1.
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

/// A parsed KV1 file: directives plus one or more root entries.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Document {
    /// `#include` / `#base` lines, in file order.
    pub directives: Vec<Directive>,
    /// Top-level entries, in file order. Usually one section.
    pub roots: Vec<Entry>,
}

/// Which directive a [`Directive`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectiveKind {
    /// `#include "file"`
    Include,
    /// `#base "file"`
    Base,
}

/// A `#include` or `#base` line. The path is reported as written, never opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive {
    /// Which directive it is.
    pub kind: DirectiveKind,
    /// The file it names.
    pub path: String,
}

/// One key with its value and optional conditional tag.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The key, as written.
    pub key: String,
    /// The value.
    pub value: Value,
    /// Conditional tag without brackets, e.g. `$WIN32` or `!$X360`.
    pub condition: Option<String>,
}

/// A value. Text KV1 only produces [`Value::String`] and [`Value::Section`]; the rest come
/// from the binary form, and are written to text as their usual string form.
#[derive(Debug, Clone, PartialEq)]
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
}

impl Document {
    /// Parses KV1 text with default [`Options`].
    pub fn parse(text: &str) -> Result<Self> {
        Self::parse_with(text, &Options::default())
    }

    /// Parses KV1 text.
    pub fn parse_with(text: &str, options: &Options) -> Result<Self> {
        text_read::parse(text, options)
    }

    /// Writes Valve-layout KV1 text with default [`Options`].
    pub fn to_text(&self) -> Result<String> {
        self.to_text_with(&Options::default())
    }

    /// Writes Valve-layout KV1 text: tab indent, `"key"\t\t"value"`.
    pub fn to_text_with(&self, options: &Options) -> Result<String> {
        text_write::write(self, options)
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

    /// First root whose key matches, ASCII case-insensitively.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Entry> {
        find(&self.roots, key)
    }
}

fn find<'a>(entries: &'a [Entry], key: &str) -> Option<&'a Entry> {
    entries.iter().find(|e| e.key.eq_ignore_ascii_case(key))
}

impl Entry {
    /// An entry with no conditional tag.
    pub fn new(key: impl Into<String>, value: Value) -> Self {
        Self {
            key: key.into(),
            value,
            condition: None,
        }
    }

    /// A string entry.
    pub fn string(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(key, Value::String(value.into()))
    }

    /// A section entry.
    pub fn section(key: impl Into<String>, children: Vec<Entry>) -> Self {
        Self::new(key, Value::Section(children))
    }

    /// Sets the conditional tag, without brackets.
    #[must_use]
    pub fn with_condition(mut self, condition: impl Into<String>) -> Self {
        self.condition = Some(condition.into());
        self
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
