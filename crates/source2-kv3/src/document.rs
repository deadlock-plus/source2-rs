//! [`Document`]: a root value with everything needed to write it back.

use crate::error::Result;
use crate::guid::{GENERIC_FORMAT, TEXT_ENCODING};
use crate::read::parse_root;
use crate::{Value, WriteOptions, decode};

/// One entry of a text header, `name:version{guid}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    /// The entry's name, such as `text` or `generic`.
    pub name: String,
    /// The GUID in binary header layout; see [`Tag::guid_string`].
    pub guid: [u8; 16],
}

impl Tag {
    /// The text encoding Valve's text KV3 uses.
    #[must_use]
    pub fn text_encoding() -> Self {
        Tag {
            name: "text".into(),
            guid: TEXT_ENCODING,
        }
    }

    /// The GUID as a text header spells it, `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`.
    #[must_use]
    pub fn guid_string(&self) -> String {
        crate::guid::format_guid(&self.guid)
    }
}

/// What a text KV3 header says: `<!-- kv3 encoding:... format:... -->`.
///
/// The format's GUID is not here. It is [`WriteOptions::format`] on the document, because binary
/// headers carry it too, and one document has one format. Only the format's *name* is text-only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextHeader {
    /// The `encoding:` entry.
    pub encoding: Tag,
    /// The name in the `format:` entry. Binary headers carry no names, so a document read from
    /// binary has `generic` for the generic format's GUID and an empty name for any other, which
    /// the text writer spells `unnamed`.
    pub format_name: String,
}

impl Default for TextHeader {
    fn default() -> Self {
        TextHeader {
            encoding: Tag::text_encoding(),
            format_name: "generic".into(),
        }
    }
}

/// A KV3 document: the root value, and what it takes to write it back as it was read.
///
/// One type for both encodings. [`parse`](crate::parse) and [`parse_text`](crate::parse_text)
/// fill in the whole document; [`Document::new`] makes one for a hand-built value with Valve's
/// defaults; [`to_bytes`](Document::to_bytes) and [`to_text`](Document::to_text) write it.
///
/// The settings sit in public fields to be changed after a read or a [`new`](Document::new):
///
/// ```
/// use source2_kv3::{Compression, Document, Object, Version};
///
/// let mut doc = Document::new(Object::from_iter([("answer", 42)]));
/// doc.options.version = Version::V4;
/// doc.options.compression = Compression::None;
/// let bytes = doc.to_bytes()?;
/// assert_eq!(Document::from_bytes(&bytes)?.root, doc.root);
/// # Ok::<(), source2_kv3::Error>(())
/// ```
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Document {
    /// The root value. Always an object or an array in practice.
    pub root: Value,
    /// How the binary form is encoded: revision, compression, format GUID. Read from the block
    /// header by [`parse`](crate::parse); text carries only the GUID.
    pub options: WriteOptions,
    /// What the text header says.
    pub text: TextHeader,
}

impl Document {
    /// A document for a hand-built value, in Valve's usual encoding: v5, the generic format,
    /// compressed with LZ4 (or the first codec this build has).
    #[must_use]
    pub fn new(root: impl Into<Value>) -> Self {
        Document {
            root: root.into(),
            options: WriteOptions::default(),
            text: TextHeader::default(),
        }
    }

    /// Read a binary KV3 block; see [`parse`](crate::parse).
    ///
    /// # Errors
    ///
    /// As for [`parse`](crate::parse).
    pub fn from_bytes(block: &[u8]) -> Result<Self> {
        parse(block)
    }

    /// Read a text KV3 document; see [`parse_text`](crate::parse_text).
    ///
    /// # Errors
    ///
    /// As for [`parse_text`](crate::parse_text).
    pub fn from_text(input: &str) -> Result<Self> {
        crate::parse_text(input)
    }

    /// Write the binary form with the document's own options.
    ///
    /// A document read with [`parse`](crate::parse) and written back has the payload it came
    /// from, byte for byte; the compressed bytes match when the compressor makes the same
    /// choices Valve's did (see the crate docs for which inputs that has been checked for).
    ///
    /// # Errors
    ///
    /// As for [`write`](crate::write).
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        crate::write(&self.root, &self.options)
    }

    /// Write the binary form with other options than the document's own.
    ///
    /// # Errors
    ///
    /// As for [`write`](crate::write).
    pub fn to_bytes_with(&self, options: &WriteOptions) -> Result<Vec<u8>> {
        crate::write(&self.root, options)
    }

    /// Write the text form; see [`write_text`](crate::write_text).
    ///
    /// # Errors
    ///
    /// As for [`write_text`](crate::write_text).
    pub fn to_text(&self) -> Result<String> {
        crate::write_text(self)
    }
}

/// Decompress and parse a binary KV3 block into a document.
///
/// The input is the whole block, header included, such as the `DATA` block of a compiled
/// resource. Every revision from the original `VKV\x03` encoding to `KV3\x05` is read. The tree
/// remembers how each number and array was stored, so [`Document::to_bytes`] reproduces the
/// payload; see [`Value`].
///
/// # Errors
///
/// [`Error::Malformed`](crate::Error::Malformed) if the block will not decode ([`decode`]) or its
/// pools and type stream disagree - a truncated pool, an out-of-range string index, or nesting
/// deeper than the reader allows; [`Error::Unsupported`](crate::Error::Unsupported) for a
/// compression method this crate does not know; [`Error::Compression`](crate::Error::Compression)
/// if a codec fails or is not enabled.
pub fn parse(block: &[u8]) -> Result<Document> {
    let decoded = decode(block)?;
    let root = parse_root(&decoded)?;
    let h = &decoded.header;
    let options = WriteOptions {
        version: h.version,
        compression: h.compression,
        format: h.format,
        dictionary_id: h.dictionary_id,
        frame_size: h.frame_size,
    };
    let format_name = if h.format == GENERIC_FORMAT {
        "generic"
    } else {
        ""
    };
    Ok(Document {
        root,
        options,
        text: TextHeader {
            encoding: Tag::text_encoding(),
            format_name: format_name.into(),
        },
    })
}
