//! Errors from reading or writing a Datamodel document.

use crate::Uuid;
use std::path::PathBuf;

/// What went wrong reading or writing a document.
#[derive(Debug)]
pub enum Error {
    /// The first line is not a `<!-- dmx encoding ... -->` header.
    BadHeader(String),
    /// The header names an encoding this crate does not handle.
    UnsupportedEncoding(String),
    /// The encoding is known but this version of it is not.
    UnsupportedVersion {
        /// Encoding name from the header.
        encoding: String,
        /// Version number from the header.
        version: u32,
    },
    /// The text body does not parse.
    Syntax {
        /// One-based line of the offending token.
        line: usize,
        /// What was expected or found.
        message: String,
    },
    /// The binary body ended early, or a count, index or type was out of range.
    Malformed(String),
    /// An element reference names an id that no element in the document has.
    UnresolvedReference(Uuid),
    /// Two elements share one id.
    DuplicateId(Uuid),
    /// Elements are nested deeper than the reader or writer allows.
    TooDeep,
    /// The document cannot be written in the requested encoding.
    InvalidModel(String),
    /// A file could not be read or written.
    Io {
        /// The path that failed.
        path: PathBuf,
        /// The underlying message.
        source: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::BadHeader(m) => write!(f, "bad DMX header: {m}"),
            Error::UnsupportedEncoding(e) => write!(f, "unsupported DMX encoding `{e}`"),
            Error::UnsupportedVersion { encoding, version } => {
                write!(f, "unsupported {encoding} version {version}")
            }
            Error::Syntax { line, message } => write!(f, "line {line}: {message}"),
            Error::Malformed(m) => write!(f, "malformed DMX binary: {m}"),
            Error::UnresolvedReference(id) => write!(f, "no element has id {id}"),
            Error::DuplicateId(id) => write!(f, "more than one element has id {id}"),
            Error::TooDeep => write!(f, "elements are nested too deeply"),
            Error::InvalidModel(m) => write!(f, "cannot write document: {m}"),
            Error::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for Error {}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
