//! Extension traits that compose the format crates.
//!
//! The types live in other crates, so the helpers are traits: import them (or the whole
//! module with `use source2::ext::*;`) and call the methods on the foreign types. Each
//! trait exists only when every feature it needs is enabled, and none can be implemented
//! outside this crate.
//!
//! Naming: `read_*` parses, `put_*` writes. A `put_*` on a pack adds the entry or replaces
//! the one with that path (see [`VpkResource`] for exactly how). Nothing here is lossy:
//! what a document keeps, it writes back, and when a format has two encodings there are two
//! named methods rather than a guess from the file extension.
//!
//! | Trait | On | Needs | Methods |
//! |---|---|---|---|
//! | `ResourceKv3` | `Resource` | `resource`, `kv3` | `read_kv3`, `put_kv3` |
//! | `VpkResource` | `Vpk` | `vpk`, `resource` | `read_resource`, `put_resource` |
//! | `VpkKv1` | `Vpk` | `vpk`, `kv1` | `read_kv1_text`, `read_kv1_binary`, `put_kv1_text`, `put_kv1_binary` |
//! | `VpkKv2` | `Vpk` | `vpk`, `kv2` | `read_kv2`, `put_kv2` |
//! | `VpkKv3` | `Vpk` | `vpk`, `kv3` | `read_kv3`, `read_kv3_text`, `put_kv3`, `put_kv3_text` |
//! | `VpkResourceKv3` | `Vpk` | `vpk`, `resource`, `kv3` | `read_resource_kv3`, `put_resource_kv3` |

#[cfg(any(
    all(
        feature = "vpk",
        any(
            feature = "resource",
            feature = "kv1",
            feature = "kv2",
            feature = "kv3"
        )
    ),
    all(feature = "resource", feature = "kv3")
))]
mod sealed {
    pub trait Sealed {}

    #[cfg(all(
        feature = "vpk",
        any(
            feature = "resource",
            feature = "kv1",
            feature = "kv2",
            feature = "kv3"
        )
    ))]
    impl Sealed for source2_vpk::Vpk {}

    #[cfg(all(feature = "resource", feature = "kv3"))]
    impl Sealed for source2_resource::Resource {}
}

#[cfg(all(
    feature = "vpk",
    any(
        feature = "resource",
        feature = "kv1",
        feature = "kv2",
        feature = "kv3"
    )
))]
mod pack {
    use source2_vpk::{Data, Vpk};

    use crate::{Error, Result};

    pub(super) fn read_entry(pack: &Vpk, path: &str) -> Result<Vec<u8>> {
        let entry = pack
            .find(path)
            .ok_or_else(|| Error::EntryNotFound(path.to_owned()))?;
        Ok(pack.read(entry)?)
    }

    pub(super) fn put_entry(pack: &mut Vpk, path: &str, bytes: Vec<u8>) -> Result<()> {
        // Checked up front so a bad path cannot leave the replaced entries removed.
        Vpk::new(1).add(path, Vec::new())?;

        let inline = pack.find(path).is_some_and(|e| {
            matches!(
                e.data,
                Data::Stored { archive: None, .. } | Data::Memory { inline: true, .. }
            )
        });
        while pack.remove(path).is_some() {}
        let entry = pack.add(path, bytes)?;
        if let Data::Memory { inline: place, .. } = &mut entry.data {
            *place = inline;
        }
        Ok(())
    }
}

#[cfg(all(feature = "kv3", any(feature = "resource", feature = "vpk")))]
fn kv3_from_binary(bytes: &[u8]) -> crate::Result<source2_kv3::Document> {
    let found = crate::detect(bytes);
    if found != crate::Format::Kv3Binary {
        return Err(crate::Error::UnrecognisedFormat {
            expected: crate::Format::Kv3Binary,
            found,
        });
    }
    Ok(source2_kv3::Document::from_bytes(bytes)?)
}

#[cfg(all(feature = "resource", feature = "kv3"))]
pub use resource_kv3::ResourceKv3;

#[cfg(all(feature = "resource", feature = "kv3"))]
mod resource_kv3 {
    use source2_kv3::Document;
    use source2_resource::{Block, BlockKind, Resource};

    use super::sealed::Sealed;
    use crate::{Error, Result};

    /// Read and write a [`Resource`] block as KeyValues 3.
    ///
    /// ```
    /// use source2::ext::ResourceKv3;
    /// use source2::kv3::{Document, Object};
    /// use source2::resource::{Block, BlockKind, Resource};
    ///
    /// let doc = Document::new(Object::from_iter([("count", 1)]));
    /// let mut resource = Resource::new()
    ///     .with_block(Block::new(BlockKind::RERL, vec![1, 2, 3]))
    ///     .with_block(Block::new(BlockKind::DATA, doc.to_bytes()?));
    ///
    /// let mut edited = resource.read_kv3(BlockKind::DATA)?;
    /// edited.root.as_object_mut().expect("an object").insert("count", 2);
    /// resource.put_kv3(BlockKind::DATA, &edited)?;
    ///
    /// let count = resource.read_kv3(BlockKind::DATA)?.root.get("count").and_then(|v| v.as_i64());
    /// assert_eq!(count, Some(2));
    /// assert_eq!(resource.blocks[0].data, [1, 2, 3]);
    /// # Ok::<(), source2::Error>(())
    /// ```
    pub trait ResourceKv3: Sealed {
        /// Parse the first block tagged `kind` as binary KeyValues 3.
        ///
        /// The block is the whole KV3 block, header included, as resources store it.
        ///
        /// # Errors
        ///
        /// [`Error::BlockNotFound`] when no block has the tag,
        /// [`Error::UnrecognisedFormat`] when it does not start like a KV3 binary block,
        /// and [`Error::Kv3`] when it is truncated or malformed.
        fn read_kv3(&self, kind: BlockKind) -> Result<Document>;

        /// Write `doc` into the first block tagged `kind`, or append a new block with that
        /// tag if there is none.
        ///
        /// The document is written as binary KV3 with its own [`Document::options`], so a
        /// document read from a block is written back in the same revision and codec. Every
        /// other block, the versions, the padding and the surrounding bytes are untouched;
        /// `declared_size`, if the resource set one, is left as it was.
        ///
        /// # Errors
        ///
        /// [`Error::Kv3`] when the document cannot be written. The resource is unchanged on
        /// error.
        fn put_kv3(&mut self, kind: BlockKind, doc: &Document) -> Result<()>;
    }

    impl ResourceKv3 for Resource {
        fn read_kv3(&self, kind: BlockKind) -> Result<Document> {
            let block = self
                .block(kind)
                .ok_or_else(|| Error::BlockNotFound(kind.to_string()))?;
            super::kv3_from_binary(&block.data)
        }

        fn put_kv3(&mut self, kind: BlockKind, doc: &Document) -> Result<()> {
            let bytes = doc.to_bytes()?;
            match self.blocks.iter_mut().find(|b| b.kind == kind) {
                Some(block) => block.data = bytes,
                None => self.blocks.push(Block::new(kind, bytes)),
            }
            Ok(())
        }
    }
}

#[cfg(all(feature = "vpk", feature = "resource"))]
pub use vpk_resource::VpkResource;

#[cfg(all(feature = "vpk", feature = "resource"))]
mod vpk_resource {
    use source2_resource::Resource;
    use source2_vpk::Vpk;

    use super::pack::{put_entry, read_entry};
    use super::sealed::Sealed;
    use crate::Result;

    /// Read and write a [`Vpk`] entry as a compiled [`Resource`].
    ///
    /// ```
    /// use source2::ext::VpkResource;
    /// use source2::resource::{Block, BlockKind, Resource};
    /// use source2::vpk::Vpk;
    ///
    /// let resource = Resource::new().with_block(Block::new(BlockKind::DATA, vec![1, 2, 3]));
    /// let mut pack = Vpk::new(2);
    /// pack.put_resource("things/a.thing_c", &resource)?;
    /// assert_eq!(pack.read_resource("things/a.thing_c")?, resource);
    /// # Ok::<(), source2::Error>(())
    /// ```
    pub trait VpkResource: Sealed {
        /// Read the entry at `path` and parse it as a compiled resource.
        ///
        /// # Errors
        ///
        /// [`Error::EntryNotFound`](crate::Error::EntryNotFound) when the pack has no such
        /// path, [`Error::Vpk`](crate::Error::Vpk) when the entry cannot be read (including
        /// a CRC mismatch), and [`Error::Resource`](crate::Error::Resource) when the bytes
        /// are not a resource or are truncated.
        fn read_resource(&self, path: &str) -> Result<Resource>;

        /// Add `resource` at `path`, or replace what is there.
        ///
        /// A pack may list one path more than once, and `Vpk::remove` drops a single entry
        /// per call. Replacing removes every entry with this path, then adds one. The new
        /// entry keeps the placement of the first old one: inline in the directory file if
        /// that was, otherwise (and for a path that is new) bound for the numbered
        /// archives, like `Vpk::add`, so `to_bytes` fails until the pack is written with
        /// `write` or `build`. A preload on the old entry is not carried over, the CRC is
        /// recomputed, and the entry may move within the directory to sit with its
        /// neighbours. Other entries are untouched.
        ///
        /// # Errors
        ///
        /// [`Error::Resource`](crate::Error::Resource) when the resource cannot be written
        /// and [`Error::Vpk`](crate::Error::Vpk) for a path the tree cannot hold. The pack
        /// is unchanged on error.
        fn put_resource(&mut self, path: &str, resource: &Resource) -> Result<()>;
    }

    impl VpkResource for Vpk {
        fn read_resource(&self, path: &str) -> Result<Resource> {
            Ok(Resource::parse(&read_entry(self, path)?)?)
        }

        fn put_resource(&mut self, path: &str, resource: &Resource) -> Result<()> {
            put_entry(self, path, resource.to_bytes()?)
        }
    }
}

#[cfg(all(feature = "vpk", feature = "kv1"))]
pub use vpk_kv1::VpkKv1;

#[cfg(all(feature = "vpk", feature = "kv1"))]
mod vpk_kv1 {
    use source2_kv1::Document;
    use source2_vpk::Vpk;

    use super::pack::{put_entry, read_entry};
    use super::sealed::Sealed;
    use crate::Result;

    /// Read and write a [`Vpk`] entry as a KeyValues 1 [`Document`], in text or binary.
    ///
    /// KeyValues 1 text has no magic, so the encoding is the method you call.
    ///
    /// ```
    /// use source2::ext::VpkKv1;
    /// use source2::kv1::Document;
    /// use source2::vpk::Vpk;
    ///
    /// let doc = Document::parse(r#""Settings" { "name" "demo" }"#)?;
    /// let mut pack = Vpk::new(2);
    /// pack.put_kv1_text("cfg/settings.vdf", &doc)?;
    /// assert_eq!(pack.read_kv1_text("cfg/settings.vdf")?, doc);
    /// # Ok::<(), source2::Error>(())
    /// ```
    pub trait VpkKv1: Sealed {
        /// Read the entry at `path` as KeyValues 1 text (UTF-8, else Windows-1252).
        ///
        /// # Errors
        ///
        /// [`Error::EntryNotFound`](crate::Error::EntryNotFound), a
        /// [`Error::Vpk`](crate::Error::Vpk) read failure, or
        /// [`Error::Kv1`](crate::Error::Kv1) for text that does not parse.
        fn read_kv1_text(&self, path: &str) -> Result<Document>;

        /// Read the entry at `path` as binary KeyValues 1. Errors as
        /// [`read_kv1_text`](Self::read_kv1_text).
        fn read_kv1_binary(&self, path: &str) -> Result<Document>;

        /// Add or replace the entry at `path` with `doc` as KeyValues 1 text, in the
        /// document's own encoding. Placement and replacement follow
        /// [`VpkResource::put_resource`](super::VpkResource::put_resource).
        ///
        /// # Errors
        ///
        /// [`Error::Kv1`](crate::Error::Kv1) when the document cannot be written as text
        /// (typed values need `Document::stringified` first, which is lossy and so is
        /// never applied for you), and [`Error::Vpk`](crate::Error::Vpk) for a bad path.
        fn put_kv1_text(&mut self, path: &str, doc: &Document) -> Result<()>;

        /// Add or replace the entry at `path` with `doc` as binary KeyValues 1. Errors as
        /// [`put_kv1_text`](Self::put_kv1_text).
        fn put_kv1_binary(&mut self, path: &str, doc: &Document) -> Result<()>;
    }

    impl VpkKv1 for Vpk {
        fn read_kv1_text(&self, path: &str) -> Result<Document> {
            Ok(Document::parse_bytes(&read_entry(self, path)?)?)
        }

        fn read_kv1_binary(&self, path: &str) -> Result<Document> {
            Ok(Document::from_binary(&read_entry(self, path)?)?)
        }

        fn put_kv1_text(&mut self, path: &str, doc: &Document) -> Result<()> {
            put_entry(self, path, doc.to_text_bytes()?)
        }

        fn put_kv1_binary(&mut self, path: &str, doc: &Document) -> Result<()> {
            put_entry(self, path, doc.to_binary()?)
        }
    }
}

#[cfg(all(feature = "vpk", feature = "kv2"))]
pub use vpk_kv2::VpkKv2;

#[cfg(all(feature = "vpk", feature = "kv2"))]
mod vpk_kv2 {
    use source2_kv2::Document;
    use source2_vpk::Vpk;

    use super::pack::{put_entry, read_entry};
    use super::sealed::Sealed;
    use crate::Result;

    /// Read and write a [`Vpk`] entry as a KeyValues 2 (DMX) [`Document`].
    ///
    /// The header line says whether a file is text or binary, so one method each way is
    /// enough: a document is written in the encoding it carries.
    ///
    /// ```
    /// use source2::ext::VpkKv2;
    /// use source2::kv2::{Document, Element};
    /// use source2::vpk::Vpk;
    ///
    /// let mut doc = Document::new("dmx", 1);
    /// doc.add_element(Element::new("Root").attr("size", 3));
    /// let mut pack = Vpk::new(2);
    /// pack.put_kv2("scenes/a.dmx", &doc)?;
    /// assert_eq!(pack.read_kv2("scenes/a.dmx")?.to_bytes()?, doc.to_bytes()?);
    /// # Ok::<(), source2::Error>(())
    /// ```
    pub trait VpkKv2: Sealed {
        /// Read the entry at `path` as KeyValues 2, text or binary.
        ///
        /// # Errors
        ///
        /// [`Error::EntryNotFound`](crate::Error::EntryNotFound), a
        /// [`Error::Vpk`](crate::Error::Vpk) read failure, or
        /// [`Error::Kv2`](crate::Error::Kv2) when the bytes do not parse.
        fn read_kv2(&self, path: &str) -> Result<Document>;

        /// Add or replace the entry at `path` with `doc`, in the document's own encoding.
        /// Placement and replacement follow
        /// [`VpkResource::put_resource`](super::VpkResource::put_resource).
        ///
        /// # Errors
        ///
        /// [`Error::Kv2`](crate::Error::Kv2) when the document is invalid in its encoding,
        /// and [`Error::Vpk`](crate::Error::Vpk) for a bad path.
        fn put_kv2(&mut self, path: &str, doc: &Document) -> Result<()>;
    }

    impl VpkKv2 for Vpk {
        fn read_kv2(&self, path: &str) -> Result<Document> {
            Ok(Document::parse(&read_entry(self, path)?)?)
        }

        fn put_kv2(&mut self, path: &str, doc: &Document) -> Result<()> {
            put_entry(self, path, doc.to_bytes()?)
        }
    }
}

#[cfg(all(feature = "vpk", feature = "kv3"))]
pub use vpk_kv3::VpkKv3;

#[cfg(all(feature = "vpk", feature = "kv3"))]
mod vpk_kv3 {
    use source2_kv3::Document;
    use source2_vpk::Vpk;

    use super::pack::{put_entry, read_entry};
    use super::sealed::Sealed;
    use crate::{Error, Result};

    /// Read and write a [`Vpk`] entry as a KeyValues 3 [`Document`], in binary or text.
    ///
    /// The encoding is the method you call, never a guess from the extension.
    ///
    /// ```
    /// use source2::ext::VpkKv3;
    /// use source2::kv3::{Document, Object};
    /// use source2::vpk::Vpk;
    ///
    /// let doc = Document::new(Object::from_iter([("count", 3)]));
    /// let mut pack = Vpk::new(2);
    /// pack.put_kv3("data/a.kv3", &doc)?;
    /// pack.put_kv3_text("data/b.kv3", &doc)?;
    /// assert_eq!(pack.read_kv3("data/a.kv3")?.root, doc.root);
    /// assert_eq!(pack.read_kv3_text("data/b.kv3")?.root, doc.root);
    /// # Ok::<(), source2::Error>(())
    /// ```
    pub trait VpkKv3: Sealed {
        /// Read the entry at `path` as a binary KV3 block.
        ///
        /// # Errors
        ///
        /// [`Error::EntryNotFound`], a [`Error::Vpk`] read failure,
        /// [`Error::UnrecognisedFormat`] when the bytes do not start like a binary KV3
        /// block (text KV3 included), and [`Error::Kv3`] when it is truncated or malformed.
        fn read_kv3(&self, path: &str) -> Result<Document>;

        /// Read the entry at `path` as KV3 text. The header comment is optional.
        ///
        /// # Errors
        ///
        /// [`Error::EntryNotFound`], a [`Error::Vpk`] read failure, [`Error::NotUtf8`], and
        /// [`Error::Kv3`] for text that does not parse.
        fn read_kv3_text(&self, path: &str) -> Result<Document>;

        /// Add or replace the entry at `path` with `doc` as binary KV3, using the
        /// document's own revision and codec. Placement and replacement follow
        /// [`VpkResource::put_resource`](super::VpkResource::put_resource).
        ///
        /// # Errors
        ///
        /// [`Error::Kv3`] when the document cannot be written and [`Error::Vpk`] for a bad
        /// path. The pack is unchanged on error.
        fn put_kv3(&mut self, path: &str, doc: &Document) -> Result<()>;

        /// Add or replace the entry at `path` with `doc` as KV3 text. Text keeps the
        /// values but not binary storage hints such as integer widths. Errors as
        /// [`put_kv3`](Self::put_kv3).
        fn put_kv3_text(&mut self, path: &str, doc: &Document) -> Result<()>;
    }

    impl VpkKv3 for Vpk {
        fn read_kv3(&self, path: &str) -> Result<Document> {
            super::kv3_from_binary(&read_entry(self, path)?)
        }

        fn read_kv3_text(&self, path: &str) -> Result<Document> {
            let bytes = read_entry(self, path)?;
            let text = std::str::from_utf8(&bytes).map_err(|_| Error::NotUtf8(path.to_owned()))?;
            Ok(Document::from_text(text)?)
        }

        fn put_kv3(&mut self, path: &str, doc: &Document) -> Result<()> {
            put_entry(self, path, doc.to_bytes()?)
        }

        fn put_kv3_text(&mut self, path: &str, doc: &Document) -> Result<()> {
            put_entry(self, path, doc.to_text()?.into_bytes())
        }
    }
}

#[cfg(all(feature = "vpk", feature = "resource", feature = "kv3"))]
pub use vpk_resource_kv3::VpkResourceKv3;

#[cfg(all(feature = "vpk", feature = "resource", feature = "kv3"))]
mod vpk_resource_kv3 {
    use source2_kv3::Document;
    use source2_resource::BlockKind;
    use source2_vpk::Vpk;

    use super::sealed::Sealed;
    use super::{ResourceKv3, VpkResource};
    use crate::Result;

    /// Read and write the KeyValues 3 block of a compiled resource inside a [`Vpk`], in one
    /// call. Composes [`VpkResource`] and [`ResourceKv3`].
    ///
    /// ```
    /// use source2::ext::{VpkResource, VpkResourceKv3};
    /// use source2::kv3::{Document, Object};
    /// use source2::resource::{Block, BlockKind, Resource};
    /// use source2::vpk::Vpk;
    ///
    /// let doc = Document::new(Object::from_iter([("count", 1)]));
    /// let resource = Resource::new().with_block(Block::new(BlockKind::DATA, doc.to_bytes()?));
    /// let mut pack = Vpk::new(2);
    /// pack.put_resource("things/a.thing_c", &resource)?;
    ///
    /// let mut data = pack.read_resource_kv3("things/a.thing_c", BlockKind::DATA)?;
    /// data.root.as_object_mut().expect("an object").insert("count", 2);
    /// pack.put_resource_kv3("things/a.thing_c", BlockKind::DATA, &data)?;
    ///
    /// let again = pack.read_resource_kv3("things/a.thing_c", BlockKind::DATA)?;
    /// assert_eq!(again.root.get("count").and_then(|v| v.as_i64()), Some(2));
    /// # Ok::<(), source2::Error>(())
    /// ```
    pub trait VpkResourceKv3: Sealed {
        /// Read the entry at `path` as a resource and parse its first `kind` block as KV3.
        ///
        /// # Errors
        ///
        /// Those of [`VpkResource::read_resource`] and [`ResourceKv3::read_kv3`].
        fn read_resource_kv3(&self, path: &str, kind: BlockKind) -> Result<Document>;

        /// Read the resource at `path`, put `doc` into its `kind` block, and store the
        /// result back under the same path. Only that block changes. The entry must exist;
        /// use [`VpkResource::put_resource`] to create one.
        ///
        /// # Errors
        ///
        /// Those of [`VpkResource::read_resource`], [`ResourceKv3::put_kv3`] and
        /// [`VpkResource::put_resource`]. The pack is unchanged on error.
        fn put_resource_kv3(&mut self, path: &str, kind: BlockKind, doc: &Document) -> Result<()>;
    }

    impl VpkResourceKv3 for Vpk {
        fn read_resource_kv3(&self, path: &str, kind: BlockKind) -> Result<Document> {
            self.read_resource(path)?.read_kv3(kind)
        }

        fn put_resource_kv3(&mut self, path: &str, kind: BlockKind, doc: &Document) -> Result<()> {
            let mut resource = self.read_resource(path)?;
            resource.put_kv3(kind, doc)?;
            self.put_resource(path, &resource)
        }
    }
}
