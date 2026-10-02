#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(not(doctest), doc = include_str!("../README.md"))]

#[cfg(all(
    doctest,
    feature = "vpk",
    feature = "resource",
    feature = "kv1",
    feature = "kv2",
    feature = "kv3"
))]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

mod error;
pub mod ext;
mod format;

pub use error::{Error, Result};
pub use format::{Format, detect};

#[cfg(feature = "vpk")]
#[cfg_attr(docsrs, doc(cfg(feature = "vpk")))]
pub use source2_vpk as vpk;

#[cfg(feature = "resource")]
#[cfg_attr(docsrs, doc(cfg(feature = "resource")))]
pub use source2_resource as resource;

#[cfg(feature = "kv1")]
#[cfg_attr(docsrs, doc(cfg(feature = "kv1")))]
pub use source2_kv1 as kv1;

#[cfg(feature = "kv2")]
#[cfg_attr(docsrs, doc(cfg(feature = "kv2")))]
pub use source2_kv2 as kv2;

#[cfg(feature = "kv3")]
#[cfg_attr(docsrs, doc(cfg(feature = "kv3")))]
pub use source2_kv3 as kv3;
