//! Compression codecs: LZ4 blocks, zstd frames and Valve's block scheme.

use std::collections::HashMap;

use crate::Compression;
use crate::error::{Error, Result};

/// First four bytes of a zstd frame.
#[cfg(feature = "zstd")]
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

/// The zstd level Valve's files were written at, judged by their sizes.
#[cfg(feature = "zstd")]
const ZSTD_LEVEL: i32 = 7;

/// How far back an LZ4 match can reach.
#[cfg(feature = "lz4")]
pub(crate) const LZ4_WINDOW: usize = 64 * 1024;

/// Frame size LZ4 files declare. Advisory on read, so it is only written for fidelity.
pub(crate) const LZ4_FRAME_SIZE: u16 = 16384;

#[cfg(not(all(feature = "lz4", feature = "zstd")))]
fn no_codec(name: &str, feature: &str) -> Error {
    Error::Compression(format!(
        "this build has no {name} codec; enable the `{feature}` feature"
    ))
}

#[cfg(feature = "lz4")]
pub(crate) fn decompress_lz4_chunk(block: &[u8], want: usize, stream: &mut Vec<u8>) -> Result<()> {
    let window = &stream[stream.len().saturating_sub(LZ4_WINDOW)..];
    let chunk = lz4_flex::block::decompress_with_dict(block, want, window)
        .map_err(|e| Error::Compression(format!("lz4 decode of a blob chunk: {e}")))?;
    if chunk.len() != want {
        return Err(Error::Compression(format!(
            "KV3 blob chunk decompressed to {} bytes, expected {want}",
            chunk.len()
        )));
    }
    stream.extend_from_slice(&chunk);
    Ok(())
}

#[cfg(not(feature = "lz4"))]
pub(crate) fn decompress_lz4_chunk(
    _block: &[u8],
    _want: usize,
    _stream: &mut Vec<u8>,
) -> Result<()> {
    Err(no_codec("LZ4", "lz4"))
}

#[cfg(feature = "zstd")]
pub(crate) fn decompress_zstd(payload: &[u8], expected: usize) -> Result<Vec<u8>> {
    if payload.get(..4) != Some(&ZSTD_MAGIC) {
        return Err(Error::Compression(format!(
            "expected a zstd frame, found {:02x?}",
            payload.get(..4).unwrap_or(payload)
        )));
    }
    // `decode_all_to_vec` walks every frame in the input; a streaming read would stop at the
    // first boundary and hand back part of the payload. It fills the vector's spare capacity
    // and never grows it, so the reservation has to be the full expected size.
    let mut out = Vec::with_capacity(expected);
    ruzstd::decoding::FrameDecoder::new()
        .decode_all_to_vec(payload, &mut out)
        .map_err(|e| Error::Compression(format!("zstd decode: {e}")))?;
    Ok(out)
}

/// Decode whole zstd frames from the front of `payload` until they add up to `expected` bytes,
/// and report how many input bytes they took. Whatever follows is not touched.
///
/// For payloads that have more data after the frames they own, where nothing but the frames
/// themselves says where they end.
#[cfg(feature = "zstd")]
pub(crate) fn decompress_zstd_prefix(payload: &[u8], expected: usize) -> Result<(Vec<u8>, usize)> {
    use ruzstd::decoding::{BlockDecodingStrategy, FrameDecoder};

    let fail = |e: &dyn std::fmt::Display| Error::Compression(format!("zstd decode: {e}"));
    let mut input = payload;
    let mut out = Vec::with_capacity(expected);
    let mut decoder = FrameDecoder::new();
    while out.len() < expected {
        decoder.init(&mut input).map_err(|e| fail(&e))?;
        loop {
            decoder
                .decode_blocks(&mut input, BlockDecodingStrategy::UptoBytes(1 << 20))
                .map_err(|e| fail(&e))?;
            if let Some(chunk) = decoder.collect() {
                out.extend_from_slice(&chunk);
            }
            if decoder.is_finished() {
                break;
            }
        }
    }
    Ok((out, payload.len() - input.len()))
}

#[cfg(not(feature = "zstd"))]
pub(crate) fn decompress_zstd_prefix(
    _payload: &[u8],
    _expected: usize,
) -> Result<(Vec<u8>, usize)> {
    Err(no_codec("zstd", "zstd"))
}

#[cfg(not(feature = "zstd"))]
pub(crate) fn decompress_zstd(_payload: &[u8], _expected: usize) -> Result<Vec<u8>> {
    Err(no_codec("zstd", "zstd"))
}

/// Decode one LZ4 block against its declared output size.
///
/// A zero-length buffer is legitimate - a document with no structure to speak of - and
/// contributes nothing rather than failing.
#[cfg(feature = "lz4")]
pub(crate) fn decompress_lz4_block(input: &[u8], expected: usize, which: &str) -> Result<Vec<u8>> {
    if expected == 0 {
        return Ok(Vec::new());
    }
    // An LZ4 block cannot expand past 255 bytes per input byte, so a larger claim is forged and
    // must not size the allocation.
    if expected / 255 > input.len() {
        return Err(Error::Compression(format!(
            "lz4 {which} of {} bytes cannot yield {expected}",
            input.len()
        )));
    }
    lz4_flex::block::decompress(input, expected)
        .map_err(|e| Error::Compression(format!("lz4 decode of {which}: {e}")))
}

#[cfg(not(feature = "lz4"))]
pub(crate) fn decompress_lz4_block(
    _input: &[u8],
    _expected: usize,
    _which: &str,
) -> Result<Vec<u8>> {
    Err(no_codec("LZ4", "lz4"))
}

/// Undo Valve's block scheme.
///
/// The stream is a series of 16-bit masks, each followed by sixteen items taken low bit first.
/// A clear bit is one literal byte. A set bit is a 16-bit copy: the top twelve bits are the
/// distance back minus one and the low four are the length minus three. A copy may overlap its
/// own output, which is how runs are written.
pub(crate) fn decompress_block(payload: &[u8], want: usize) -> Result<Vec<u8>> {
    // A copy yields at most 18 bytes from 2, so the output cannot outgrow the input by more
    // than that; this keeps a forged length from sizing the allocation.
    if want / 9 > payload.len() {
        return Err(Error::Compression(format!(
            "KV3 block stream of {} bytes cannot yield {want}",
            payload.len()
        )));
    }
    let truncated = || Error::Compression("KV3 block stream ends early".into());
    let mut out = Vec::with_capacity(want);
    let mut at = 0;
    while out.len() < want {
        let mask = payload.get(at..at + 2).ok_or_else(truncated)?;
        let mask = u16::from_le_bytes([mask[0], mask[1]]);
        at += 2;
        for bit in 0..16 {
            if out.len() == want {
                break;
            }
            if mask & (1 << bit) == 0 {
                out.push(*payload.get(at).ok_or_else(truncated)?);
                at += 1;
            } else {
                let item = payload.get(at..at + 2).ok_or_else(truncated)?;
                at += 2;
                let item = u16::from_le_bytes([item[0], item[1]]);
                let distance = usize::from(item >> 4) + 1;
                let length = usize::from(item & 0xF) + 3;
                if distance > out.len() {
                    return Err(Error::Compression(format!(
                        "KV3 block copy reaches {distance} bytes back with {} written",
                        out.len()
                    )));
                }
                for _ in 0..length {
                    out.push(out[out.len() - distance]);
                }
            }
        }
    }
    if out.len() != want {
        return Err(Error::Compression(format!(
            "KV3 block stream produced {} bytes, header says {want}",
            out.len()
        )));
    }
    Ok(out)
}

/// The farthest back a block-scheme copy reaches.
const BLOCK_WINDOW: usize = 4096;
const BLOCK_MIN_MATCH: usize = 3;
const BLOCK_MAX_MATCH: usize = 18;

/// Compress with Valve's block scheme.
///
/// A greedy matcher: a table maps each 3-byte key to the latest position that started an item,
/// and the candidate it names is taken when it matches at least three bytes. Any stream the
/// decoder accepts is valid; this one decodes to the same bytes as Valve's but does not
/// reproduce Valve's choice of matches, so compressed bytes differ from a Valve-written file
/// while the decompressed stream is identical.
pub(crate) fn compress_block(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() / 2 + 8);
    let mut latest: HashMap<[u8; 3], usize> = HashMap::new();
    let mut mask_at = 0;
    let mut mask = 0u16;
    let mut items = 0;
    let mut pos = 0;
    while pos < raw.len() {
        if items == 0 {
            mask_at = out.len();
            out.extend_from_slice(&[0, 0]);
            mask = 0;
        }
        let mut copy = None;
        if let Some(key) = raw.get(pos..pos + 3) {
            let key = [key[0], key[1], key[2]];
            if let Some(&from) = latest.get(&key)
                && pos - from <= BLOCK_WINDOW
            {
                let max = BLOCK_MAX_MATCH.min(raw.len() - pos);
                let len = (0..max)
                    .take_while(|&i| raw[pos + i] == raw[from + i])
                    .count();
                if len >= BLOCK_MIN_MATCH {
                    copy = Some((pos - from, len));
                }
            }
            latest.insert(key, pos);
        }
        match copy {
            Some((distance, len)) => {
                mask |= 1 << items;
                let item = (u16::try_from(distance - 1).expect("window is 4096") << 4)
                    | u16::try_from(len - BLOCK_MIN_MATCH).expect("length is at most 18");
                out.extend_from_slice(&item.to_le_bytes());
                pos += len;
            }
            None => {
                out.push(raw[pos]);
                pos += 1;
            }
        }
        items += 1;
        if items == 16 || pos == raw.len() {
            out[mask_at..mask_at + 2].copy_from_slice(&mask.to_le_bytes());
            items = 0;
        }
    }
    out
}

pub(crate) fn compress(raw: &[u8], compression: Compression) -> Result<Vec<u8>> {
    match compression {
        Compression::None => Ok(raw.to_vec()),
        Compression::Lz4 => compress_lz4(raw),
        Compression::Zstd => compress_zstd(raw),
        Compression::Block => Ok(compress_block(raw)),
        Compression::Unknown(v) => Err(Error::Unsupported(format!(
            "cannot write KV3 compression method {v}"
        ))),
    }
}

/// Valve's LZ4 buffers and blob chunks are all the reference encoder's level 12: every buffer
/// of every LZ4 file checked, in every revision that has them, re-encodes to the same bytes.
#[cfg(feature = "lz4")]
pub(crate) fn compress_lz4(raw: &[u8]) -> Result<Vec<u8>> {
    Ok(crate::lz4_hc::compress(&[], raw, crate::lz4_hc::Level::L12))
}

/// One blob chunk, free to match against the last 64 KiB of the chunks before it, which is the
/// window the reader decodes it against.
#[cfg(feature = "lz4")]
pub(crate) fn compress_lz4_chunk(chunk: &[u8], stream: &[u8]) -> Result<Vec<u8>> {
    let window = &stream[stream.len().saturating_sub(LZ4_WINDOW)..];
    Ok(crate::lz4_hc::compress(
        window,
        chunk,
        crate::lz4_hc::Level::L12,
    ))
}

#[cfg(not(feature = "lz4"))]
pub(crate) fn compress_lz4_chunk(_chunk: &[u8], _stream: &[u8]) -> Result<Vec<u8>> {
    Err(no_codec("LZ4", "lz4"))
}

#[cfg(not(feature = "lz4"))]
pub(crate) fn compress_lz4(_raw: &[u8]) -> Result<Vec<u8>> {
    Err(no_codec("LZ4", "lz4"))
}

#[cfg(feature = "zstd")]
fn compress_zstd(raw: &[u8]) -> Result<Vec<u8>> {
    let config = zstd_rs::CompressionConfig {
        level: ZSTD_LEVEL,
        checksum: true,
        content_size: true,
        ..zstd_rs::CompressionConfig::DEFAULT
    };
    let mut frame = Vec::new();
    zstd_rs::Compressor::new(config)
        .and_then(|mut c| c.compress(raw, None, &mut frame))
        .map_err(|e| Error::Compression(format!("zstd encode: {e}")))?;
    Ok(frame)
}

#[cfg(not(feature = "zstd"))]
fn compress_zstd(_raw: &[u8]) -> Result<Vec<u8>> {
    Err(no_codec("zstd", "zstd"))
}
