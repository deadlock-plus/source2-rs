//! Reader and writer for Valve's `KeyValues2` / DMX (Datamodel eXchange).
//!
//! A document is a header line, optional prefix elements and a graph of elements. Each
//! element has an id, a name, a class and ordered, typed attributes; attributes can point
//! at other elements by id. Both the text (`keyvalues2`, `keyvalues2_noids`) and the
//! `binary` encodings are handled, and a parsed document writes back as it was read.
//!
//! Parse and write back; the bytes come out the same:
//!
//! ```no_run
//! use source2_kv2::Document;
//!
//! let doc = Document::read_file("input.dmx")?;
//! if let Some(root) = doc.root() {
//!     println!("{} has {} attributes", root.class, root.attributes.len());
//! }
//! doc.write_file("copy.dmx")?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Build one from scratch:
//!
//! ```
//! use source2_kv2::{Document, Element, Encoding};
//!
//! let mut doc = Document::with_encoding(Encoding::Binary, 5, "dmx", 1);
//! let root = doc.add_element(Element::new("Scene").name("demo").attr("version", 3));
//! doc.add_child(root, "camera", Element::new("Camera").attr("fov", 90.0f32));
//! doc.push_child(root, "items", Element::new("Item").attr("label", "a"));
//! let bytes = doc.to_bytes()?;
//! assert_eq!(Document::parse(&bytes)?.elements.len(), 3);
//! # Ok::<(), source2_kv2::Error>(())
//! ```

#![forbid(unsafe_code)]

#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

mod binary_read;
mod binary_write;
pub mod error;
mod header;
mod model;
mod text_read;
mod text_style;
mod text_write;
mod uuid;
mod validate;
mod value_text;

#[cfg(test)]
mod api_tests;
#[cfg(test)]
mod binary_tests;
#[cfg(test)]
mod real_tests;
#[cfg(test)]
mod text_read_tests;
#[cfg(test)]
mod text_write_tests;

pub use error::{Error, Result};
pub use model::{
    Attribute, Color, Document, Element, ElementId, ElementRef, Encoding, FloatFormat, Newline,
    Prefix, TextLayout, TextStyle, Time, Value, ValueType,
};
pub use uuid::Uuid;

use std::path::Path;

impl Document {
    /// Parses a document, picking the text or binary reader from the header.
    ///
    /// Everything the file stores is kept, so [`Document::to_bytes`] gives the input
    /// back. References to ids no element has stay [`ElementRef::External`]; call
    /// [`Document::check_references`] to reject them.
    pub fn parse(bytes: &[u8]) -> Result<Document> {
        let h = header::parse(bytes)?;
        let mut doc = Document::with_encoding(h.encoding, h.version, h.format, h.format_version);
        let body = &bytes[h.body..];
        match h.encoding {
            Encoding::Binary => binary_read::parse(doc, body),
            Encoding::KeyValues2 | Encoding::KeyValues2NoIds => {
                doc.text_style = text_style::detect(body, h.newline);
                text_read::parse(doc, body)
            }
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
