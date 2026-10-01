//! Errors from reading or writing KeyValues 1.

/// What went wrong reading or writing KV1.
#[derive(Debug, Clone, PartialEq, Eq)]
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
            Error::InvalidInput(m) => write!(f, "cannot write KV1: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
