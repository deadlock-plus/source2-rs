//! Source 2 compiled-resource containers.
//!
//! Everything the engine compiles - `.vdata_c`, `.vtex_c`, `.vmdl_c` - shares one envelope:
//! a short header, then a table of four-character blocks. See [`resource`] for the layout.

#![forbid(unsafe_code)]

pub mod error;
pub mod resource;

pub use error::{Error, Result};
pub use resource::{Block, Resource};

#[cfg(test)]
mod tests;
