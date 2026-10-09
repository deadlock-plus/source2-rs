#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod bc;
pub mod error;
pub mod format;
pub mod texture;

pub use error::{Error, Result};
pub use format::Format;
pub use texture::{Extra, Header, Image, Texture};

#[cfg(test)]
mod tests;
