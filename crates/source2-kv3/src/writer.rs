//! The binary KV3 writer: a value tree to a block the reader - and the game - can decode.
//!
//! The mirror of [`value`](crate::value). Encoding walks the tree once, in the order the
//! reader will walk the type stream, and appends each operand to the pool its type reads
//! from. The pools and the type stream are then laid out as the header describes them;
//! see the reader's module docs for why the two revisions differ.
//!
//! # Choices the format leaves open
//!
//! The reader accepts many encodings of the same value, so the writer picks one:
//!
//! - Integers take the narrowest of byte, 4-byte and 8-byte storage, with `0` and `1`
//!   stored as type codes alone. The 2-byte pool is never written: v4 states no count
//!   for it, and nothing Deadlock ships uses it.
//! - Doubles are never narrowed to floats, so every `f64` round-trips bit for bit. `0.0`
//!   and `1.0` are type codes alone.
//! - Arrays are always the general form, one type code per element. The typed and
//!   byte-length forms are a size optimisation only.
//! - Strings are pooled by first use and shared between member names and values. The
//!   empty string is the index `-1`, and takes no pool space.
//!
//! # Compression
//!
//! zstd is written with `ruzstd`'s fastest level, one frame per buffer on v5. LZ4 is
//! written as a single literal run per buffer: a valid block that every LZ4 decoder
//! accepts, but one that does not shrink the data. `lz4_flex`'s encoder is behind a
//! feature this workspace does not enable.

use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::value::{MAX_DEPTH, node};
use crate::{Compression, MAGIC_V4, MAGIC_V5, Value};

/// Which binary revision to write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    /// `KV3\x04`: one buffer, object lengths drawn from the 4-byte pool.
    V4,
    /// `KV3\x05`: two buffers, with a table for object lengths.
    V5,
}

/// How to encode a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteOptions {
    /// Revision to write.
    pub version: Version,
    /// Payload compression. [`Compression::Unknown`] is refused.
    pub compression: Compression,
    /// Format GUID stored in the header, identifying the schema the payload follows.
    pub format: [u8; 16],
}

/// Frame size Deadlock's LZ4 files declare. Advisory on read, so it is only written for
/// fidelity.
const LZ4_FRAME_SIZE: u16 = 16384;

/// Marks the end of a KV3 document.
const TRAILER: u32 = 0xFFEE_DD00;

/// String index of the empty string.
const EMPTY_STRING: u32 = u32::MAX;

const HEADER_LEN_V5: usize = 120;
const HEADER_LEN_V4: usize = 72;

/// Encode a value tree as a binary KV3 block.
///
/// The result is what [`parse`](crate::parse) takes: a whole `DATA` block, header
/// included.
///
/// # Errors
///
/// If the tree holds a [`Value::Blob`] and the version is v4, which has no blob area; a
/// string containing NUL, which the string pool cannot represent;
/// nesting deeper than the reader allows; or more data than the header's 32-bit sizes
/// can describe. Also if the compression method is unknown, or its feature is not
/// enabled.
pub fn write(root: &Value, options: &WriteOptions) -> Result<Vec<u8>> {
    let raw_compression = match options.compression {
        Compression::None => 0u32,
        Compression::Lz4 => 1,
        Compression::Zstd => 2,
        Compression::Unknown(v) => {
            return Err(Error::Malformed(format!(
                "cannot write KV3 compression method {v}"
            )));
        }
    };

    let mut enc = Encoder::new(options.version);
    enc.value(root, None, 0)?;

    let mut header = match options.version {
        Version::V5 => vec![0u8; HEADER_LEN_V5],
        Version::V4 => vec![0u8; HEADER_LEN_V4],
    };
    let magic = match options.version {
        Version::V5 => MAGIC_V5,
        Version::V4 => MAGIC_V4,
    };
    put_u32(&mut header, field::MAGIC, magic);
    header[field::FORMAT..field::FORMAT + 16].copy_from_slice(&options.format);
    put_u32(&mut header, field::COMPRESSION, raw_compression);
    if options.compression == Compression::Lz4 {
        header[field::FRAME_SIZE..field::FRAME_SIZE + 2]
            .copy_from_slice(&LZ4_FRAME_SIZE.to_le_bytes());
    }

    let payload = match options.version {
        Version::V5 => enc.finish_v5(&mut header, options.compression)?,
        Version::V4 => enc.finish_v4(&mut header, options.compression)?,
    };

    header.extend_from_slice(&payload);
    Ok(header)
}

/// Byte offsets of the header fields the writer sets.
///
/// Fields the reader does not use are left zero, as they are in every shipped file this
/// was checked against.
mod field {
    pub const MAGIC: usize = 0;
    pub const FORMAT: usize = 4;
    pub const COMPRESSION: usize = 20;
    pub const FRAME_SIZE: usize = 26;
    pub const BINARY_BYTES: usize = 28;
    pub const INTEGERS: usize = 32;
    pub const EIGHT_BYTES: usize = 36;
    pub const TYPE_COUNT: usize = 40;
    pub const OBJECT_COUNT: usize = 44;
    pub const ARRAY_COUNT: usize = 46;
    pub const UNCOMPRESSED_SIZE: usize = 48;
    pub const COMPRESSED_SIZE: usize = 52;
    pub const BUFFER1_UNCOMPRESSED: usize = 72;
    pub const BUFFER1_COMPRESSED: usize = 76;
    pub const BUFFER2_UNCOMPRESSED: usize = 80;
    pub const BUFFER2_COMPRESSED: usize = 84;
    pub const B2_BYTES: usize = 88;
    pub const B2_INTEGERS: usize = 96;
    pub const B2_EIGHT_BYTES: usize = 100;
    pub const B2_OBJECTS: usize = 108;
    pub const VALUE_COUNT: usize = 104;
    pub const B2_ARRAYS: usize = 112;
    pub const B2_ARRAY_ELEMENTS: usize = 116;
    pub const BLOB_COUNT: usize = 56;
    pub const BLOB_TOTAL_SIZE: usize = 60;
}

fn put_u32(header: &mut [u8], at: usize, v: u32) {
    header[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn size(n: usize, what: &str) -> Result<u32> {
    u32::try_from(n)
        .map_err(|_| Error::Malformed(format!("KV3 {what} is {n}, too large for a 32-bit field")))
}

fn put_size(header: &mut [u8], at: usize, n: usize, what: &str) -> Result<()> {
    put_u32(header, at, size(n, what)?);
    Ok(())
}

fn pad_to(out: &mut Vec<u8>, width: usize) {
    out.resize(out.len().next_multiple_of(width), 0);
}

/// The pools and type stream a document's values are drawn into.
struct Encoder {
    version: Version,
    string_ids: HashMap<String, u32>,
    /// NUL-terminated strings, in pool order.
    string_blob: Vec<u8>,
    types: Vec<u8>,
    bytes: Vec<u8>,
    ints: Vec<u32>,
    eights: Vec<u64>,
    /// v5's table. Empty on v4, where counts go in `ints`.
    object_lengths: Vec<u32>,
    objects: usize,
    arrays: usize,
    array_elements: usize,
    /// v5 only: stored after the two buffers rather than in a pool.
    blobs: Vec<Vec<u8>>,
}

impl Encoder {
    fn new(version: Version) -> Self {
        Encoder {
            version,
            string_ids: HashMap::new(),
            string_blob: Vec::new(),
            types: Vec::new(),
            bytes: Vec::new(),
            ints: Vec::new(),
            eights: Vec::new(),
            object_lengths: Vec::new(),
            objects: 0,
            arrays: 0,
            array_elements: 0,
            blobs: Vec::new(),
        }
    }

    fn string_id(&mut self, s: &str) -> Result<u32> {
        if s.is_empty() {
            return Ok(EMPTY_STRING);
        }
        if let Some(&id) = self.string_ids.get(s) {
            return Ok(id);
        }
        if s.contains('\0') {
            return Err(Error::Malformed(
                "KV3 strings cannot contain NUL, it terminates them in the pool".into(),
            ));
        }
        let id = size(self.string_ids.len(), "string count")?;
        self.string_blob.extend_from_slice(s.as_bytes());
        self.string_blob.push(0);
        self.string_ids.insert(s.to_string(), id);
        Ok(id)
    }

    fn array_length(&mut self, n: usize) -> Result<()> {
        self.ints.push(size(n, "array length")?);
        Ok(())
    }

    fn object_length(&mut self, n: usize) -> Result<()> {
        let n = size(n, "object member count")?;
        match self.version {
            Version::V5 => self.object_lengths.push(n),
            Version::V4 => self.ints.push(n),
        }
        Ok(())
    }

    /// Append one value: its type code, its member name if it has one, then its operands
    /// and children. This is the order the reader draws from the pools.
    fn value(&mut self, value: &Value, name: Option<&str>, depth: u32) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(Error::Malformed(format!(
                "KV3 nesting deeper than {MAX_DEPTH}, which the reader would refuse"
            )));
        }
        let ty = match value {
            Value::Null => node::NULL,
            Value::Bool(true) => node::BOOLEAN_TRUE,
            Value::Bool(false) => node::BOOLEAN_FALSE,
            Value::Int(0) => node::INT64_ZERO,
            Value::Int(1) => node::INT64_ONE,
            Value::Int(v) if i8::try_from(*v).is_ok() => node::INT32_AS_BYTE,
            Value::Int(v) if i32::try_from(*v).is_ok() => node::INT32,
            Value::Int(_) => node::INT64,
            Value::UInt(v) if u32::try_from(*v).is_ok() => node::UINT32,
            Value::UInt(_) => node::UINT64,
            Value::Double(v) if v.to_bits() == 0 => node::DOUBLE_ZERO,
            Value::Double(v) if v.to_bits() == 1.0f64.to_bits() => node::DOUBLE_ONE,
            Value::Double(_) => node::DOUBLE,
            Value::String(_) => node::STRING,
            Value::Array(_) => node::ARRAY,
            Value::Object(_) => node::OBJECT,
            Value::Blob(bytes) => {
                if self.version == Version::V4 {
                    return Err(Error::Malformed(
                        "KV3 v4 has no blob area, so a blob value needs v5".into(),
                    ));
                }
                self.blobs.push(bytes.clone());
                node::BINARY_BLOB
            }
        };
        self.types.push(ty);
        if let Some(name) = name {
            let id = self.string_id(name)?;
            self.ints.push(id);
        }

        match value {
            Value::Int(v) => match ty {
                node::INT32_AS_BYTE => self.bytes.push(*v as i8 as u8),
                node::INT32 => self.ints.push(*v as i32 as u32),
                node::INT64 => self.eights.push(*v as u64),
                _ => {}
            },
            Value::UInt(v) => match ty {
                node::UINT32 => self.ints.push(*v as u32),
                _ => self.eights.push(*v),
            },
            Value::Double(v) if ty == node::DOUBLE => self.eights.push(v.to_bits()),
            Value::String(s) => {
                let id = self.string_id(s)?;
                self.ints.push(id);
            }
            Value::Array(items) => {
                self.arrays += 1;
                self.array_elements += items.len();
                self.array_length(items.len())?;
                for item in items {
                    self.value(item, None, depth + 1)?;
                }
            }
            Value::Object(object) => {
                self.objects += 1;
                self.object_length(object.len())?;
                for (key, member) in object.iter() {
                    self.value(member, Some(key), depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// The counts both revisions state: how many objects and arrays the document holds.
    ///
    /// Two 16-bit fields that real files fill in and this reader ignores. They saturate
    /// rather than wrap, so a document past 65535 states a wrong figure that is at least
    /// the right order of magnitude.
    fn put_shape_counts(&self, header: &mut [u8]) {
        let clamp = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
        header[field::OBJECT_COUNT..field::OBJECT_COUNT + 2]
            .copy_from_slice(&clamp(self.objects).to_le_bytes());
        header[field::ARRAY_COUNT..field::ARRAY_COUNT + 2]
            .copy_from_slice(&clamp(self.arrays).to_le_bytes());
    }

    /// Lay out buffers 1 and 2, compress each, and fill in the v5 header.
    fn finish_v5(self, header: &mut [u8], compression: Compression) -> Result<Vec<u8>> {
        let string_count = size(self.string_ids.len(), "string count")?;

        let mut buf1 = self.string_blob.clone();
        pad_to(&mut buf1, 4);
        buf1.extend_from_slice(&string_count.to_le_bytes());

        let mut buf2 = Vec::new();
        for l in &self.object_lengths {
            buf2.extend_from_slice(&l.to_le_bytes());
        }
        buf2.extend_from_slice(&self.bytes);
        if !self.ints.is_empty() {
            pad_to(&mut buf2, 4);
        }
        for i in &self.ints {
            buf2.extend_from_slice(&i.to_le_bytes());
        }
        if !self.eights.is_empty() {
            pad_to(&mut buf2, 8);
        }
        for e in &self.eights {
            buf2.extend_from_slice(&e.to_le_bytes());
        }
        buf2.extend_from_slice(&self.types);
        for b in &self.blobs {
            buf2.extend_from_slice(&size(b.len(), "blob")?.to_le_bytes());
        }
        buf2.extend_from_slice(&TRAILER.to_le_bytes());
        let (area, chunk_sizes) = blob_area(&self.blobs, compression)?;
        for c in &chunk_sizes {
            buf2.extend_from_slice(&c.to_le_bytes());
        }

        put_size(
            header,
            field::BINARY_BYTES,
            self.string_blob.len(),
            "string blob",
        )?;
        put_u32(header, field::INTEGERS, 1);
        self.put_shape_counts(header);
        put_size(header, field::VALUE_COUNT, self.types.len(), "value count")?;
        put_size(header, field::B2_ARRAYS, self.arrays, "array count")?;
        put_size(
            header,
            field::B2_ARRAY_ELEMENTS,
            self.array_elements,
            "array elements",
        )?;
        put_size(header, field::TYPE_COUNT, self.types.len(), "type stream")?;
        put_size(header, field::B2_BYTES, self.bytes.len(), "1-byte pool")?;
        put_size(header, field::B2_INTEGERS, self.ints.len(), "4-byte pool")?;
        put_size(
            header,
            field::B2_EIGHT_BYTES,
            self.eights.len(),
            "8-byte pool",
        )?;
        put_size(
            header,
            field::B2_OBJECTS,
            self.object_lengths.len(),
            "object count",
        )?;

        let blob_total: usize = self.blobs.iter().map(Vec::len).sum();
        put_size(header, field::BLOB_COUNT, self.blobs.len(), "blob count")?;
        put_size(header, field::BLOB_TOTAL_SIZE, blob_total, "blob total")?;

        let c1 = compress(&buf1, compression)?;
        let c2 = compress(&buf2, compression)?;
        let uncompressed = size(buf1.len() + buf2.len(), "payload")?;
        // Only zstd counts the blob frames here; LZ4 files stop at the buffers.
        let counted_area = if compression == Compression::Zstd {
            area.len()
        } else {
            0
        };
        let compressed = size(c1.len() + c2.len() + counted_area, "compressed payload")?;
        put_u32(header, field::UNCOMPRESSED_SIZE, uncompressed);
        put_u32(header, field::COMPRESSED_SIZE, compressed);
        put_size(header, field::BUFFER1_UNCOMPRESSED, buf1.len(), "buffer 1")?;
        // Shipped uncompressed files state no per-buffer compressed size; the reader does not
        // consult it for them.
        let stated = |c: &[u8]| {
            if compression == Compression::None {
                0
            } else {
                c.len()
            }
        };
        put_size(
            header,
            field::BUFFER1_COMPRESSED,
            stated(&c1),
            "compressed buffer 1",
        )?;
        put_size(header, field::BUFFER2_UNCOMPRESSED, buf2.len(), "buffer 2")?;
        put_size(
            header,
            field::BUFFER2_COMPRESSED,
            stated(&c2),
            "compressed buffer 2",
        )?;

        let mut payload = c1;
        payload.extend_from_slice(&c2);
        if !self.blobs.is_empty() {
            payload.extend_from_slice(&area);
            payload.extend_from_slice(&TRAILER.to_le_bytes());
        }
        Ok(payload)
    }

    /// Lay out the single v4 buffer, compress it, and fill in the v4 header.
    fn finish_v4(mut self, header: &mut [u8], compression: Compression) -> Result<Vec<u8>> {
        let string_count = size(self.string_ids.len(), "string count")?;
        self.ints.insert(0, string_count);

        let mut payload = self.bytes.clone();
        pad_to(&mut payload, 4);
        for i in &self.ints {
            payload.extend_from_slice(&i.to_le_bytes());
        }
        if !self.eights.is_empty() {
            pad_to(&mut payload, 8);
        }
        for e in &self.eights {
            payload.extend_from_slice(&e.to_le_bytes());
        }
        let region = self.string_blob.len() + self.types.len();
        payload.extend_from_slice(&self.string_blob);
        payload.extend_from_slice(&self.types);
        payload.extend_from_slice(&TRAILER.to_le_bytes());

        put_size(header, field::BINARY_BYTES, self.bytes.len(), "1-byte pool")?;
        put_size(header, field::INTEGERS, self.ints.len(), "4-byte pool")?;
        put_size(header, field::EIGHT_BYTES, self.eights.len(), "8-byte pool")?;
        self.put_shape_counts(header);
        put_size(header, field::TYPE_COUNT, region, "string and type region")?;

        let compressed = compress(&payload, compression)?;
        put_size(header, field::UNCOMPRESSED_SIZE, payload.len(), "payload")?;
        put_size(
            header,
            field::COMPRESSED_SIZE,
            compressed.len(),
            "compressed payload",
        )?;
        Ok(compressed)
    }
}

/// The compressed blob area, and on LZ4 the compressed length of each chunk.
///
/// LZ4 compresses each blob in chunks of the frame size, one block per chunk, which the
/// reader undoes chunk by chunk. zstd takes the blobs back to back as a single stream.
fn blob_area(blobs: &[Vec<u8>], compression: Compression) -> Result<(Vec<u8>, Vec<u16>)> {
    let mut chunk_sizes = Vec::new();
    if blobs.is_empty() {
        return Ok((Vec::new(), chunk_sizes));
    }
    let area = match compression {
        Compression::Lz4 => {
            let mut area = Vec::new();
            for chunk in blobs
                .iter()
                .flat_map(|b| b.chunks(usize::from(LZ4_FRAME_SIZE)))
            {
                let block = compress_lz4(chunk)?;
                chunk_sizes.push(u16::try_from(block.len()).map_err(|_| {
                    Error::Malformed("KV3 blob chunk does not fit a 16-bit length".into())
                })?);
                area.extend_from_slice(&block);
            }
            area
        }
        other => compress(&blobs.concat(), other)?,
    };
    Ok((area, chunk_sizes))
}

fn compress(raw: &[u8], compression: Compression) -> Result<Vec<u8>> {
    match compression {
        Compression::None => Ok(raw.to_vec()),
        Compression::Lz4 => compress_lz4(raw),
        Compression::Zstd => compress_zstd(raw),
        Compression::Unknown(v) => Err(Error::Malformed(format!(
            "cannot write KV3 compression method {v}"
        ))),
    }
}

/// One LZ4 block holding `raw` as a single literal run.
///
/// A block made only of literals is complete and valid, because the format's last
/// sequence carries literals and no match. The token's high nibble is the literal count,
/// or 15 with the remainder following as 255-valued continuation bytes.
#[cfg(feature = "lz4")]
fn compress_lz4(raw: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(raw.len() + raw.len() / 255 + 2);
    if raw.len() < 15 {
        out.push((raw.len() as u8) << 4);
    } else {
        out.push(0xF0);
        let mut rest = raw.len() - 15;
        while rest >= 255 {
            out.push(255);
            rest -= 255;
        }
        out.push(rest as u8);
    }
    out.extend_from_slice(raw);
    Ok(out)
}

#[cfg(not(feature = "lz4"))]
fn compress_lz4(_raw: &[u8]) -> Result<Vec<u8>> {
    Err(Error::Malformed(
        "this build cannot write LZ4; enable the `lz4` feature".into(),
    ))
}

#[cfg(feature = "zstd")]
fn compress_zstd(raw: &[u8]) -> Result<Vec<u8>> {
    Ok(ruzstd::encoding::compress_to_vec(
        raw,
        ruzstd::encoding::CompressionLevel::Fastest,
    ))
}

#[cfg(not(feature = "zstd"))]
fn compress_zstd(_raw: &[u8]) -> Result<Vec<u8>> {
    Err(Error::Malformed(
        "this build cannot write zstd; enable the `zstd` feature".into(),
    ))
}
