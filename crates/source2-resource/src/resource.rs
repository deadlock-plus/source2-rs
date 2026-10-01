//! The compiled-resource model, reader and writer.

use std::io::{Read, Write};

use crate::error::{Error, Result};
use crate::kind::BlockKind;

/// The only header version this crate reads and writes.
pub const HEADER_VERSION: u16 = 12;

/// Bytes of the fixed header: size, versions, table offset, block count.
const HEADER_LEN: usize = 16;
/// Bytes of one table entry: tag, offset, length.
const ENTRY_LEN: usize = 12;
/// Alignment of automatically padded blocks.
const AUTO_ALIGN: usize = 16;

/// The two version numbers in the header.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Versions {
    /// Container layout version. Only `12` is supported.
    pub header: u16,
    /// Per-type revision of the contents. Stored as given.
    pub resource: u16,
}

impl Versions {
    /// Versions with the given header and resource numbers.
    ///
    /// The struct is `non_exhaustive`, so outside this crate it is built with this
    /// constructor or [`Versions::default`] and then edited through its public fields.
    pub const fn new(header: u16, resource: u16) -> Self {
        Versions { header, resource }
    }
}

impl Default for Versions {
    fn default() -> Self {
        Versions {
            header: HEADER_VERSION,
            resource: 0,
        }
    }
}

/// Bytes between the end of the previous section and the start of a block.
///
/// `non_exhaustive`: matches outside this crate need a wildcard arm.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Padding {
    /// Zero bytes up to the next 16-byte boundary of the file.
    #[default]
    Auto,
    /// Exactly these bytes.
    Exact(Vec<u8>),
}

/// One block: a tag and its bytes.
///
/// `non_exhaustive`: outside this crate, build it with [`Block::new`] and
/// [`Block::with_padding`], or assign the public fields.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Block {
    /// The block's tag.
    pub kind: BlockKind,
    /// The block's bytes, opaque to this crate.
    pub data: Vec<u8>,
    /// Padding written before the block. Offsets and lengths are derived when writing.
    pub padding: Padding,
}

impl Block {
    /// A block with default padding.
    pub fn new(kind: BlockKind, data: Vec<u8>) -> Self {
        Block {
            kind,
            data,
            padding: Padding::Auto,
        }
    }

    /// Set the padding written before the block.
    #[must_use]
    pub fn with_padding(mut self, padding: Padding) -> Self {
        self.padding = padding;
        self
    }
}

/// A compiled-resource container.
///
/// `non_exhaustive`: outside this crate, start from [`Resource::new`] and use the
/// `with_*` builders or assign the public fields.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Resource {
    /// Header and resource versions.
    pub versions: Versions,
    /// Blocks in table order. Their bytes are laid out in the same order.
    pub blocks: Vec<Block>,
    /// Bytes between the 16-byte header and the block table.
    pub pre_table: Vec<u8>,
    /// Bytes after the last block, outside the stored size.
    pub trailing: Vec<u8>,
    /// The size field when it differs from the end of the last block. `None` writes the
    /// end of the last block, which is what real files store.
    pub declared_size: Option<u32>,
}

fn auto_padding(cursor: u64) -> usize {
    let align = AUTO_ALIGN as u64;
    ((align - cursor % align) % align) as usize
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn to_u32(value: u64, what: &'static str) -> Result<u32> {
    u32::try_from(value).map_err(|_| Error::TooLarge { what })
}

impl Resource {
    /// An empty resource with default versions.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the header and resource versions.
    #[must_use]
    pub fn with_versions(mut self, versions: Versions) -> Self {
        self.versions = versions;
        self
    }

    /// Append a block.
    #[must_use]
    pub fn with_block(mut self, block: Block) -> Self {
        self.blocks.push(block);
        self
    }

    /// Set the bytes between the header and the block table.
    #[must_use]
    pub fn with_pre_table(mut self, pre_table: Vec<u8>) -> Self {
        self.pre_table = pre_table;
        self
    }

    /// Set the bytes after the last block.
    #[must_use]
    pub fn with_trailing(mut self, trailing: Vec<u8>) -> Self {
        self.trailing = trailing;
        self
    }

    /// Set the size field to write instead of the end of the last block.
    #[must_use]
    pub fn with_declared_size(mut self, declared_size: Option<u32>) -> Self {
        self.declared_size = declared_size;
        self
    }

    /// Append a block with default padding and return it.
    pub fn push_block(&mut self, kind: BlockKind, data: Vec<u8>) -> &mut Block {
        self.blocks.push(Block::new(kind, data));
        let last = self.blocks.len() - 1;
        &mut self.blocks[last]
    }

    /// The first block with this tag.
    pub fn block(&self, kind: BlockKind) -> Option<&Block> {
        self.blocks.iter().find(|b| b.kind == kind)
    }

    /// The bytes of the first `DATA` block.
    pub fn data(&self) -> Option<&[u8]> {
        self.block(BlockKind::DATA).map(|b| b.data.as_slice())
    }

    /// Parse a compiled resource.
    ///
    /// Every byte of `bytes` ends up in the model, so [`Resource::to_bytes`] reproduces
    /// the input.
    ///
    /// # Errors
    ///
    /// [`Error::Truncated`] when the header or table does not fit,
    /// [`Error::UnsupportedVersion`] for a header version other than 12,
    /// [`Error::BadOffset`] when a block runs past the end, and [`Error::BadLayout`] for
    /// tables the model cannot represent.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let available = bytes.len() as u64;
        if bytes.len() < HEADER_LEN {
            return Err(Error::Truncated {
                what: "header",
                needed: HEADER_LEN as u64,
                available,
            });
        }
        let size = u32_at(bytes, 0);
        let versions = Versions {
            header: u16_at(bytes, 4),
            resource: u16_at(bytes, 6),
        };
        if versions.header != HEADER_VERSION {
            return Err(Error::UnsupportedVersion {
                found: versions.header,
            });
        }

        // The table offset is relative to its own field at byte 8.
        let table_rel = u64::from(u32_at(bytes, 8));
        let count = u64::from(u32_at(bytes, 12));
        if table_rel < 8 {
            return Err(Error::BadLayout {
                index: None,
                reason: "block table overlaps the header",
            });
        }
        let table_start = 8 + table_rel;
        // Checking the table against the input length bounds `count` before any allocation.
        let table_end = count
            .checked_mul(ENTRY_LEN as u64)
            .and_then(|t| t.checked_add(table_start))
            .filter(|&end| end <= available)
            .ok_or(Error::Truncated {
                what: "block table",
                needed: count
                    .saturating_mul(ENTRY_LEN as u64)
                    .saturating_add(table_start),
                available,
            })?;

        // Both casts are bounded by `available`.
        let table_start = table_start as usize;
        let mut cursor = table_end as usize;
        let mut blocks = Vec::with_capacity(count as usize);
        for index in 0..count as usize {
            let entry = table_start + index * ENTRY_LEN;
            let kind = BlockKind([
                bytes[entry],
                bytes[entry + 1],
                bytes[entry + 2],
                bytes[entry + 3],
            ]);
            // The offset is relative to the field that holds it.
            let offset = (entry as u64 + 4) + u64::from(u32_at(bytes, entry + 4));
            let length = u64::from(u32_at(bytes, entry + 8));
            if offset < cursor as u64 {
                return Err(Error::BadLayout {
                    index: Some(index),
                    reason: "block starts before the end of the table or the previous block",
                });
            }
            let end = offset + length;
            if end > available {
                return Err(Error::BadOffset {
                    index,
                    kind,
                    offset,
                    length,
                    available,
                });
            }
            let (offset, end) = (offset as usize, end as usize);
            let gap = &bytes[cursor..offset];
            let padding = if gap.len() == auto_padding(cursor as u64) && gap.iter().all(|&b| b == 0)
            {
                Padding::Auto
            } else {
                Padding::Exact(gap.to_vec())
            };
            blocks.push(Block {
                kind,
                data: bytes[offset..end].to_vec(),
                padding,
            });
            cursor = end;
        }

        Ok(Resource {
            versions,
            blocks,
            pre_table: bytes[HEADER_LEN..table_start].to_vec(),
            trailing: bytes[cursor..].to_vec(),
            declared_size: (u64::from(size) != cursor as u64).then_some(size),
        })
    }

    /// Read a whole compiled resource from `reader`.
    ///
    /// # Errors
    ///
    /// I/O failures, and everything [`Resource::parse`] reports.
    pub fn read<R: Read>(mut reader: R) -> Result<Self> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Self::parse(&bytes)
    }

    /// Serialize to `writer`.
    ///
    /// # Errors
    ///
    /// [`Error::UnsupportedVersion`] for a header version other than 12,
    /// [`Error::TooLarge`] when an offset or length overflows 32 bits, and I/O failures.
    pub fn write<W: Write>(&self, mut writer: W) -> Result<()> {
        if self.versions.header != HEADER_VERSION {
            return Err(Error::UnsupportedVersion {
                found: self.versions.header,
            });
        }
        let table_start = (HEADER_LEN + self.pre_table.len()) as u64;
        let table_rel = to_u32(table_start - 8, "table offset")?;
        let count = to_u32(self.blocks.len() as u64, "block count")?;

        let mut cursor = table_start + self.blocks.len() as u64 * ENTRY_LEN as u64;
        let mut table = Vec::with_capacity(self.blocks.len() * ENTRY_LEN);
        let mut gaps = Vec::with_capacity(self.blocks.len());
        for (index, block) in self.blocks.iter().enumerate() {
            let gap_len = match &block.padding {
                Padding::Auto => auto_padding(cursor),
                Padding::Exact(bytes) => bytes.len(),
            };
            let start = cursor + gap_len as u64;
            let entry = table_start + (index * ENTRY_LEN) as u64;
            let rel = to_u32(start - (entry + 4), "block offset")?;
            let length = to_u32(block.data.len() as u64, "block length")?;
            table.extend_from_slice(block.kind.as_bytes());
            table.extend_from_slice(&rel.to_le_bytes());
            table.extend_from_slice(&length.to_le_bytes());
            gaps.push(gap_len);
            cursor = start + u64::from(length);
        }
        let size = match self.declared_size {
            Some(size) => size,
            None => to_u32(cursor, "resource size")?,
        };

        let mut header = [0u8; HEADER_LEN];
        header[0..4].copy_from_slice(&size.to_le_bytes());
        header[4..6].copy_from_slice(&self.versions.header.to_le_bytes());
        header[6..8].copy_from_slice(&self.versions.resource.to_le_bytes());
        header[8..12].copy_from_slice(&table_rel.to_le_bytes());
        header[12..16].copy_from_slice(&count.to_le_bytes());
        writer.write_all(&header)?;
        writer.write_all(&self.pre_table)?;
        writer.write_all(&table)?;
        for (block, &gap_len) in self.blocks.iter().zip(&gaps) {
            match &block.padding {
                Padding::Auto => writer.write_all(&[0u8; AUTO_ALIGN][..gap_len])?,
                Padding::Exact(bytes) => writer.write_all(bytes)?,
            }
            writer.write_all(&block.data)?;
        }
        writer.write_all(&self.trailing)?;
        Ok(())
    }

    /// Serialize to a new buffer.
    ///
    /// # Errors
    ///
    /// As [`Resource::write`].
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        self.write(&mut out)?;
        Ok(out)
    }
}
