//! Source 2 compiled-resource containers.
//!
//! Everything the game compiles - `.vdata_c`, `.vtex_c`, `.vmdl_c` - shares one envelope:
//! a short header, then a table of four-character blocks. `scripts/heroes.vdata_c` carries
//! four of them:
//!
//! | Block | What it is |
//! |---|---|
//! | `RERL` | External references - other resources this one points at |
//! | `RED2` | Edit info: the compiler inputs and settings |
//! | `DATA` | The payload. For a `vdata_c` this is binary KV3 |
//! | `FLCI` | A per-field index the engine uses for fast lookup |
//!
//! Only [`Block::DATA`] matters for reading game data; the rest are recorded so a caller
//! can see what a resource actually contains.
//!
//! Offsets inside the table are relative to the field holding them, not the file, which
//! is the detail that makes a naive parser read garbage.

use crate::error::{Error, Result};

/// Bytes of the fixed header before the block table.
const HEADER_LEN: usize = 8;

/// One block in a compiled resource.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// Four-character tag, e.g. `DATA`.
    pub kind: [u8; 4],
    /// Offset of the block's bytes from the start of the resource.
    pub offset: usize,
    /// Length of the block in bytes.
    pub length: usize,
}

impl Block {
    /// The payload block of a compiled resource.
    pub const DATA: [u8; 4] = *b"DATA";
    /// External resource references.
    pub const RERL: [u8; 4] = *b"RERL";
    /// Edit info, version 2.
    pub const RED2: [u8; 4] = *b"RED2";

    /// The tag as text, for logs and errors.
    pub fn name(&self) -> String {
        String::from_utf8_lossy(&self.kind).into_owned()
    }
}

/// A parsed compiled-resource container.
#[derive(Clone, Debug)]
pub struct Resource<'a> {
    bytes: &'a [u8],
    header_version: u16,
    resource_version: u16,
    blocks: Vec<Block>,
}

impl<'a> Resource<'a> {
    /// Parse the header and block table of a compiled resource.
    ///
    /// # Errors
    ///
    /// If the file is too short, its self-reported size disagrees with its actual length,
    /// or a block points outside it.
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() < 16 {
            return Err(Error::Malformed(format!(
                "resource is {} bytes, too short for a header",
                bytes.len()
            )));
        }
        let u32_at =
            |o: usize| u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
        let u16_at = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]);

        // A compiled resource states its own size first. Checking it here turns "this is
        // not actually a resource" into a clear error instead of a nonsense block table.
        let file_size = u32_at(0) as usize;
        if file_size != bytes.len() {
            return Err(Error::Malformed(format!(
                "resource says {file_size} bytes, got {}",
                bytes.len()
            )));
        }
        let header_version = u16_at(4);
        let resource_version = u16_at(6);

        // Both of these are relative to their own position, not the file start.
        let block_offset = u32_at(8) as usize + HEADER_LEN;
        let block_count = u32_at(12) as usize;

        let mut blocks = Vec::with_capacity(block_count);
        for i in 0..block_count {
            let at = block_offset + i * 12;
            if at + 12 > bytes.len() {
                return Err(Error::Malformed(format!(
                    "block {i} of {block_count} runs past the end"
                )));
            }
            let kind = [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]];
            // The offset is written relative to the field that holds it.
            let offset = at + 4 + u32_at(at + 4) as usize;
            let length = u32_at(at + 8) as usize;
            if offset
                .checked_add(length)
                .is_none_or(|end| end > bytes.len())
            {
                return Err(Error::Malformed(format!(
                    "block {} spans {offset}..{} of {} bytes",
                    String::from_utf8_lossy(&kind),
                    offset.saturating_add(length),
                    bytes.len()
                )));
            }
            blocks.push(Block {
                kind,
                offset,
                length,
            });
        }

        Ok(Resource {
            bytes,
            header_version,
            resource_version,
            blocks,
        })
    }

    /// Header format version. `12` for everything Deadlock ships.
    pub fn header_version(&self) -> u16 {
        self.header_version
    }

    /// Resource format version.
    pub fn resource_version(&self) -> u16 {
        self.resource_version
    }

    /// Every block, in the order the table lists them.
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// The bytes of a block by tag, e.g. `*b"DATA"`.
    pub fn block(&self, kind: [u8; 4]) -> Option<&'a [u8]> {
        let b = self.blocks.iter().find(|b| b.kind == kind)?;
        Some(&self.bytes[b.offset..b.offset + b.length])
    }

    /// The payload block, which is what a caller almost always wants.
    ///
    /// # Errors
    ///
    /// If the resource has no `DATA` block.
    pub fn data(&self) -> Result<&'a [u8]> {
        self.block(Block::DATA)
            .ok_or_else(|| Error::Malformed("resource has no DATA block".into()))
    }
}
