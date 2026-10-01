//! Reader and writer for Valve's KeyValues 1 (KV1), text and binary.
//!
//! KV1 is the older Valve key/value format: `.vdf`, `.acf`, `.vcfg` and similar files. It is
//! an ordered tree. Every [`Entry`] has a key and either a string or a nested section, and
//! keys may repeat.
//!
//! ```no_run
//! use source2_kv1::Document;
//!
//! let doc = Document::parse_bytes(&std::fs::read("settings.vdf")?)?;
//! let name = doc.get("Settings").and_then(|e| e.get_str("name"));
//! std::fs::write("copy.vdf", doc.to_text_bytes()?)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Building a document by hand needs no layout information:
//!
//! ```
//! use source2_kv1::{Document, Entry};
//!
//! let doc = Document::new(vec![
//!     Entry::section("Settings", vec![Entry::string("name", "demo"), Entry::bool("on", true)])
//!         .with_comment(" generated"),
//! ]);
//! assert_eq!(
//!     doc.to_text()?,
//!     "// generated\n\"Settings\"\n{\n\t\"name\"\t\t\"demo\"\n\t\"on\"\t\t\"1\"\n}\n"
//! );
//! # Ok::<(), source2_kv1::Error>(())
//! ```
//!
//! # Round trip
//!
//! Parsing then writing gives back the same bytes. What the tree does not need for its
//! meaning is kept in `layout` fields on [`Document`], [`Entry`] and [`Directive`]: `//`
//! comments, blank lines and spacing, line endings, a byte order mark, whether a token was
//! quoted or bare, the exact spelling of escapes, directive keyword case, where each
//! conditional tag sat, and the text encoding. Every one of them is optional. A hand-built
//! document leaves them at their defaults and is written in Valve's conventional layout.
//! [`Document::without_layout`] drops them to compare content only.
//!
//! # Escape sequences
//!
//! Valve's parser only honours escapes when a caller opts in, and the engine default is off.
//! Tools and Steam treat backslashes as escapes, so [`Options::escape_sequences`] defaults to
//! **on**. When on, the set `\n \t \v \b \r \f \a \\ \? \' \"` is decoded and any other `\x`
//! is kept verbatim, backslash included, so Windows paths survive. When off, a backslash is
//! an ordinary character and a value cannot contain `"`. The choice is stored in
//! [`Document::escapes`] and the writer follows it, so a file read with escapes off is
//! written with escapes off. The set is from Valve's string conversion table as remembered;
//! it is not checked against Valve source or a sample that uses the rarer escapes.
//!
//! # Typed values
//!
//! Text KV1 has no types: every leaf is a string. The binary form has [`Value::Int`],
//! [`Value::Float`] and others. Writing them to text would lose the type, so
//! [`Document::to_text`] returns [`Error::TypedValueInText`] for them. To write them anyway,
//! convert on purpose with [`Document::stringified`] (`Int(5)` becomes the string `"5"`).
//!
//! # Encodings
//!
//! Text and binary strings are UTF-8 when they are valid UTF-8 and Windows-1252 otherwise,
//! recorded in [`Document::encoding`]. Each Windows-1252 byte maps to one character, so
//! the bytes come back exactly. Use [`Document::parse_bytes`] and
//! [`Document::to_text_bytes`] for files; [`Document::parse`] takes text you already hold as
//! UTF-8. UTF-16 text is an error.
//!
//! # Binary
//!
//! Type bytes: `0` section, `1` string, `2` int, `3` float, `4` ptr, `5` wide string,
//! `6` color, `7` uint64, `8` end, `10` int64. Anything else is
//! [`Error::UnsupportedType`]: `9` and `11` (compiled-int and alternate-end bytes seen in
//! other tools' notes) and the string-table variant of Steam's `appinfo.vdf`, whose key names
//! are table indices. Whether the closing end marker was present is kept in
//! [`DocumentLayout::end_marker`], so files that stop without it are rewritten without it.
//! Nothing here was checked against a real binary KV1 file; the layout is from memory.
//!
//! # Includes and conditions
//!
//! - `#include` and `#base` are recognised only at the top level of a file and become
//!   [`Directive`]s, in file order (see [`Directive::before_root`]). Nothing is opened, so
//!   nested includes are never followed or merged; that is left to the caller.
//!   Inside a section the same word is an ordinary key and is kept as an entry.
//! - A conditional tag such as `[$WIN32]` or `[!$X360]` is stored without its brackets on
//!   the entry (or directive) it follows, and is never evaluated, so an entry whose
//!   condition is false for the caller is still in the tree. It is accepted after the key,
//!   or after the value or closing brace; the position is kept in the layout. How Valve's
//!   own loader treats a tag next to a directive or inside a nested include is not verified.
//! - `/* */` comments are not part of KV1 and are not supported.
//! - Keys are matched ASCII case-insensitively by the accessors, as Valve does. The tree
//!   itself keeps keys exactly as written.

#![forbid(unsafe_code)]

mod binary;
mod encoding;
pub mod error;
mod escape;
mod layout;
mod model;
mod text_read;
mod text_write;

#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

#[cfg(test)]
mod binary_tests;
#[cfg(test)]
mod build_tests;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod real_tests;
#[cfg(test)]
mod roundtrip_tests;
#[cfg(test)]
mod text_read_tests;
#[cfg(test)]
mod text_write_tests;

pub use encoding::Encoding;
pub use error::{Error, Result};
pub use layout::{ConditionAt, DocumentLayout, Layout, LineEnding, Quote, Spelling, Trivia};
pub use model::{DEFAULT_MAX_DEPTH, Directive, DirectiveKind, Document, Entry, Options, Value};
