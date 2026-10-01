//! The binary value reader: decompressed payload bytes to a value tree.
//!
//! Ported from [`ValveResourceFormat`]'s `BinaryKV3` reader, which is MIT licensed and is the
//! reference implementation for this format. Every structural figure below was also checked
//! against sample files; where the two disagreed the files won.
//!
//! # Why the data is split across two buffers
//!
//! A v5 document is not a byte stream to be walked front to back. It is a set of *pools* plus
//! a *type stream*, and decoding means walking the types while drawing operands from whichever
//! pool each type calls for:
//!
//! - **Buffer 1** holds the string blob and, for v5, acts as the *auxiliary* buffer.
//! - **Buffer 2** holds the object lengths, the 1/2/4/8-byte operand pools, and the type stream
//!   itself, ending in the `0xFFEEDD00` trailer.
//!
//! Splitting values by width is what makes the format compress as well as it does: all the
//! 4-byte operands sit together, all the strings sit together.
//!
//! One consequence worth knowing: an `ARRAY_TYPE_AUXILIARY_BUFFER` swaps the two buffers for
//! the duration of its elements, so the same type code reads from a different pool depending on
//! where it appears.
//!
//! # What v3 and v4 do instead
//!
//! They have one buffer, and its regions run in a different order: the 1/2/4/8-byte pools
//! first, then a single region holding the string blob followed by the type stream, then the
//! trailer. Three differences follow, and each is a way to read a v4 document as if it were v5
//! and get plausible nonsense rather than an error:
//!
//! - The header field at offset 40 sizes **strings and types together** in v4, where in v5 it
//!   sizes the type stream alone.
//! - There is no object-length table. A member count is drawn from the ordinary 4-byte pool,
//!   in stream order with every other operand, so a reader looking for a table takes the next
//!   member's name index for a length.
//! - There is no second buffer, so `ARRAY_TYPE_AUXILIARY_BUFFER` has nothing to swap to.
//!
//! What is the same: the type codes, the flag-byte convention, and the string count being the
//! first word of the 4-byte pool rather than a header field.
//!
//! v1 and v2 lay the payload out like v4 without the 2-byte pool and with the strings walked
//! forward; the original `VKV\x03` encoding has no pools at all and is read by `legacy`.
//!
//! [`ValveResourceFormat`]: https://github.com/ValveResourceFormat/ValveResourceFormat

use crate::error::{Error, Result};
use crate::node;
use crate::value::{ArrayForm, Kind, Storage};
use crate::{Decoded, Header, Object, Value, Version};

/// How deeply values may nest before parsing is abandoned.
///
/// Real documents nest a handful deep. This exists so a malformed or hostile file cannot drive
/// the recursive reader into a stack overflow, which is not something a `Result` could report.
pub(crate) const MAX_DEPTH: u32 = 128;

/// The depth limit has to stay inside a thread stack to be worth anything.
///
/// A Rust thread gets 2 MiB by default and this reader's frames are not small, so a few hundred
/// levels is far inside that and a hundred thousand is not. Checked at compile time: both sides
/// are constants, so a limit that defeats its own purpose is a build error rather than
/// something a test run might not reach.
const _: () = assert!(MAX_DEPTH <= 1024);

/// Marks the end of a KV3 document.
pub(crate) const TRAILER: u32 = 0xFFEE_DD00;

/// Parse the root value of a decoded block.
pub(crate) fn parse_root(decoded: &Decoded) -> Result<Value> {
    let header = &decoded.header;
    match header.version {
        Version::Legacy => crate::legacy::parse_legacy(&decoded.payload),
        Version::V1 | Version::V2 => parse_flat(&decoded.payload, header),
        Version::V3 | Version::V4 => parse_v4(&decoded.payload, header),
        Version::V5 => parse_v5(&decoded.payload, header, &decoded.blobs),
    }
}

/// One pool set: the 1, 2, 4 and 8-byte operand regions of a buffer.
#[derive(Default)]
struct Pools<'a> {
    b1: &'a [u8],
    b2: &'a [u8],
    b4: &'a [u8],
    b8: &'a [u8],
}

impl<'a> Pools<'a> {
    fn u8(&mut self) -> Result<u8> {
        let (a, rest) = self.b1.split_first().ok_or_else(|| short("1-byte"))?;
        self.b1 = rest;
        Ok(*a)
    }

    fn bytes(&mut self, len: usize) -> Result<&'a [u8]> {
        if self.b1.len() < len {
            return Err(short("1-byte"));
        }
        let (a, rest) = self.b1.split_at(len);
        self.b1 = rest;
        Ok(a)
    }

    fn u16(&mut self) -> Result<u16> {
        if self.b2.len() < 2 {
            return Err(short("2-byte"));
        }
        let (a, rest) = self.b2.split_at(2);
        self.b2 = rest;
        Ok(u16::from_le_bytes([a[0], a[1]]))
    }

    fn u32(&mut self) -> Result<u32> {
        if self.b4.len() < 4 {
            return Err(short("4-byte"));
        }
        let (a, rest) = self.b4.split_at(4);
        self.b4 = rest;
        Ok(u32::from_le_bytes([a[0], a[1], a[2], a[3]]))
    }

    fn u64(&mut self) -> Result<u64> {
        if self.b8.len() < 8 {
            return Err(short("8-byte"));
        }
        let (a, rest) = self.b8.split_at(8);
        self.b8 = rest;
        Ok(u64::from_le_bytes(a.try_into().expect("split_at gave 8")))
    }
}

fn short(pool: &str) -> Error {
    Error::Malformed(format!("KV3 {pool} pool ran out mid-value"))
}

/// Where an object's member count comes from.
///
/// v5 gives objects a table of their own at the head of buffer 2. The other pooled revisions
/// have no such table and draw the count from the ordinary 4-byte pool, interleaved with every
/// other operand. Reading the wrong one does not fail loudly - it takes the next member's name
/// index for a length - so this is a variant rather than an empty slice standing in.
#[derive(Clone, Copy)]
enum ObjectLengths<'a> {
    /// v5's dedicated table, consumed four bytes at a time.
    Table(&'a [u8]),
    /// Everything else, where the count is just another 4-byte operand.
    Pool,
}

/// Everything the recursive reader draws from.
struct Reader<'a> {
    strings: Vec<&'a str>,
    blobs: &'a [Vec<u8>],
    next_blob: usize,
    /// Whether blobs are a length in the 4-byte pool followed by that many bytes of the 1-byte
    /// pool, rather than separate areas after the buffers.
    inline_blobs: bool,
    types: &'a [u8],
    object_lengths: ObjectLengths<'a>,
    /// The pool set type codes read from right now.
    buffer: Pools<'a>,
    /// The other pool set, swapped in by `ARRAY_TYPE_AUXILIARY_BUFFER`.
    ///
    /// Empty outside v5, which has no second buffer for that type code to reach.
    auxiliary: Pools<'a>,
}

fn align(offset: usize, to: usize) -> usize {
    offset + (to - offset % to) % to
}

/// Alignment before a pool, which applies only when the pool has entries.
///
/// An empty pool is not padded for. A buffer 2 with no 8-byte entries whose regions account for
/// exactly its own length, leaving the four bytes the trailer needs, is the shape that settles
/// it: padding unconditionally overruns it.
fn align_for(offset: usize, to: usize, entries: u32) -> usize {
    if entries == 0 {
        offset
    } else {
        align(offset, to)
    }
}

/// Split `len` bytes off the front of `rest`, reporting the region name on failure.
fn take<'a>(rest: &mut &'a [u8], len: usize, what: &str) -> Result<&'a [u8]> {
    if rest.len() < len {
        return Err(Error::Malformed(format!(
            "KV3 {what} wants {len} bytes, {} left",
            rest.len()
        )));
    }
    let (a, b) = rest.split_at(len);
    *rest = b;
    Ok(a)
}

fn parse_v5(payload: &[u8], h: &Header, blobs: &[Vec<u8>]) -> Result<Value> {
    let b1_len = h.buffer1_uncompressed_size as usize;
    if payload.len() < b1_len {
        return Err(Error::Malformed(format!(
            "KV3 payload is {} bytes, buffer 1 alone claims {b1_len}",
            payload.len()
        )));
    }
    let (buf1, buf2) = payload.split_at(b1_len);

    // Buffer 1: the string blob, plus the auxiliary pools.
    let mut o = 0;
    let strings_blob =
        &buf1[o..o + slice_len(buf1, o, h.binary_byte_count as usize, "buffer1 1-byte")?];
    o += h.binary_byte_count as usize;
    o = align_for(o, 2, h.two_byte_count);
    o += h.two_byte_count as usize * 2;
    o = align_for(o, 4, h.integer_count);
    let b4_start = o;
    o += h.integer_count as usize * 4;
    o = align_for(o, 8, h.eight_byte_count);
    let b8_start = o;

    // The 4-byte pool opens with the number of strings; the pool proper starts after it.
    if h.integer_count == 0 {
        return Err(Error::Malformed(
            "KV3 buffer 1 has no 4-byte pool, so it cannot hold the string count".into(),
        ));
    }
    let string_count = u32::from_le_bytes(
        buf1.get(b4_start..b4_start + 4)
            .ok_or_else(|| short("4-byte"))?
            .try_into()
            .map_err(|_| short("4-byte"))?,
    ) as usize;

    let (strings, blob) = read_strings(strings_blob, string_count)?;

    let aux = Pools {
        b1: blob,
        b2: &[],
        b4: buf1
            .get(b4_start + 4..b8_start.min(buf1.len()))
            .ok_or_else(|| short("4-byte"))?,
        b8: buf1.get(b8_start..).unwrap_or(&[]),
    };

    // Buffer 2: object lengths, the operand pools, then the type stream.
    let mut rest = buf2;
    let object_lengths = take(
        &mut rest,
        h.object_count_buffer2 as usize * 4,
        "object lengths",
    )?;
    let consumed = |rest: &[u8]| buf2.len() - rest.len();

    let b1 = take(&mut rest, h.byte_count_buffer2 as usize, "buffer2 1-byte")?;
    let pad = align_for(consumed(rest), 2, h.two_byte_count_buffer2) - consumed(rest);
    take(&mut rest, pad, "buffer2 2-byte padding")?;
    let b2 = take(
        &mut rest,
        h.two_byte_count_buffer2 as usize * 2,
        "buffer2 2-byte",
    )?;
    let pad = align_for(consumed(rest), 4, h.integer_count_buffer2) - consumed(rest);
    take(&mut rest, pad, "buffer2 4-byte padding")?;
    let b4 = take(
        &mut rest,
        h.integer_count_buffer2 as usize * 4,
        "buffer2 4-byte",
    )?;
    let pad = align_for(consumed(rest), 8, h.eight_byte_count_buffer2) - consumed(rest);
    take(&mut rest, pad, "buffer2 8-byte padding")?;
    let b8 = take(
        &mut rest,
        h.eight_byte_count_buffer2 as usize * 8,
        "buffer2 8-byte",
    )?;
    let types = take(&mut rest, h.type_count as usize, "type stream")?;
    take(&mut rest, blobs.len() * 4, "blob lengths")?;

    check_trailer(take(&mut rest, 4, "trailer")?)?;

    let mut reader = Reader {
        blobs,
        next_blob: 0,
        inline_blobs: false,
        strings,
        types,
        object_lengths: ObjectLengths::Table(object_lengths),
        buffer: Pools { b1, b2, b4, b8 },
        auxiliary: aux,
    };

    let (ty, flags) = reader.read_type()?;
    reader.read_value(ty, flags, 0)
}

/// A v3 or v4 document: the pools first, then the strings, the type stream and the trailer.
///
/// Where the pools end is not something the header states. It states the size of the
/// *string-and-type* region instead, and the trailer sits immediately after that region, so
/// measuring back from the end of the payload pins both exactly. Valve's files align the string
/// blob to 8 bytes even when the 8-byte pool is empty; measuring backwards accepts that and
/// files that do not.
fn parse_v4(payload: &[u8], h: &Header) -> Result<Value> {
    let blob_len = h.type_count as usize;
    let blob_start = blob_len
        .checked_add(4)
        .and_then(|tail| payload.len().checked_sub(tail))
        .ok_or_else(|| {
            Error::Malformed(format!(
                "KV3 v4 strings and types claim {blob_len} bytes plus a trailer, payload has {}",
                payload.len()
            ))
        })?;
    let eight_start = (h.eight_byte_count as usize)
        .checked_mul(8)
        .and_then(|len| blob_start.checked_sub(len))
        .ok_or_else(|| {
            Error::Malformed(format!(
                "KV3 v4 8-byte pool of {} entries does not fit before the string blob",
                h.eight_byte_count
            ))
        })?;

    // The pools run 1, 2, 4 then 8 bytes wide, exactly as v5's buffer 1 does. The 2-byte count
    // comes from offset 64, which reads zero on every v4 file seen, so that pool's place in the
    // order is taken from v5 rather than measured.
    let byte_len = h.binary_byte_count as usize;
    let two_start = align_for(byte_len, 2, h.two_byte_count);
    let two_len = (h.two_byte_count as usize).saturating_mul(2);
    let four_start = align_for(two_start.saturating_add(two_len), 4, h.integer_count);
    let four_end = four_start.saturating_add((h.integer_count as usize).saturating_mul(4));
    if four_end > eight_start {
        return Err(Error::Malformed(format!(
            "KV3 v4 pools run to {four_end}, past the 8-byte pool that starts at {eight_start}"
        )));
    }

    // The 4-byte pool opens with the number of strings; the pool proper starts after it.
    if h.integer_count == 0 {
        return Err(Error::Malformed(
            "KV3 v4 has no 4-byte pool, so it cannot hold the string count".into(),
        ));
    }
    let string_count = u32::from_le_bytes(
        payload[four_start..four_start + 4]
            .try_into()
            .map_err(|_| short("4-byte"))?,
    ) as usize;

    // Strings and types share one region in that order, so whatever the strings leave is the
    // type stream. Its length is never stated on its own.
    let blob = &payload[blob_start..blob_start + blob_len];
    let (strings, types) = read_strings(blob, string_count)?;
    check_trailer(&payload[blob_start + blob_len..])?;

    let mut reader = Reader {
        blobs: &[],
        next_blob: 0,
        inline_blobs: false,
        strings,
        types,
        object_lengths: ObjectLengths::Pool,
        buffer: Pools {
            b1: &payload[..byte_len],
            b2: &payload[two_start..two_start + two_len],
            b4: &payload[four_start + 4..four_end],
            b8: &payload[eight_start..blob_start],
        },
        auxiliary: Pools::default(),
    };

    let (ty, flags) = reader.read_type()?;
    reader.read_value(ty, flags, 0)
}

/// A v1 or v2 document: pools, then strings, the type stream and the trailer.
///
/// No field gives the length of the string region, so the strings are walked forward from the
/// end of the 8-byte pool, and the trailer is the last word of the payload. The strings start
/// on an 8-byte boundary whether or not the 8-byte pool has entries.
fn parse_flat(payload: &[u8], h: &Header) -> Result<Value> {
    let byte_len = h.binary_byte_count as usize;
    let four_start = align(byte_len, 4);
    let four_end = (h.integer_count as usize)
        .checked_mul(4)
        .and_then(|len| four_start.checked_add(len))
        .filter(|&end| end <= payload.len())
        .ok_or_else(|| {
            Error::Malformed(format!(
                "KV3 4-byte pool of {} entries does not fit a payload of {} bytes",
                h.integer_count,
                payload.len()
            ))
        })?;
    if h.integer_count == 0 {
        return Err(Error::Malformed(
            "KV3 has no 4-byte pool, so it cannot hold the string count".into(),
        ));
    }
    let eight_start = align(four_end, 8);
    let eight_end = (h.eight_byte_count as usize)
        .checked_mul(8)
        .and_then(|len| eight_start.checked_add(len))
        .filter(|&end| end <= payload.len())
        .ok_or_else(|| {
            Error::Malformed(format!(
                "KV3 8-byte pool of {} entries does not fit a payload of {} bytes",
                h.eight_byte_count,
                payload.len()
            ))
        })?;
    let string_count = u32::from_le_bytes(
        payload[four_start..four_start + 4]
            .try_into()
            .map_err(|_| short("4-byte"))?,
    ) as usize;

    let (strings, rest) = read_strings(&payload[eight_end..], string_count)?;
    let types_len = rest
        .len()
        .checked_sub(4)
        .ok_or_else(|| Error::Malformed("KV3 payload has no room for a trailer".into()))?;
    check_trailer(&rest[types_len..])?;

    let mut reader = Reader {
        blobs: &[],
        next_blob: 0,
        inline_blobs: true,
        strings,
        types: &rest[..types_len],
        object_lengths: ObjectLengths::Pool,
        buffer: Pools {
            b1: &payload[..byte_len],
            b2: &[],
            b4: &payload[four_start + 4..four_end],
            b8: &payload[eight_start..eight_end],
        },
        auxiliary: Pools::default(),
    };
    let (ty, flags) = reader.read_type()?;
    reader.read_value(ty, flags, 0)
}

/// Read `count` NUL-terminated strings off the front of `blob`, and whatever follows them.
///
/// The count is a figure out of the file, so it is bounded before it sizes an allocation: every
/// string costs at least its terminator, which makes more strings than bytes impossible.
pub(crate) fn read_strings(blob: &[u8], count: usize) -> Result<(Vec<&str>, &[u8])> {
    if count > blob.len() {
        return Err(Error::Malformed(format!(
            "KV3 claims {count} strings in a {}-byte region",
            blob.len()
        )));
    }
    let mut rest = blob;
    let mut strings = Vec::with_capacity(count);
    for i in 0..count {
        let end = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| Error::Malformed(format!("KV3 string {i} is unterminated")))?;
        strings.push(
            std::str::from_utf8(&rest[..end])
                .map_err(|e| Error::Malformed(format!("KV3 string {i} is not UTF-8: {e}")))?,
        );
        rest = &rest[end + 1..];
    }
    Ok((strings, rest))
}

/// The string at `id`, where `-1` is the empty string.
pub(crate) fn lookup_string(strings: &[&str], id: i32) -> Result<String> {
    if id == -1 {
        return Ok(String::new());
    }
    let i = usize::try_from(id)
        .ok()
        .filter(|&i| i < strings.len())
        .ok_or_else(|| {
            Error::Malformed(format!(
                "KV3 string index {id} out of range, {} strings",
                strings.len()
            ))
        })?;
    Ok(strings[i].to_string())
}

/// Check the word that marks the end of a document.
fn check_trailer(bytes: &[u8]) -> Result<()> {
    let trailer = bytes
        .get(..4)
        .map(|b| u32::from_le_bytes(b.try_into().expect("get gave 4")))
        .ok_or_else(|| {
            Error::Malformed(format!("KV3 trailer wants 4 bytes, {} left", bytes.len()))
        })?;
    if trailer != TRAILER {
        return Err(Error::Malformed(format!(
            "KV3 trailer is {trailer:#010x}, expected {TRAILER:#010x}"
        )));
    }
    Ok(())
}

fn slice_len(buf: &[u8], at: usize, want: usize, what: &str) -> Result<usize> {
    if at + want > buf.len() {
        return Err(Error::Malformed(format!(
            "KV3 {what} region wants {want} bytes at {at}, buffer is {}",
            buf.len()
        )));
    }
    Ok(want)
}

impl<'a> Reader<'a> {
    /// Read one type code, and its flag byte when the high bit marks one.
    fn read_type(&mut self) -> Result<(u8, u8)> {
        let (&first, rest) = self
            .types
            .split_first()
            .ok_or_else(|| Error::Malformed("KV3 type stream ran out".into()))?;
        self.types = rest;

        if first & 0x80 == 0 {
            return Ok((first, 0));
        }
        // The flag lives in the next byte, and only the low six bits are the type.
        let (&flag, rest) = self
            .types
            .split_first()
            .ok_or_else(|| Error::Malformed("KV3 type flag byte is missing".into()))?;
        self.types = rest;
        Ok((first & 0x3F, flag))
    }

    fn next_object_length(&mut self) -> Result<usize> {
        match self.object_lengths {
            ObjectLengths::Pool => Ok(self.buffer.u32()? as usize),
            ObjectLengths::Table(table) => {
                if table.len() < 4 {
                    return Err(Error::Malformed("KV3 object-length table ran out".into()));
                }
                let (a, rest) = table.split_at(4);
                self.object_lengths = ObjectLengths::Table(rest);
                Ok(u32::from_le_bytes([a[0], a[1], a[2], a[3]]) as usize)
            }
        }
    }

    fn string(&mut self, id: i32) -> Result<String> {
        lookup_string(&self.strings, id)
    }

    /// Read one member: a type, a name drawn from the string pool, a value.
    fn read_member(&mut self, depth: u32) -> Result<(String, Value)> {
        let (ty, flags) = self.read_type()?;
        let id = self.buffer.u32()? as i32;
        let name = self.string(id)?;
        let value = self.read_value(ty, flags, depth)?;
        Ok((name, value))
    }

    fn scalar(&mut self, ty: u8, flags: u8) -> Result<Value> {
        let bits = match ty {
            node::NULL
            | node::BOOLEAN_TRUE
            | node::BOOLEAN_FALSE
            | node::INT64_ZERO
            | node::INT64_ONE
            | node::DOUBLE_ZERO
            | node::DOUBLE_ONE => 0,
            node::BOOLEAN | node::INT32_AS_BYTE => u64::from(self.buffer.u8()?),
            node::INT16 | node::UINT16 => u64::from(self.buffer.u16()?),
            node::INT32 | node::UINT32 | node::FLOAT => u64::from(self.buffer.u32()?),
            node::INT64 | node::UINT64 | node::DOUBLE => self.buffer.u64()?,
            other => {
                return Err(Error::Malformed(format!(
                    "KV3 type code {other} is not one this reader knows"
                )));
            }
        };
        Ok(node::scalar_value(ty, bits, flags))
    }

    fn read_value(&mut self, ty: u8, flags: u8, depth: u32) -> Result<Value> {
        if depth > MAX_DEPTH {
            return Err(Error::Malformed(format!(
                "KV3 nesting deeper than {MAX_DEPTH}"
            )));
        }
        let next = depth + 1;
        let (kind, storage) = match ty {
            node::STRING => {
                let id = self.buffer.u32()? as i32;
                (Kind::String(self.string(id)?), Storage::Inferred)
            }
            node::ARRAY => {
                let n = self.buffer.u32()? as usize;
                let mut items = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    let (ty, flags) = self.read_type()?;
                    items.push(self.read_value(ty, flags, next)?);
                }
                (Kind::Array(items), Storage::Array(ArrayForm::General))
            }
            node::ARRAY_TYPED | node::ARRAY_TYPE_BYTE_LENGTH => {
                let n = if ty == node::ARRAY_TYPE_BYTE_LENGTH {
                    usize::from(self.buffer.u8()?)
                } else {
                    self.buffer.u32()? as usize
                };
                // One type code for the whole array, read once.
                let (sub, sub_flags) = self.read_type()?;
                let mut items = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    items.push(self.read_value(sub, sub_flags, next)?);
                }
                let form = if ty == node::ARRAY_TYPE_BYTE_LENGTH {
                    ArrayForm::ByteLength {
                        ty: sub,
                        flags: sub_flags,
                    }
                } else {
                    ArrayForm::Typed {
                        ty: sub,
                        flags: sub_flags,
                    }
                };
                (Kind::Array(items), Storage::Array(form))
            }
            node::ARRAY_TYPE_AUXILIARY_BUFFER => {
                let n = usize::from(self.buffer.u8()?);
                let (sub, sub_flags) = self.read_type()?;
                // The elements are drawn from the other pool set. Swapping and swapping back is
                // what the reference does, and it keeps one code path per type.
                std::mem::swap(&mut self.buffer, &mut self.auxiliary);
                let mut items = Vec::with_capacity(n.min(4096));
                let mut err = None;
                for _ in 0..n {
                    match self.read_value(sub, sub_flags, next) {
                        Ok(v) => items.push(v),
                        Err(e) => {
                            err = Some(e);
                            break;
                        }
                    }
                }
                // Swap back even on failure, so the error surfaces instead of a panic or a
                // silently corrupted reader state.
                std::mem::swap(&mut self.buffer, &mut self.auxiliary);
                if let Some(e) = err {
                    return Err(e);
                }
                (
                    Kind::Array(items),
                    Storage::Array(ArrayForm::Auxiliary {
                        ty: sub,
                        flags: sub_flags,
                    }),
                )
            }
            node::OBJECT => {
                let n = self.next_object_length()?;
                let mut object = Object::new();
                for _ in 0..n {
                    let (key, member) = self.read_member(next)?;
                    object.push(key, member);
                }
                (Kind::Object(object), Storage::Inferred)
            }
            node::BINARY_BLOB if self.inline_blobs => {
                let len = self.buffer.u32()? as usize;
                (
                    Kind::Blob(self.buffer.bytes(len)?.to_vec()),
                    Storage::Inferred,
                )
            }
            node::BINARY_BLOB => {
                let blob = self.blobs.get(self.next_blob).ok_or_else(|| {
                    Error::Malformed(
                        "KV3 type stream asks for more blobs than the file holds".into(),
                    )
                })?;
                self.next_blob += 1;
                (Kind::Blob(blob.clone()), Storage::Inferred)
            }
            other => return self.scalar(other, flags),
        };
        Ok(Value::stored(kind, flags, storage))
    }
}

#[cfg(test)]
mod depth_limit_tests {
    use super::MAX_DEPTH;

    /// The recursion limit stays inside a thread's stack.
    ///
    /// Nothing else checks the limit itself: raising it from `128` to `100000` would pass the
    /// rest of the suite, because every real file nests far below either number and no test
    /// feeds a deep one. The failure would be a process abort rather than a parse error.
    #[test]
    fn the_depth_limit_stays_inside_a_thread_stack() {
        assert_eq!(MAX_DEPTH, 128);
    }
}
