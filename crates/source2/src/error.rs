use std::fmt;

use crate::Format;

/// An error from a composition helper in [`ext`](crate::ext), wrapping the error of
/// whichever format crate failed.
///
/// `non_exhaustive`: match with a wildcard arm. A variant that wraps a format crate's error
/// exists only when that crate's feature is enabled.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// A VPK failure.
    #[cfg(feature = "vpk")]
    #[cfg_attr(docsrs, doc(cfg(feature = "vpk")))]
    Vpk(source2_vpk::Error),
    /// A compiled-resource failure.
    #[cfg(feature = "resource")]
    #[cfg_attr(docsrs, doc(cfg(feature = "resource")))]
    Resource(source2_resource::Error),
    /// A KeyValues 1 failure.
    #[cfg(feature = "kv1")]
    #[cfg_attr(docsrs, doc(cfg(feature = "kv1")))]
    Kv1(source2_kv1::Error),
    /// A KeyValues 2 failure.
    #[cfg(feature = "kv2")]
    #[cfg_attr(docsrs, doc(cfg(feature = "kv2")))]
    Kv2(source2_kv2::Error),
    /// A KeyValues 3 failure.
    #[cfg(feature = "kv3")]
    #[cfg_attr(docsrs, doc(cfg(feature = "kv3")))]
    Kv3(source2_kv3::Error),
    /// The pack has no entry with this path.
    EntryNotFound(String),
    /// The resource has no block with this tag.
    BlockNotFound(String),
    /// The bytes do not start like the format the call needs.
    UnrecognisedFormat {
        /// What the call needed.
        expected: Format,
        /// What [`detect`](crate::detect) made of the bytes.
        found: Format,
    },
    /// A text format was asked for, but the entry is not UTF-8.
    NotUtf8(String),
}

/// Result alias for this crate's composition helpers.
pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "vpk")]
            Error::Vpk(e) => write!(f, "vpk: {e}"),
            #[cfg(feature = "resource")]
            Error::Resource(e) => write!(f, "resource: {e}"),
            #[cfg(feature = "kv1")]
            Error::Kv1(e) => write!(f, "kv1: {e}"),
            #[cfg(feature = "kv2")]
            Error::Kv2(e) => write!(f, "kv2: {e}"),
            #[cfg(feature = "kv3")]
            Error::Kv3(e) => write!(f, "kv3: {e}"),
            Error::EntryNotFound(path) => write!(f, "no entry named `{path}`"),
            Error::BlockNotFound(kind) => write!(f, "no block tagged `{kind}`"),
            Error::UnrecognisedFormat { expected, found } => {
                write!(f, "expected {expected} data, found {found}")
            }
            Error::NotUtf8(path) => write!(f, "entry `{path}` is not UTF-8 text"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            #[cfg(feature = "vpk")]
            Error::Vpk(e) => Some(e),
            #[cfg(feature = "resource")]
            Error::Resource(e) => Some(e),
            #[cfg(feature = "kv1")]
            Error::Kv1(e) => Some(e),
            #[cfg(feature = "kv2")]
            Error::Kv2(e) => Some(e),
            #[cfg(feature = "kv3")]
            Error::Kv3(e) => Some(e),
            _ => None,
        }
    }
}

#[cfg(feature = "vpk")]
impl From<source2_vpk::Error> for Error {
    fn from(e: source2_vpk::Error) -> Self {
        Error::Vpk(e)
    }
}

#[cfg(feature = "resource")]
impl From<source2_resource::Error> for Error {
    fn from(e: source2_resource::Error) -> Self {
        Error::Resource(e)
    }
}

#[cfg(feature = "kv1")]
impl From<source2_kv1::Error> for Error {
    fn from(e: source2_kv1::Error) -> Self {
        Error::Kv1(e)
    }
}

#[cfg(feature = "kv2")]
impl From<source2_kv2::Error> for Error {
    fn from(e: source2_kv2::Error) -> Self {
        Error::Kv2(e)
    }
}

#[cfg(feature = "kv3")]
impl From<source2_kv3::Error> for Error {
    fn from(e: source2_kv3::Error) -> Self {
        Error::Kv3(e)
    }
}
