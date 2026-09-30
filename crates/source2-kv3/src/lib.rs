#![forbid(unsafe_code)]

//! Binary KV3: header and decompression.
//!
//! A `vdata_c`'s `DATA` block is Valve's KeyValues 3 in its binary form. Deadlock ships
//! two revisions: **version 5** (`KV3\x05`) for all but six `scripts/*.vdata_c`, and
//! **version 4** (`KV3\x04`) for those six. Either may be stored as-is or compressed with
//! zstd or LZ4; the method is per file, LZ4 is the common case, and only `heroes` and
//! `abilities` are zstd.
//!
//! This module gets from the block to the decompressed payload. Turning that payload into
//! a value tree is [`value`]'s job.
//!
//! # v5 splits the payload in two, v4 does not
//!
//! In v5, buffer 1 holds the pools - strings, and the 1/2/4/8-byte operands - and buffer
//! 2 holds the structure that gives them meaning. Each is compressed on its own, and the
//! header sizes them separately: offsets 72/76 are buffer 1's uncompressed and compressed
//! lengths, 80/84 are buffer 2's, and 48/52 are the totals.
//!
//! Reading a total where a buffer size belongs, or the reverse, is the mistake this
//! layout invites: a decode validated against offset 72 accepts a payload that stopped
//! after the string table, which looks plausible rather than obviously broken.
//!
//! A v4 payload is one buffer. Offsets 72 onwards are not a shorter form of the same
//! fields, they are where the payload starts, so the four per-buffer sizes read zero and
//! 48/52 are the only lengths there are.
//!
//! # Why the compression paths differ
//!
//! A zstd frame is self-delimiting, so the whole payload decodes in one call that walks
//! both frames. **A raw LZ4 block is not** - nothing in the byte stream says where one
//! ends - so on v5 the LZ4 path has to cut the payload at buffer 1's compressed length
//! and decode each buffer against its own declared output size. v4's single buffer needs
//! no cut: the whole payload is one block, sized by offset 48.
//!
//! `frame_size` (16384 on every LZ4 file, 0 on every zstd one) is not that boundary and
//! must not be used as one: `scripts/npc_units.vdata_c` has a 28,344-byte buffer 1 that
//! decodes as a single block. Whether a v4 payload larger than `frame_size` would be
//! split is untested - the largest Deadlock ships decompresses to 6,527 bytes - and one
//! that did split would fail the length check rather than decode partially.
//!
//! # Locating the payload
//!
//! The header is a fixed 120 bytes in v5 and 72 in v4, but rather than trusting either
//! constant this derives the payload offset as `block_length - compressed_size`, then
//! checks the result lands past the header. A file whose header grows by a field still
//! reads correctly, and a file that is not what we think it is fails loudly instead of
//! decompressing garbage.

pub mod error;

use crate::error::{Error, Result};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod write_tests;

/// `KV3\x05` — the revision Deadlock ships.
pub const MAGIC_V5: u32 = 0x4B56_3305;
/// `KV3\x04`.
pub const MAGIC_V4: u32 = 0x4B56_3304;
/// `KV3\x03`.
pub const MAGIC_V3: u32 = 0x4B56_3303;

pub mod value;
pub use value::{Document, Object, Value, parse};

pub mod writer;
pub use writer::{Version, WriteOptions, write};

/// First four bytes of a zstd frame.
#[cfg(feature = "zstd")]
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

/// How a KV3 payload is compressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compression {
    /// Stored as-is.
    None,
    /// LZ4 block compression, used by older Source 2 titles.
    Lz4,
    /// Zstandard. What Deadlock ships.
    Zstd,
    /// A method this reader does not know.
    Unknown(u32),
}

impl Compression {
    fn from_raw(v: u32) -> Self {
        match v {
            0 => Compression::None,
            1 => Compression::Lz4,
            2 => Compression::Zstd,
            other => Compression::Unknown(other),
        }
    }
}

/// The parts of a binary KV3 header this reader uses.
///
/// The format carries more fields than these; the ones left out describe the internal
/// buffer layout and belong to the value decoder rather than here.
#[derive(Clone, Copy, Debug)]
pub struct Header {
    /// Revision: 3, 4 or 5.
    pub version: u8,
    /// Format GUID, identifying the schema the payload follows.
    pub format: [u8; 16],
    /// How the payload is compressed.
    pub compression: Compression,
    /// Dictionary id, for compression schemes that use one. Always 0 in Deadlock.
    pub dictionary_id: u16,
    /// Compressor frame size. 16384 on every LZ4 file Deadlock ships, 0 on every zstd one.
    ///
    /// Advisory: it is not a block boundary, and buffers larger than it decode as one
    /// block.
    pub frame_size: u16,
    /// Count of single bytes in the decoded value buffer.
    pub binary_byte_count: u32,
    /// Count of 4-byte integers in the decoded value buffer.
    pub integer_count: u32,
    /// Count of 8-byte values in the decoded value buffer.
    pub eight_byte_count: u32,
    /// Count of 2-byte values in buffer 1. Zero in everything Deadlock ships.
    pub two_byte_count: u32,
    /// Size of the type stream in v5, where it lives at the end of buffer 2.
    ///
    /// **The field means something wider in v4**: there it sizes the string blob and
    /// the type stream together, as one region at the end of the single buffer. Reading
    /// it as a type-stream length on a v4 file overshoots by the whole string table.
    pub type_count: u32,
    /// Count of 1-byte values in buffer 2.
    pub byte_count_buffer2: u32,
    /// Count of 2-byte values in buffer 2.
    pub two_byte_count_buffer2: u32,
    /// Count of 4-byte values in buffer 2.
    pub integer_count_buffer2: u32,
    /// Count of 8-byte values in buffer 2.
    pub eight_byte_count_buffer2: u32,
    /// Number of objects in buffer 2, and so the length of its object-length table.
    pub object_count_buffer2: u32,
    /// Compressed length of the whole payload, across every frame.
    pub compressed_size: u32,
    /// Length the whole payload decompresses to, across every frame.
    pub uncompressed_size: u32,
    /// Length buffer 1 - the pools - decompresses to.
    ///
    /// Easy to mistake for [`Header::uncompressed_size`], and a reader that validates a
    /// whole-payload decode against it accepts one that stopped after the string table.
    pub buffer1_uncompressed_size: u32,
    /// Compressed length of buffer 1.
    pub buffer1_compressed_size: u32,
    /// Length buffer 2 - the structure - decompresses to.
    pub buffer2_uncompressed_size: u32,
    /// Compressed length of buffer 2.
    pub buffer2_compressed_size: u32,
    /// Where the payload starts within the block.
    pub payload_offset: usize,
}

/// Byte offsets of the fields read out of the header.
mod field {
    pub const MAGIC: usize = 0;
    pub const FORMAT: usize = 4;
    pub const COMPRESSION: usize = 20;
    pub const DICTIONARY_ID: usize = 24;
    pub const FRAME_SIZE: usize = 26;
    pub const BINARY_BYTES: usize = 28;
    pub const INTEGERS: usize = 32;
    pub const EIGHT_BYTES: usize = 36;
    pub const TYPE_COUNT: usize = 40;
    pub const TWO_BYTES: usize = 64;
    /// Totals across every frame. Verified against both files Deadlock ships.
    pub const UNCOMPRESSED_SIZE: usize = 48;
    pub const COMPRESSED_SIZE: usize = 52;
    /// Per-buffer sizes - deceptively similar to the totals above.
    pub const BUFFER1_UNCOMPRESSED: usize = 72;
    pub const BUFFER1_COMPRESSED: usize = 76;
    pub const BUFFER2_UNCOMPRESSED: usize = 80;
    pub const BUFFER2_COMPRESSED: usize = 84;
    /// Buffer 2's own pool counts.
    pub const B2_BYTES: usize = 88;
    pub const B2_TWO_BYTES: usize = 92;
    pub const B2_INTEGERS: usize = 96;
    pub const B2_EIGHT_BYTES: usize = 100;
    pub const B2_OBJECTS: usize = 108;
    /// Everything above has to be present for a v5 header to be readable.
    ///
    /// A v5 header is 120 bytes; the buffer-2 counts sit in the tail, so the value model
    /// needs the lot rather than the first 80.
    pub const MIN_LEN: usize = 120;
    /// A v3 or v4 header, which stops before the four per-buffer sizes.
    ///
    /// Measured across the six pre-v5 files Deadlock ships: `scale_functions`'s `DATA`
    /// block is 132 bytes and declares 60 compressed, which puts the payload at 72, and
    /// its first pool word is already there. Offsets 64 to 71 are inside the header and
    /// read zero on every one of the six, so what they hold is unverified.
    pub const MIN_LEN_V4: usize = 72;
}

impl Header {
    /// Read the header of a binary KV3 block.
    ///
    /// # Errors
    ///
    /// If the block is too short, does not start with a KV3 magic this reader knows, or
    /// declares a compressed size that does not fit inside it.
    pub fn parse(block: &[u8]) -> Result<Self> {
        if block.len() < field::MIN_LEN_V4 {
            return Err(Error::Malformed(format!(
                "KV3 block is {} bytes, need at least {}",
                block.len(),
                field::MIN_LEN_V4
            )));
        }
        let u16_at = |o: usize| u16::from_le_bytes([block[o], block[o + 1]]);
        let u32_at =
            |o: usize| u32::from_le_bytes([block[o], block[o + 1], block[o + 2], block[o + 3]]);

        let magic = u32_at(field::MAGIC);
        let version = match magic {
            MAGIC_V5 => 5,
            MAGIC_V4 => 4,
            MAGIC_V3 => 3,
            other => {
                return Err(Error::Malformed(format!(
                    "not binary KV3: magic {other:#010x}"
                )));
            }
        };

        let header_len = if version >= 5 {
            field::MIN_LEN
        } else {
            field::MIN_LEN_V4
        };
        if block.len() < header_len {
            return Err(Error::Malformed(format!(
                "KV3 v{version} block is {} bytes, its header alone is {header_len}",
                block.len()
            )));
        }

        // Everything from offset 72 on lives past the end of a v3/v4 header, where the
        // payload itself begins. Reading those fields anyway yields whatever the body
        // happens to start with, which then sizes a decode: two shipped v4 files produced
        // buffer claims of 201 MB and 16 MB out of payloads of 349 and 2023 bytes. On a
        // short v4 block they are not merely wrong, they are off the end of it.
        let v5_only = |o: usize| if version >= 5 { u32_at(o) } else { 0 };

        let mut format = [0u8; 16];
        format.copy_from_slice(&block[field::FORMAT..field::FORMAT + 16]);

        let compressed_size = u32_at(field::COMPRESSED_SIZE);
        // Derived rather than assumed: see the module docs.
        let payload_offset = block
            .len()
            .checked_sub(compressed_size as usize)
            .ok_or_else(|| {
                Error::Malformed(format!(
                    "KV3 declares {compressed_size} compressed bytes in a {} byte block",
                    block.len()
                ))
            })?;
        if payload_offset < header_len {
            return Err(Error::Malformed(format!(
                "KV3 payload would start at {payload_offset}, inside a {header_len}-byte header"
            )));
        }

        Ok(Header {
            version,
            format,
            compression: Compression::from_raw(u32_at(field::COMPRESSION)),
            dictionary_id: u16_at(field::DICTIONARY_ID),
            frame_size: u16_at(field::FRAME_SIZE),
            binary_byte_count: u32_at(field::BINARY_BYTES),
            integer_count: u32_at(field::INTEGERS),
            eight_byte_count: u32_at(field::EIGHT_BYTES),
            two_byte_count: u32_at(field::TWO_BYTES),
            type_count: u32_at(field::TYPE_COUNT),
            byte_count_buffer2: v5_only(field::B2_BYTES),
            two_byte_count_buffer2: v5_only(field::B2_TWO_BYTES),
            integer_count_buffer2: v5_only(field::B2_INTEGERS),
            eight_byte_count_buffer2: v5_only(field::B2_EIGHT_BYTES),
            object_count_buffer2: v5_only(field::B2_OBJECTS),
            compressed_size,
            uncompressed_size: u32_at(field::UNCOMPRESSED_SIZE),
            buffer1_uncompressed_size: v5_only(field::BUFFER1_UNCOMPRESSED),
            buffer1_compressed_size: v5_only(field::BUFFER1_COMPRESSED),
            buffer2_uncompressed_size: v5_only(field::BUFFER2_UNCOMPRESSED),
            buffer2_compressed_size: v5_only(field::BUFFER2_COMPRESSED),
            payload_offset,
        })
    }
}

/// A decoded KV3 block: its header, and the payload with compression undone.
#[derive(Clone, Debug)]
pub struct Decoded {
    /// The header this came from.
    pub header: Header,
    /// The decompressed payload.
    pub payload: Vec<u8>,
}

/// Read a binary KV3 block and undo its compression.
///
/// # Errors
///
/// If the header will not parse, the compression method is one this reader does not
/// implement, the zstd frame is corrupt, or the result is not the length the header
/// promised.
pub fn decode(block: &[u8]) -> Result<Decoded> {
    let header = Header::parse(block)?;
    let payload = &block[header.payload_offset..];

    let out = match header.compression {
        Compression::None => payload.to_vec(),
        Compression::Zstd => decompress_zstd(payload, header.uncompressed_size as usize)?,
        Compression::Lz4 => decompress_lz4(payload, &header)?,
        Compression::Unknown(v) => {
            return Err(Error::Malformed(format!(
                "KV3 compression method {v} is not one this reader knows"
            )));
        }
    };

    // Checked against the total, not the first frame's figure: getting this wrong is
    // exactly how a partial decode passes for a complete one.
    if out.len() != header.uncompressed_size as usize {
        return Err(Error::Malformed(format!(
            "KV3 decompressed to {} bytes, header says {} across all frames",
            out.len(),
            header.uncompressed_size
        )));
    }

    Ok(Decoded {
        header,
        payload: out,
    })
}

#[cfg(feature = "zstd")]
fn decompress_zstd(payload: &[u8], expected: usize) -> Result<Vec<u8>> {
    if payload.get(..4) != Some(&ZSTD_MAGIC) {
        return Err(Error::Malformed(format!(
            "expected a zstd frame, found {:02x?}",
            payload.get(..4).unwrap_or(payload)
        )));
    }
    // `decode_all_to_vec` walks every frame in the input; a streaming read would stop at
    // the first boundary and hand back part of the payload. It fills the vector's spare
    // capacity and never grows it, so the reservation has to be the full expected size.
    let mut out = Vec::with_capacity(expected);
    ruzstd::decoding::FrameDecoder::new()
        .decode_all_to_vec(payload, &mut out)
        .map_err(|e| Error::Malformed(format!("zstd decode: {e}")))?;
    Ok(out)
}

#[cfg(not(feature = "zstd"))]
fn decompress_zstd(_payload: &[u8], _expected: usize) -> Result<Vec<u8>> {
    Err(Error::Malformed(
        "this build has no zstd decoder; enable the `zstd` feature".into(),
    ))
}

/// Undo LZ4 compression, one buffer at a time.
///
/// Raw LZ4 blocks carry no magic and no length, so unlike the zstd path this cannot hand
/// the whole payload to the decoder and let it find the boundary: on v5 it has to cut at
/// buffer 1's compressed length and decode each side against its own declared output
/// size. Both sizes are load-bearing rather than hints, and a wrong one fails here
/// instead of yielding a short result.
///
/// v4 has no such cut to make. Its payload is a single buffer, so the block is the whole
/// payload and the size to decode against is the header's total.
#[cfg(feature = "lz4")]
fn decompress_lz4(payload: &[u8], header: &Header) -> Result<Vec<u8>> {
    if header.version < 5 {
        return lz4_block(payload, header.uncompressed_size as usize, "the payload");
    }
    let split = header.buffer1_compressed_size as usize;
    let end = split.saturating_add(header.buffer2_compressed_size as usize);
    if end > payload.len() {
        return Err(Error::Malformed(format!(
            "KV3 buffers claim {end} compressed bytes, payload has {}",
            payload.len()
        )));
    }
    let one = lz4_block(
        &payload[..split],
        header.buffer1_uncompressed_size as usize,
        "buffer 1",
    )?;
    let two = lz4_block(
        &payload[split..end],
        header.buffer2_uncompressed_size as usize,
        "buffer 2",
    )?;
    let mut out = Vec::with_capacity(one.len() + two.len());
    out.extend_from_slice(&one);
    out.extend_from_slice(&two);
    Ok(out)
}

/// Decode one LZ4 buffer against its declared output size.
///
/// A zero-length buffer is legitimate - a document with no structure to speak of - and
/// contributes nothing rather than failing.
#[cfg(feature = "lz4")]
fn lz4_block(input: &[u8], expected: usize, which: &str) -> Result<Vec<u8>> {
    if expected == 0 {
        return Ok(Vec::new());
    }
    lz4_flex::block::decompress(input, expected)
        .map_err(|e| Error::Malformed(format!("lz4 decode of {which}: {e}")))
}

#[cfg(not(feature = "lz4"))]
fn decompress_lz4(_payload: &[u8], _header: &Header) -> Result<Vec<u8>> {
    Err(Error::Malformed(
        "this build has no LZ4 decoder; enable the `lz4` feature".into(),
    ))
}
