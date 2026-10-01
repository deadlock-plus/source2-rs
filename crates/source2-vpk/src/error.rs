//! Errors from reading or writing a VPK.

use std::path::PathBuf;

/// What went wrong reading or writing an archive.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The file does not start with the VPK signature.
    NotAVpk {
        /// What the first four bytes actually were.
        found: u32,
    },
    /// The directory file is a VPK version this crate does not handle.
    UnsupportedVersion(u32),
    /// The directory tree or a section ended early, or its sizes disagree with the file.
    Malformed(String),
    /// A name in the directory tree is not valid UTF-8.
    ///
    /// [`Vpk::parse_lossy`](crate::Vpk::parse_lossy) reads such files with the bad bytes
    /// replaced, at the cost of not writing back to the same bytes.
    InvalidName {
        /// The name, with invalid bytes replaced, for display.
        lossy: String,
    },
    /// An entry's stored CRC did not match the bytes read for it.
    ChecksumMismatch {
        /// Path of the entry inside the archive.
        path: String,
        /// CRC recorded in the directory.
        expected: u32,
        /// CRC of the bytes actually read.
        found: u32,
    },
    /// No entry has the requested path.
    NotFound(String),
    /// The document cannot be laid out as a VPK, or a request cannot be answered.
    InvalidInput(String),
    /// A file could not be read or written.
    Io {
        /// The path that failed.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotAVpk { found } => {
                write!(f, "not a VPK: signature {found:#010x}")
            }
            Error::UnsupportedVersion(v) => write!(f, "unsupported VPK version {v}"),
            Error::NotFound(p) => write!(f, "no entry {p}"),
            Error::InvalidInput(m) => write!(f, "invalid VPK input: {m}"),
            Error::Malformed(m) => write!(f, "malformed VPK: {m}"),
            Error::InvalidName { lossy } => write!(f, "name is not valid UTF-8: {lossy:?}"),
            Error::ChecksumMismatch {
                path,
                expected,
                found,
            } => write!(
                f,
                "{path}: CRC {found:#010x}, directory says {expected:#010x}"
            ),
            Error::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
