//! The binary block header: revision, compression and the layout fields.

use crate::error::{Error, Result};

/// `KV3\x05`.
pub const MAGIC_V5: u32 = 0x4B56_3305;
/// `KV3\x04`.
pub const MAGIC_V4: u32 = 0x4B56_3304;
/// `KV3\x03`.
pub const MAGIC_V3: u32 = 0x4B56_3303;
/// `KV3\x02`.
pub const MAGIC_V2: u32 = 0x4B56_3302;
/// `KV3\x01`.
pub const MAGIC_V1: u32 = 0x4B56_3301;
/// `VKV\x03`, the encoding that predates the numbered revisions.
pub const MAGIC_LEGACY: u32 = 0x0356_4B56;

/// Which binary revision a block is.
///
/// `Legacy` is the original `VKV\x03` encoding; the rest are the numbered `KV3\x01` to `KV3\x05`
/// magics. `Legacy`, `V1`, `V4` and `V5` are checked against real files; `V2` and `V3` have no
/// known sample and follow the layout the reader infers for them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Version {
    /// `VKV\x03`: one inline stream, strings first.
    Legacy,
    /// `KV3\x01`.
    V1,
    /// `KV3\x02`.
    V2,
    /// `KV3\x03`. Laid out as v4.
    V3,
    /// `KV3\x04`: one buffer, object lengths drawn from the 4-byte pool.
    V4,
    /// `KV3\x05`: two buffers, with a table for object lengths.
    V5,
}

impl Version {
    /// The little-endian magic word that opens a block of this revision.
    #[must_use]
    pub fn magic(self) -> u32 {
        match self {
            Version::Legacy => MAGIC_LEGACY,
            Version::V1 => MAGIC_V1,
            Version::V2 => MAGIC_V2,
            Version::V3 => MAGIC_V3,
            Version::V4 => MAGIC_V4,
            Version::V5 => MAGIC_V5,
        }
    }

    /// The revision a magic word names, if it names one.
    #[must_use]
    pub fn from_magic(magic: u32) -> Option<Self> {
        Some(match magic {
            MAGIC_LEGACY => Version::Legacy,
            MAGIC_V1 => Version::V1,
            MAGIC_V2 => Version::V2,
            MAGIC_V3 => Version::V3,
            MAGIC_V4 => Version::V4,
            MAGIC_V5 => Version::V5,
            _ => return None,
        })
    }

    /// Whether the revision stores its payload as the single buffer v3 and v4 use.
    pub(crate) fn is_single_buffer(self) -> bool {
        matches!(self, Version::V3 | Version::V4)
    }

    /// Whether the revision has a 2-byte pool to draw `INT16` and `UINT16` from.
    pub(crate) fn has_two_byte_pool(self) -> bool {
        matches!(self, Version::V3 | Version::V4 | Version::V5)
    }
}

/// How a KV3 payload is compressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Compression {
    /// Stored as-is.
    None,
    /// LZ4 block compression. Written with a built-in high-compression encoder.
    Lz4,
    /// Zstandard.
    Zstd,
    /// Valve's byte-oriented block scheme, used only by the original `VKV` encoding.
    Block,
    /// A method this crate does not know. A header can carry it, so [`Header::parse`] reports
    /// it; decoding fails with [`Error::Unsupported`], and writing it is refused.
    Unknown(u32),
}

impl Compression {
    pub(crate) fn from_raw(v: u32) -> Self {
        match v {
            0 => Compression::None,
            1 => Compression::Lz4,
            2 => Compression::Zstd,
            other => Compression::Unknown(other),
        }
    }
}

/// The header of a binary KV3 block, as read.
///
/// This is a description of one block's layout, for tools that want to look inside it. It is not
/// what [`write`](crate::write) consumes: the writer derives every count, size and offset from
/// the value tree it is given, and takes only the revision, compression, format GUID,
/// dictionary id and frame size from its [`WriteOptions`](crate::WriteOptions). A
/// [`Document`](crate::Document) keeps those, and
/// [`WriteOptions::try_from`](crate::WriteOptions) turns a header into them. The remaining
/// fields - the counts, the per-buffer sizes, the compressed and uncompressed totals, the
/// payload offset - describe one block and are recomputed every time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Header {
    /// Revision.
    pub version: Version,
    /// Format GUID, identifying the schema the payload follows.
    pub format: [u8; 16],
    /// How the payload is compressed.
    pub compression: Compression,
    /// Dictionary id, for compression schemes that use one. Read from `KV3\x02` and later
    /// headers that carry it, and 0 in every file seen.
    pub dictionary_id: u16,
    /// Compressor frame size. 16384 on every LZ4 file seen, 0 on every zstd one.
    ///
    /// Advisory: it is not a block boundary, and buffers larger than it decode as one block.
    pub frame_size: u16,
    /// Count of single bytes in the decoded value buffer.
    pub binary_byte_count: u32,
    /// Count of 4-byte integers in the decoded value buffer.
    pub integer_count: u32,
    /// Count of 8-byte values in the decoded value buffer.
    pub eight_byte_count: u32,
    /// Count of 2-byte values in buffer 1. Zero in every file seen.
    pub two_byte_count: u32,
    /// Size of the type stream in v5, where it lives at the end of buffer 2.
    ///
    /// **The field means something wider in v4**: there it sizes the string blob and the type
    /// stream together, as one region at the end of the single buffer. Reading it as a
    /// type-stream length on a v4 file overshoots by the whole string table.
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
    /// Number of binary blobs stored after the two buffers (v5 only).
    pub blob_count: u32,
    /// Total length of the blobs once decompressed.
    pub blob_total_size: u32,
    /// Compressed length of the payload. Across both buffers for LZ4; for zstd it also covers
    /// the blob frames when there are any.
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
    pub const FORMAT: usize = 4;
    pub const COMPRESSION: usize = 20;
    pub const DICTIONARY_ID: usize = 24;
    pub const FRAME_SIZE: usize = 26;
    pub const BINARY_BYTES: usize = 28;
    pub const INTEGERS: usize = 32;
    pub const EIGHT_BYTES: usize = 36;
    pub const TYPE_COUNT: usize = 40;
    pub const TWO_BYTES: usize = 64;
    pub const BLOB_COUNT: usize = 56;
    pub const BLOB_TOTAL_SIZE: usize = 60;
    pub const UNCOMPRESSED_SIZE: usize = 48;
    pub const COMPRESSED_SIZE: usize = 52;
    pub const BUFFER1_UNCOMPRESSED: usize = 72;
    pub const BUFFER1_COMPRESSED: usize = 76;
    pub const BUFFER2_UNCOMPRESSED: usize = 80;
    pub const BUFFER2_COMPRESSED: usize = 84;
    pub const B2_BYTES: usize = 88;
    pub const B2_TWO_BYTES: usize = 92;
    pub const B2_INTEGERS: usize = 96;
    pub const B2_EIGHT_BYTES: usize = 100;
    pub const B2_OBJECTS: usize = 108;
}

/// A v5 header is 120 bytes.
pub(crate) const HEADER_LEN_V5: usize = 120;
/// A v3 or v4 header stops before the four per-buffer sizes.
pub(crate) const HEADER_LEN_V4: usize = 72;
/// `KV3\x01`: magic, format, compression and four counts.
pub(crate) const HEADER_LEN_V1: usize = 40;
/// `KV3\x02`: `KV3\x01`'s plus the dictionary id and frame size.
pub(crate) const HEADER_LEN_V2: usize = 44;
/// `VKV\x03`: magic, encoding GUID, format GUID.
pub(crate) const HEADER_LEN_LEGACY: usize = 36;
/// Where a `VKV\x03` payload starts when a length word sits between the header and the data.
pub(crate) const LEGACY_SIZED_PAYLOAD: usize = 40;

pub(crate) const ENCODING_BLOCK: [u8; 16] = [
    0x46, 0x1A, 0x79, 0x95, 0xBC, 0x95, 0x6C, 0x4F, 0xA7, 0x0B, 0x05, 0xBC, 0xA1, 0xB7, 0xDF, 0xD2,
];
pub(crate) const ENCODING_LZ4: [u8; 16] = [
    0x8A, 0x34, 0x47, 0x68, 0xA1, 0x63, 0x5C, 0x4F, 0xA1, 0x97, 0x53, 0x80, 0x6F, 0xD9, 0xB1, 0x19,
];
/// Not seen in any sample file; recalled rather than measured.
pub(crate) const ENCODING_STORED: [u8; 16] = [
    0x00, 0x05, 0x86, 0x1B, 0xD8, 0xF7, 0xC1, 0x40, 0xAD, 0x82, 0x75, 0xA4, 0x82, 0x67, 0xE7, 0x14,
];

pub(crate) fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

pub(crate) fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn blank(version: Version, format: [u8; 16], compression: Compression) -> Header {
    Header {
        version,
        format,
        compression,
        dictionary_id: 0,
        frame_size: 0,
        binary_byte_count: 0,
        integer_count: 0,
        eight_byte_count: 0,
        two_byte_count: 0,
        type_count: 0,
        byte_count_buffer2: 0,
        two_byte_count_buffer2: 0,
        integer_count_buffer2: 0,
        eight_byte_count_buffer2: 0,
        object_count_buffer2: 0,
        blob_count: 0,
        blob_total_size: 0,
        compressed_size: 0,
        uncompressed_size: 0,
        buffer1_uncompressed_size: 0,
        buffer1_compressed_size: 0,
        buffer2_uncompressed_size: 0,
        buffer2_compressed_size: 0,
        payload_offset: 0,
    }
}

impl Header {
    /// Read the header of a binary KV3 block.
    ///
    /// # Errors
    ///
    /// [`Error::Malformed`] if the block is too short, does not start with a KV3 magic, or
    /// declares a compressed size that does not fit inside it.
    pub fn parse(block: &[u8]) -> Result<Self> {
        let version = block
            .get(..4)
            .map(|m| u32_at(m, 0))
            .and_then(Version::from_magic)
            .ok_or_else(|| {
                Error::Malformed(match block.get(..4) {
                    Some(m) => format!("not binary KV3: magic {:#010x}", u32_at(m, 0)),
                    None => format!("KV3 block is {} bytes, need at least 4", block.len()),
                })
            })?;
        match version {
            Version::Legacy => Self::parse_legacy(block),
            Version::V1 | Version::V2 => Self::parse_numbered(block, version),
            Version::V3 | Version::V4 | Version::V5 => Self::parse_late(block, version),
        }
    }

    fn parse_late(block: &[u8], version: Version) -> Result<Self> {
        let header_len = if version == Version::V5 {
            HEADER_LEN_V5
        } else {
            HEADER_LEN_V4
        };
        if block.len() < header_len {
            return Err(Error::Malformed(format!(
                "KV3 block is {} bytes, its header alone is {header_len}",
                block.len()
            )));
        }
        let u32_at = |o: usize| u32_at(block, o);

        // Everything from offset 72 on lives past the end of a v3/v4 header, where the payload
        // itself begins. Reading those fields anyway yields whatever the body happens to start
        // with, which then sizes a decode: shipped v4 files produced buffer claims of 201 MB
        // and 16 MB out of payloads of 349 and 2023 bytes.
        let v5_only = |o: usize| if version == Version::V5 { u32_at(o) } else { 0 };

        let mut format = [0u8; 16];
        format.copy_from_slice(&block[field::FORMAT..field::FORMAT + 16]);

        let compressed_size = u32_at(field::COMPRESSED_SIZE);
        let blob_count = v5_only(field::BLOB_COUNT);
        // Derived rather than assumed. With blobs the block runs on past the buffers, and LZ4
        // files do not count that tail in `compressed_size` while zstd ones do, so the
        // subtraction means nothing and the payload sits right after the header.
        let payload_offset = if blob_count > 0 {
            header_len
        } else {
            block
                .len()
                .checked_sub(compressed_size as usize)
                .ok_or_else(|| {
                    Error::Malformed(format!(
                        "KV3 declares {compressed_size} compressed bytes in a {} byte block",
                        block.len()
                    ))
                })?
        };
        if payload_offset < header_len {
            return Err(Error::Malformed(format!(
                "KV3 payload would start at {payload_offset}, inside a {header_len}-byte header"
            )));
        }

        Ok(Header {
            dictionary_id: crate::header::u16_at(block, field::DICTIONARY_ID),
            frame_size: crate::header::u16_at(block, field::FRAME_SIZE),
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
            blob_count,
            blob_total_size: v5_only(field::BLOB_TOTAL_SIZE),
            compressed_size,
            uncompressed_size: u32_at(field::UNCOMPRESSED_SIZE),
            buffer1_uncompressed_size: v5_only(field::BUFFER1_UNCOMPRESSED),
            buffer1_compressed_size: v5_only(field::BUFFER1_COMPRESSED),
            buffer2_uncompressed_size: v5_only(field::BUFFER2_UNCOMPRESSED),
            buffer2_compressed_size: v5_only(field::BUFFER2_COMPRESSED),
            payload_offset,
            ..blank(
                version,
                format,
                Compression::from_raw(u32_at(field::COMPRESSION)),
            )
        })
    }

    fn parse_legacy(block: &[u8]) -> Result<Self> {
        if block.len() < HEADER_LEN_LEGACY {
            return Err(Error::Malformed(format!(
                "KV3 block is {} bytes, its header alone is {HEADER_LEN_LEGACY}",
                block.len()
            )));
        }
        let encoding = &block[4..20];
        let mut format = [0u8; 16];
        format.copy_from_slice(&block[20..36]);

        let compression = if encoding == ENCODING_STORED {
            Compression::None
        } else if encoding == ENCODING_LZ4 {
            Compression::Lz4
        } else if encoding == ENCODING_BLOCK {
            Compression::Block
        } else {
            return Err(Error::Unsupported(format!(
                "VKV\\x03 encoding {encoding:02x?} is not one this crate knows"
            )));
        };

        let (payload_offset, uncompressed_size) = match compression {
            Compression::None => (HEADER_LEN_LEGACY, block.len() - HEADER_LEN_LEGACY),
            _ => {
                if block.len() < LEGACY_SIZED_PAYLOAD {
                    return Err(Error::Malformed(format!(
                        "KV3 block is {} bytes, its length word ends at {LEGACY_SIZED_PAYLOAD}",
                        block.len()
                    )));
                }
                let word = u32_at(block, HEADER_LEN_LEGACY);
                let size = if compression == Compression::Block {
                    word & 0x00FF_FFFF
                } else {
                    word
                };
                (LEGACY_SIZED_PAYLOAD, size as usize)
            }
        };

        let mut header = blank(Version::Legacy, format, compression);
        header.payload_offset = payload_offset;
        header.compressed_size = u32::try_from(block.len() - payload_offset)
            .map_err(|_| Error::Malformed("KV3 payload does not fit 32 bits".into()))?;
        header.uncompressed_size = u32::try_from(uncompressed_size)
            .map_err(|_| Error::Malformed("KV3 payload does not fit 32 bits".into()))?;
        Ok(header)
    }

    fn parse_numbered(block: &[u8], version: Version) -> Result<Self> {
        let counts = if version == Version::V1 { 24 } else { 28 };
        let header_len = counts + 16;
        if block.len() < header_len {
            return Err(Error::Malformed(format!(
                "KV3 block is {} bytes, its header alone is {header_len}",
                block.len()
            )));
        }
        let mut format = [0u8; 16];
        format.copy_from_slice(&block[4..20]);

        let mut header = blank(
            version,
            format,
            Compression::from_raw(u32_at(block, field::COMPRESSION)),
        );
        if version == Version::V2 {
            header.dictionary_id = u16_at(block, field::DICTIONARY_ID);
            header.frame_size = u16_at(block, field::FRAME_SIZE);
        }
        header.binary_byte_count = u32_at(block, counts);
        header.integer_count = u32_at(block, counts + 4);
        header.eight_byte_count = u32_at(block, counts + 8);
        header.uncompressed_size = u32_at(block, counts + 12);
        header.payload_offset = header_len;
        header.compressed_size = u32::try_from(block.len() - header_len)
            .map_err(|_| Error::Malformed("KV3 payload does not fit 32 bits".into()))?;
        Ok(header)
    }
}
