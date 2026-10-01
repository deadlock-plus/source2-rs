//! Errors from reading or writing a compiled resource.

use crate::kind::BlockKind;

/// What went wrong reading or writing a resource.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The input ends before a structure the header promises.
    Truncated {
        /// The structure that did not fit, e.g. `"header"` or `"block table"`.
        what: &'static str,
        /// Total input length that structure needs, in bytes.
        needed: u64,
        /// Input length actually available, in bytes.
        available: u64,
    },
    /// A block points outside the input.
    BadOffset {
        /// Position of the block in the table.
        index: usize,
        /// The block's tag.
        kind: BlockKind,
        /// Absolute offset of the block's bytes.
        offset: u64,
        /// Length the table gives the block.
        length: u64,
        /// Input length, in bytes.
        available: u64,
    },
    /// The table describes a layout this crate cannot represent: a block that overlaps
    /// the table or an earlier block, blocks out of offset order, or a table that
    /// overlaps the header.
    BadLayout {
        /// Position of the offending block, or `None` for the table itself.
        index: Option<usize>,
        /// What is wrong.
        reason: &'static str,
    },
    /// The header version is not one this crate reads or writes.
    UnsupportedVersion {
        /// The header version found (or set on the resource being written).
        found: u16,
    },
    /// A value does not fit the 32-bit field the format stores it in.
    TooLarge {
        /// The value that overflowed, e.g. `"block length"`.
        what: &'static str,
    },
    /// The underlying reader or writer failed.
    Io(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Truncated {
                what,
                needed,
                available,
            } => write!(f, "truncated {what}: need {needed} bytes, have {available}"),
            Error::BadOffset {
                index,
                kind,
                offset,
                length,
                available,
            } => write!(
                f,
                "block {index} ({kind}) spans {offset}..{} of {available} bytes",
                offset.saturating_add(*length)
            ),
            Error::BadLayout {
                index: Some(i),
                reason,
            } => write!(f, "unsupported layout at block {i}: {reason}"),
            Error::BadLayout {
                index: None,
                reason,
            } => write!(f, "unsupported layout: {reason}"),
            Error::UnsupportedVersion { found } => {
                write!(f, "unsupported resource header version {found}")
            }
            Error::TooLarge { what } => write!(f, "{what} does not fit in 32 bits"),
            Error::Io(e) => write!(f, "i/o error: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
