//! Errors from reading and writing KV3, binary and text.

/// What went wrong reading or writing a KV3 document.
///
/// The variants separate who is at fault: [`Malformed`](Error::Malformed) and
/// [`Syntax`](Error::Syntax) are bad input to a reader, [`Invalid`](Error::Invalid) is a value
/// a writer cannot store, [`Unsupported`](Error::Unsupported) is something the format has that
/// this crate does not handle, and [`Compression`](Error::Compression) is a codec that failed
/// or is not built in.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The bytes are not a well-formed binary KV3 block: a bad magic, sizes that disagree, a
    /// truncated pool, an out-of-range string index, nesting past the depth limit.
    Malformed(String),
    /// The text is not well-formed text KV3. `line` counts from 1.
    Syntax {
        /// Line of the input the problem was found on.
        line: usize,
        /// What was wrong.
        message: String,
    },
    /// A value the chosen output cannot store: a string with a NUL in binary, a non-finite
    /// double or an unspellable flag bit in text, a blob in a revision with no blob area,
    /// nesting deeper than a reader would accept, or more data than a 32-bit size can hold.
    Invalid(String),
    /// The format allows it, this crate does not handle it: a revision or compression method
    /// that is unknown, or a combination of the two that no file is known to use.
    Unsupported(String),
    /// A compressed stream would not decode, did not produce the length the header promised,
    /// or the codec needed is not enabled in this build.
    Compression(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Malformed(m) => write!(f, "malformed KV3: {m}"),
            Error::Syntax { line, message } => write!(f, "text KV3 line {line}: {message}"),
            Error::Invalid(m) => write!(f, "cannot write KV3: {m}"),
            Error::Unsupported(m) => write!(f, "unsupported KV3: {m}"),
            Error::Compression(m) => write!(f, "KV3 compression: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
