//! Errors from reading or decoding a texture.

use crate::format::Format;

/// What went wrong reading or decoding a texture.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The input ends before a structure the header promises.
    Truncated {
        /// The structure that did not fit, e.g. `"header"` or `"extra data table"`.
        what: &'static str,
        /// Total input length that structure needs, in bytes.
        needed: u64,
        /// Input length actually available, in bytes.
        available: u64,
    },
    /// Width, height, depth or the mip count is zero.
    ZeroDimension {
        /// The field that is zero: `"width"`, `"height"`, `"depth"` or `"mip count"`.
        what: &'static str,
    },
    /// The pixel format is one this crate does not decode.
    UnsupportedFormat {
        /// The format found.
        format: Format,
    },
    /// The texture is a cube map, volume or array. Only plain 2D textures are read.
    UnsupportedLayout {
        /// What the header says it is.
        kind: &'static str,
    },
    /// The mips are stored compressed (extra data type 3), which this crate does not read.
    UnsupportedCompressedMips,
    /// The pixel bytes are not the length the header's mips add up to.
    BadPixelLength {
        /// Length the mip chain needs, in bytes.
        expected: u64,
        /// Length given, in bytes.
        actual: u64,
    },
    /// The requested mip does not exist.
    MipOutOfRange {
        /// The index asked for. Index 0 is the largest mip.
        index: u32,
        /// How many mips the texture has.
        count: u32,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Truncated {
                what,
                needed,
                available,
            } => write!(f, "truncated {what}: need {needed} bytes, have {available}"),
            Error::ZeroDimension { what } => write!(f, "{what} is zero"),
            Error::UnsupportedFormat { format } => write!(f, "unsupported pixel format {format}"),
            Error::UnsupportedLayout { kind } => write!(f, "unsupported texture layout: {kind}"),
            Error::UnsupportedCompressedMips => f.write_str("compressed mips are not supported"),
            Error::BadPixelLength { expected, actual } => {
                write!(f, "mips need {expected} pixel bytes, got {actual}")
            }
            Error::MipOutOfRange { index, count } => {
                write!(f, "mip {index} out of range for {count} mips")
            }
        }
    }
}

impl std::error::Error for Error {}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
