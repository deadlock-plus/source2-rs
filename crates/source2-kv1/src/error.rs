//! Errors from reading or writing KeyValues 1.

/// What went wrong reading or writing KV1.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// A quoted string was never closed.
    UnterminatedString {
        /// Line of the opening quote, from 1.
        line: usize,
        /// Column of the opening quote, from 1.
        column: usize,
    },
    /// A `[...]` conditional tag was never closed on its line.
    UnterminatedCondition {
        /// Line of the opening bracket, from 1.
        line: usize,
        /// Column of the opening bracket, from 1.
        column: usize,
    },
    /// The input ended while something was still expected.
    UnexpectedEof {
        /// Line the input ended on.
        line: usize,
        /// What the parser was waiting for.
        expected: &'static str,
    },
    /// A token appeared where it is not allowed.
    UnexpectedToken {
        /// Line of the token, from 1.
        line: usize,
        /// Column of the token, from 1.
        column: usize,
        /// The offending token.
        found: String,
        /// What the parser wanted instead.
        expected: &'static str,
    },
    /// Sections are nested deeper than [`Options::max_depth`](crate::Options::max_depth).
    TooDeep {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// Binary KV1 data is truncated or otherwise invalid.
    MalformedBinary(String),
    /// A binary type byte this crate does not read. See the crate docs for the supported set.
    UnsupportedType {
        /// The type byte.
        type_byte: u8,
        /// Offset of the byte in the input.
        offset: usize,
    },
    /// The text is in an encoding this crate does not read, such as UTF-16.
    UnsupportedEncoding(&'static str),
    /// A typed value cannot be written as text without losing its type. Call
    /// [`Document::stringified`](crate::Document::stringified) to convert it on purpose.
    TypedValueInText {
        /// Key of the entry.
        key: String,
        /// Name of the value type, e.g. `int`.
        kind: &'static str,
    },
    /// The tree cannot be written in the requested format.
    InvalidInput(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::UnterminatedString { line, column } => {
                write!(f, "{line}:{column}: unterminated quoted string")
            }
            Error::UnterminatedCondition { line, column } => {
                write!(f, "{line}:{column}: unterminated conditional tag")
            }
            Error::UnexpectedEof { line, expected } => {
                write!(f, "line {line}: input ended, expected {expected}")
            }
            Error::UnexpectedToken {
                line,
                column,
                found,
                expected,
            } => write!(
                f,
                "{line}:{column}: unexpected {found:?}, expected {expected}"
            ),
            Error::TooDeep { limit } => write!(f, "sections nested deeper than {limit}"),
            Error::MalformedBinary(m) => write!(f, "malformed binary KV1: {m}"),
            Error::UnsupportedType { type_byte, offset } => {
                write!(
                    f,
                    "unsupported binary KV1 type byte {type_byte} at {offset}"
                )
            }
            Error::UnsupportedEncoding(e) => write!(f, "unsupported text encoding: {e}"),
            Error::TypedValueInText { key, kind } => write!(
                f,
                "{key:?} is a typed {kind} value; text KV1 has no types (see Document::stringified)"
            ),
            Error::InvalidInput(m) => write!(f, "cannot write KV1: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
