//! Errors from reading or writing a VPK.

use std::path::PathBuf;

/// What went wrong reading or writing an archive.
#[derive(Debug)]
pub enum Error {
    /// The file does not start with the VPK signature.
    NotAVpk {
        /// What the first four bytes actually were.
        found: u32,
    },
    /// The directory file is a VPK version this reader does not handle.
    UnsupportedVersion(u32),
    /// The directory tree ended early, or an offset pointed outside the file.
    Malformed(String),
    /// An entry's stored CRC did not match the bytes read for it.
    ChecksumMismatch {
        /// Path of the entry inside the archive.
        path: String,
        /// CRC recorded in the directory.
        expected: u32,
        /// CRC of the bytes actually read.
        found: u32,
    },
    /// The files handed to the writer cannot be laid out as a VPK.
    InvalidInput(String),
    /// A numbered archive the directory refers to could not be read.
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
            Error::NotAVpk { found } => {
                write!(f, "not a VPK: signature {found:#010x}")
            }
            Error::UnsupportedVersion(v) => write!(f, "unsupported VPK version {v}"),
            Error::InvalidInput(m) => write!(f, "cannot write VPK: {m}"),
            Error::Malformed(m) => write!(f, "malformed VPK directory: {m}"),
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

impl std::error::Error for Error {}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
