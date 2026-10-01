//! From a binary block to its decompressed payload.
//!
//! # v5 splits the payload in two, v4 does not
//!
//! In v5, buffer 1 holds the pools - strings, and the 1/2/4/8-byte operands - and buffer 2
//! holds the structure that gives them meaning. Each is compressed on its own, and the header
//! sizes them separately: offsets 72/76 are buffer 1's uncompressed and compressed lengths,
//! 80/84 are buffer 2's, and 48/52 are the totals.
//!
//! Reading a total where a buffer size belongs, or the reverse, is the mistake this layout
//! invites: a decode validated against offset 72 accepts a payload that stopped after the
//! string table, which looks plausible rather than obviously broken.
//!
//! A v4 payload is one buffer. Offsets 72 onwards are not a shorter form of the same fields,
//! they are where the payload starts, so the four per-buffer sizes read zero and 48/52 are the
//! only lengths there are.
//!
//! # Why the compression paths differ
//!
//! A zstd frame is self-delimiting, so the whole payload decodes in one call that walks both
//! frames. **A raw LZ4 block is not** - nothing in the byte stream says where one ends - so on
//! v5 the LZ4 path has to cut the payload at buffer 1's compressed length and decode each
//! buffer against its own declared output size. Every other revision's single buffer needs no
//! cut: the whole payload is one block, sized by the header's total.
//!
//! `frame_size` (16384 on every LZ4 file) is not that boundary and must not be used as one: a
//! 28,344-byte buffer 1 decodes as a single block. Whether a single-buffer payload larger than
//! `frame_size` would be split is untested, and one that did split would fail the length check
//! rather than decode partially.
//!
//! # Locating the payload
//!
//! The header is a fixed 120 bytes in v5 and 72 in v4, but rather than trusting either constant
//! this derives the payload offset as `block_length - compressed_size`, then checks the result
//! lands past the header. A file whose header grows by a field still reads correctly, and a
//! file that is not what we think it is fails loudly instead of decompressing garbage.

use crate::compression::{
    decompress_block, decompress_lz4_block, decompress_lz4_chunk, decompress_zstd,
};
use crate::error::{Error, Result};
use crate::header::{HEADER_LEN_LEGACY, u32_at};
use crate::{Compression, Header, Version};

/// A decoded KV3 block: its header, and the payload with compression undone.
#[derive(Clone, Debug)]
pub struct Decoded {
    /// The header this came from.
    pub header: Header,
    /// The decompressed payload: buffer 1 followed by buffer 2.
    pub payload: Vec<u8>,
    /// The decompressed binary blobs, in the order the type stream asks for them.
    pub blobs: Vec<Vec<u8>>,
}

/// Read a binary KV3 block and undo its compression.
///
/// # Errors
///
/// [`Error::Malformed`] if the header will not parse or the result is not the length the
/// header promised; [`Error::Unsupported`] for a compression method this crate does not know;
/// [`Error::Compression`] if a codec fails or is not enabled.
pub fn decode(block: &[u8]) -> Result<Decoded> {
    let header = Header::parse(block)?;
    let mut payload = &block[header.payload_offset..];
    let mut blob_area: &[u8] = &[];
    if header.blob_count > 0 {
        let main_len = match header.compression {
            Compression::None => header.uncompressed_size as usize,
            _ => (header.buffer1_compressed_size as usize)
                .saturating_add(header.buffer2_compressed_size as usize),
        };
        if main_len > payload.len() {
            return Err(Error::Malformed(format!(
                "KV3 buffers claim {main_len} bytes, payload has {}",
                payload.len()
            )));
        }
        (payload, blob_area) = payload.split_at(main_len);
    }

    let out = match header.compression {
        Compression::None => payload.to_vec(),
        Compression::Zstd => decompress_zstd(payload, header.uncompressed_size as usize)?,
        Compression::Lz4 => decompress_lz4(payload, &header)?,
        Compression::Block => {
            let stored = u32_at(block, HEADER_LEN_LEGACY) & 0x8000_0000 != 0;
            if stored {
                payload.to_vec()
            } else {
                decompress_block(payload, header.uncompressed_size as usize)?
            }
        }
        Compression::Unknown(v) => {
            return Err(Error::Unsupported(format!(
                "KV3 compression method {v} is not one this crate knows"
            )));
        }
    };

    // Checked against the total, not the first frame's figure: getting this wrong is exactly
    // how a partial decode passes for a complete one.
    if out.len() != header.uncompressed_size as usize {
        return Err(Error::Malformed(format!(
            "KV3 decompressed to {} bytes, header says {} across all frames",
            out.len(),
            header.uncompressed_size
        )));
    }

    let blobs = read_blobs(&header, &out, blob_area)?;
    Ok(Decoded {
        header,
        payload: out,
        blobs,
    })
}

/// Marks the end of a buffer, and again the end of a file that has blobs.
pub(crate) const TRAILER_BYTES: [u8; 4] = 0xFFEE_DD00u32.to_le_bytes();

/// Undo the compression of the blob area that follows the two buffers.
///
/// Buffer 2 ends with each blob's decompressed length as a u32, then the trailer, then on LZ4
/// files a u16 for each compressed chunk. zstd stores the blobs back to back in one stream.
/// Both end the file with a second trailer.
///
/// LZ4 cuts each blob into chunks of at most `frame_size` bytes, compresses them in turn, and
/// lets a chunk copy from the ones before it, blobs included. So there can be more chunks than
/// blobs, and every chunk has to be decoded against the output so far.
fn read_blobs(header: &Header, payload: &[u8], area: &[u8]) -> Result<Vec<Vec<u8>>> {
    let n = header.blob_count as usize;
    if n == 0 {
        return Ok(Vec::new());
    }
    let short = || Error::Malformed("KV3 buffer 2 is too short for its blob tables".into());
    let total = header.blob_total_size as usize;

    let buffer2 = payload
        .get(header.buffer1_uncompressed_size as usize..)
        .ok_or_else(short)?;
    let chunk_len = if header.frame_size == 0 {
        usize::from(u16::MAX) + 1
    } else {
        usize::from(header.frame_size)
    };

    // The chunk table's length depends on the sizes before the trailer, and the trailer is
    // found by counting back over the table, so try each plausible length.
    let (sizes, trailer_at) = if header.compression == Compression::Lz4 {
        let most = n.saturating_add(total / chunk_len);
        (n..=most)
            .find_map(|k| {
                let trailer_at = buffer2
                    .len()
                    .checked_sub(k.checked_mul(2)?.checked_add(4)?)?;
                let sizes = blob_sizes(buffer2, trailer_at, n, total)?;
                let chunks: usize = sizes.iter().map(|s| s.div_ceil(chunk_len)).sum();
                (chunks == k).then_some((sizes, trailer_at))
            })
            .ok_or_else(short)?
    } else {
        let trailer_at = buffer2.len().checked_sub(4).ok_or_else(short)?;
        let sizes = blob_sizes(buffer2, trailer_at, n, total).ok_or_else(short)?;
        (sizes, trailer_at)
    };

    let area = area.strip_suffix(&TRAILER_BYTES).unwrap_or(area);
    let all = match header.compression {
        Compression::Lz4 => {
            let mut stream = Vec::with_capacity(total.min(1 << 24));
            let mut rest = area;
            let mut chunk = 0;
            for &size in &sizes {
                let mut left = size;
                while left > 0 {
                    let at = trailer_at + 4 + chunk * 2;
                    let clen = usize::from(u16::from_le_bytes([buffer2[at], buffer2[at + 1]]));
                    let block = take_blob_bytes(&mut rest, clen)?;
                    let want = chunk_len.min(left);
                    decompress_lz4_chunk(block, want, &mut stream)?;
                    left -= want;
                    chunk += 1;
                }
            }
            stream
        }
        Compression::Zstd => decompress_zstd(area, total)?,
        _ => area.to_vec(),
    };
    if all.len() != total {
        return Err(Error::Malformed(format!(
            "KV3 blobs decompressed to {} bytes, expected {total}",
            all.len()
        )));
    }

    let mut rest = all.as_slice();
    let mut blobs = Vec::with_capacity(n.min(4096));
    for &size in &sizes {
        blobs.push(take_blob_bytes(&mut rest, size)?.to_vec());
    }
    Ok(blobs)
}

/// The blob lengths that end at `trailer_at`, if the trailer is there and they add up to
/// `total`.
fn blob_sizes(buffer2: &[u8], trailer_at: usize, n: usize, total: usize) -> Option<Vec<usize>> {
    if buffer2.get(trailer_at..trailer_at + 4)? != TRAILER_BYTES {
        return None;
    }
    let raw = &buffer2[trailer_at.checked_sub(n.checked_mul(4)?)?..trailer_at];
    let sizes: Vec<usize> = raw
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c) as usize)
        .collect();
    (sizes.iter().try_fold(0usize, |a, &s| a.checked_add(s))? == total).then_some(sizes)
}

fn take_blob_bytes<'a>(rest: &mut &'a [u8], len: usize) -> Result<&'a [u8]> {
    if rest.len() < len {
        return Err(Error::Malformed(format!(
            "KV3 blob wants {len} bytes, {} left",
            rest.len()
        )));
    }
    let (a, b) = rest.split_at(len);
    *rest = b;
    Ok(a)
}

/// Undo LZ4 compression, one buffer at a time.
///
/// Raw LZ4 blocks carry no magic and no length, so unlike the zstd path this cannot hand the
/// whole payload to the decoder and let it find the boundary: on v5 it has to cut at buffer 1's
/// compressed length and decode each side against its own declared output size. Both sizes are
/// load-bearing rather than hints, and a wrong one fails here instead of yielding a short
/// result.
///
/// Every other revision has no such cut to make. Its payload is a single buffer, so the block is
/// the whole payload and the size to decode against is the header's total.
fn decompress_lz4(payload: &[u8], header: &Header) -> Result<Vec<u8>> {
    if header.version != Version::V5 {
        return decompress_lz4_block(payload, header.uncompressed_size as usize, "the payload");
    }
    let split = header.buffer1_compressed_size as usize;
    let end = split.saturating_add(header.buffer2_compressed_size as usize);
    if end > payload.len() {
        return Err(Error::Malformed(format!(
            "KV3 buffers claim {end} compressed bytes, payload has {}",
            payload.len()
        )));
    }
    let one = decompress_lz4_block(
        &payload[..split],
        header.buffer1_uncompressed_size as usize,
        "buffer 1",
    )?;
    let two = decompress_lz4_block(
        &payload[split..end],
        header.buffer2_uncompressed_size as usize,
        "buffer 2",
    )?;
    let mut out = Vec::with_capacity(one.len() + two.len());
    out.extend_from_slice(&one);
    out.extend_from_slice(&two);
    Ok(out)
}
