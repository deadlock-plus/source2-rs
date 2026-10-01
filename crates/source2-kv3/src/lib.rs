#![forbid(unsafe_code)]

//! Read and write Valve's KeyValues 3 (KV3), in its binary and text forms.
//!
//! KV3 is the typed, nestable key-value format Source 2 uses for configuration, data tables and
//! the structured blocks inside compiled resources. A [`Document`] is a root [`Value`] plus the
//! few settings that decide how it is encoded, and it goes from bytes or text to a tree and
//! back.
//!
//! ```
//! use source2_kv3::{Document, Object, Value};
//!
//! let mut config = Object::new();
//! config.insert("name", "example");
//! config.insert("count", 3);
//! config.insert("scale", 0.5);
//! config.insert("tags", vec![Value::from("a"), Value::from("b")]);
//!
//! let doc = Document::new(config);
//! let bytes = doc.to_bytes()?;
//! let again = Document::from_bytes(&bytes)?;
//! assert_eq!(again.root, doc.root);
//!
//! let text = doc.to_text()?;
//! assert_eq!(Document::from_text(&text)?.root, doc.root);
//! # Ok::<(), source2_kv3::Error>(())
//! ```
//!
//! # Binary revisions
//!
//! | Revision | Magic | Read | Write | Checked against |
//! |---|---|---|---|---|
//! | [`Version::Legacy`] | `VKV\x03` | yes | yes | sample files (stored by block flag, LZ4, block-compressed) |
//! | [`Version::V1`] | `KV3\x01` | yes | yes | sample files (LZ4) |
//! | [`Version::V2`] | `KV3\x02` | yes | yes | **nothing**: no sample exists, layout inferred |
//! | [`Version::V3`] | `KV3\x03` | yes | yes | **nothing**: no sample exists, laid out as v4 |
//! | [`Version::V4`] | `KV3\x04` | yes | yes | files from a shipped Source 2 title (LZ4, zstd) |
//! | [`Version::V5`] | `KV3\x05` | yes | yes | files from a shipped Source 2 title (LZ4, zstd, blobs) |
//!
//! Payloads may be stored, LZ4-compressed, zstd-compressed, or (`VKV\x03` only) packed with
//! Valve's own block scheme. The `VKV\x03` encoding that stores its stream with no compression
//! at all names itself with a GUID recalled from memory rather than measured, since no sample
//! uses it.
//!
//! # Round trips
//!
//! [`parse`] followed by [`Document::to_bytes`] gives back the same *decompressed payload*, byte
//! for byte, and the same value tree, on every sample the real-data tests were run against.
//! Values remember how each number, array and multi-line string was stored (see [`Value`]),
//! documents remember their revision, compression and format GUID, and objects keep their
//! member order and repeated keys.
//!
//! What is not reproduced is the compressed bytes and the header fields that describe them
//! (sizes, counts): the writer derives those. They match Valve's file only when the compressor
//! makes the same choices; the real-data tests report how often that holds for LZ4. Valve's
//! block scheme is written by a different match finder, so those files decompress to identical
//! bytes but are not byte-identical.
//!
//! Text KV3 drops comments and keeps everything else a value can hold except the binary
//! storage hints; see [`parse_text`] and [`write_text`].
//!
//! # Features and dependencies
//!
//! | Feature | Default | Adds |
//! |---|---|---|
//! | `lz4` | yes | LZ4 reading through `lz4_flex`. LZ4 *writing* is a port of the reference high-compression encoder (level 12) built into this crate, with no dependency. |
//! | `zstd` | yes | zstd reading through `ruzstd` and writing through `zstd-rs` |
//!
//! Without a feature, a block that needs its codec fails with [`Error::Compression`].

mod compression;
mod decode;
mod document;
mod error;
mod guid;
mod header;
mod legacy;
#[cfg(feature = "lz4")]
mod lz4_hc;
mod node;
mod object;
mod read;
mod text_read;
mod text_write;
mod value;
mod write_legacy;
mod writer;

pub use decode::{Decoded, decode};
pub use document::{Document, Tag, TextHeader, parse};
pub use error::{Error, Result};
pub use guid::{GENERIC_FORMAT, TEXT_ENCODING};
pub use header::{
    Compression, Header, MAGIC_LEGACY, MAGIC_V1, MAGIC_V2, MAGIC_V3, MAGIC_V4, MAGIC_V5, Version,
};
pub use object::Object;
pub use text_read::parse_text;
pub use text_write::write_text;
pub use value::{Kind, Value, flag};
pub use writer::{WriteOptions, write};

#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

#[cfg(all(test, feature = "lz4", feature = "zstd"))]
mod blob_tests;
#[cfg(test)]
mod edit_tests;
#[cfg(test)]
mod legacy_tests;
#[cfg(all(test, feature = "lz4"))]
mod lz4_hc_tests;
#[cfg(test)]
mod object_tests;
#[cfg(all(test, feature = "lz4", feature = "zstd"))]
mod real_tests;
#[cfg(test)]
mod roundtrip_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod text_contract_tests;
#[cfg(test)]
mod text_read_tests;
#[cfg(test)]
mod text_write_tests;
#[cfg(test)]
mod value_tests;
#[cfg(test)]
mod versions_tests;
#[cfg(test)]
mod write_tests;
