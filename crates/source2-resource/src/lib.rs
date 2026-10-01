#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

pub mod error;
pub mod kind;
pub mod resource;

pub use error::{Error, Result};
pub use kind::BlockKind;
pub use resource::{Block, HEADER_VERSION, Padding, Resource, Versions};

#[cfg(test)]
mod real_tests;
#[cfg(test)]
mod tests;
