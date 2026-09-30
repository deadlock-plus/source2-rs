//! Errors from reading binary KV3.

/// What went wrong reading a KV3 block.
#[derive(Debug)]
pub enum Error {
    /// The bytes are not a well-formed KV3 block, or use something this reader does not
    /// implement.
    Malformed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Malformed(m) => write!(f, "malformed KV3: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
