//! Reader and writer for Valve's `KeyValues2` / DMX (Datamodel eXchange).
//!
//! A document is a header line, optional prefix elements and a graph of elements. Each
//! element has an id, a name, a class and ordered, typed attributes; attributes can point
//! at other elements by id. Both the text (`keyvalues2`, `keyvalues2_noids`) and the
//! `binary` encodings are handled.
//!
//! ```no_run
//! use source2_kv2::{Document, Value};
//!
//! let doc = Document::read_file("map.vmap")?;
//! let root = doc.root().expect("a document has a root");
//! if let Some(Value::Int(build)) = root.attribute("editorbuild") {
//!     println!("{} built by editor {build}", root.class);
//! }
//! std::fs::write("copy.vmap", doc.to_bytes()?)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]

mod binary_read;
mod binary_write;
pub mod error;
mod header;
mod model;
mod text_read;
mod text_write;
mod uuid;
mod validate;
mod value_text;

#[cfg(test)]
mod binary_tests;
#[cfg(test)]
mod text_read_tests;
#[cfg(test)]
mod text_write_tests;

pub use error::{Error, Result};
pub use model::{
    Attribute, Color, Document, Element, ElementId, ElementRef, Encoding, Time, Value, ValueType,
};
pub use uuid::Uuid;

use std::path::Path;

/// Options for reading a document.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReadOptions {
    /// Keep a text reference to an unknown id as [`ElementRef::External`] instead of
    /// failing with [`Error::UnresolvedReference`].
    pub allow_unresolved: bool,
}

impl Document {
    /// Parses a document, picking the text or binary reader from the header.
    pub fn parse(bytes: &[u8]) -> Result<Document> {
        Document::parse_with(bytes, &ReadOptions::default())
    }

    /// [`Document::parse`] with explicit options.
    pub fn parse_with(bytes: &[u8], opts: &ReadOptions) -> Result<Document> {
        let h = header::parse(bytes)?;
        let doc = Document::new(h.encoding, h.version, h.format, h.format_version);
        let body = &bytes[h.body..];
        match h.encoding {
            Encoding::Binary => binary_read::parse(doc, body),
            Encoding::KeyValues2 | Encoding::KeyValues2NoIds => text_read::parse(doc, body, opts),
        }
    }

    /// Reads and parses a file.
    pub fn read_file(path: impl AsRef<Path>) -> Result<Document> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|e| Error::Io {
            path: path.to_path_buf(),
            source: e.to_string(),
        })?;
        Document::parse(&bytes)
    }

    /// Serializes the document in its own encoding and version.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        validate::validate(self)?;
        header::write(self, &mut out)?;
        match self.encoding {
            Encoding::Binary => binary_write::write(self, &mut out)?,
            Encoding::KeyValues2 | Encoding::KeyValues2NoIds => text_write::write(self, &mut out)?,
        }
        Ok(out)
    }

    /// Serializes the document and writes it to a file.
    pub fn write_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        std::fs::write(path, self.to_bytes()?).map_err(|e| Error::Io {
            path: path.to_path_buf(),
            source: e.to_string(),
        })
    }
}
