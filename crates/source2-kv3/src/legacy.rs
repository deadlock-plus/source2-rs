//! The original `VKV` stream reader.
//!
//! The magic is followed by two GUIDs: the encoding, which names the compression, and the
//! format. Then the payload, in one of three encodings:
//!
//! - stored, with nothing between the format GUID and the stream;
//! - LZ4, behind a `u32` decompressed length;
//! - Valve's own block scheme, behind a `u32` whose low 24 bits are the decompressed length and
//!   whose top bit marks the stream as stored after all.
//!
//! The decompressed stream has no pools. It is a count of strings, the strings, one value with
//! its operands inline, and `0xFFFFFFFF`. Type codes are the ones v4 uses. The headers are
//! parsed by [`Header`](crate::Header); the writer for this encoding is in `write_legacy`.
//!
//! The stored encoding's GUID is recalled rather than measured: no sample file uses it.

use crate::error::{Error, Result};
use crate::node::{self, operand_width, scalar_value};
use crate::read::{MAX_DEPTH, lookup_string, read_strings};
use crate::value::{ArrayForm, Kind, Storage};
use crate::{Object, Value};

/// Ends a `VKV` stream.
pub(crate) const LEGACY_END: u32 = 0xFFFF_FFFF;

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// Parse a decompressed `VKV\x03` stream.
pub(crate) fn parse_legacy(stream: &[u8]) -> Result<Value> {
    let malformed = |m: &str| Error::Malformed(format!("KV3 stream: {m}"));
    let body = stream
        .len()
        .checked_sub(4)
        .filter(|&n| n >= 4)
        .ok_or_else(|| malformed("too short for a string count and an end marker"))?;
    if u32_at(stream, body) != LEGACY_END {
        return Err(malformed("does not end with the end marker"));
    }
    let count = u32_at(stream, 0) as usize;
    let (strings, rest) = read_strings(&stream[4..body], count)?;
    let mut reader = Inline { strings, rest };
    let (ty, flags) = reader.read_type()?;
    let root = reader.read_value(ty, flags, 0)?;
    if !reader.rest.is_empty() {
        return Err(malformed("has bytes after the root value"));
    }
    Ok(root)
}

struct Inline<'a> {
    strings: Vec<&'a str>,
    rest: &'a [u8],
}

impl Inline<'_> {
    fn take(&mut self, len: usize) -> Result<&[u8]> {
        if self.rest.len() < len {
            return Err(Error::Malformed(format!(
                "KV3 stream wants {len} bytes, {} left",
                self.rest.len()
            )));
        }
        let (a, b) = self.rest.split_at(len);
        self.rest = b;
        Ok(a)
    }

    fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_type(&mut self) -> Result<(u8, u8)> {
        let first = self.take(1)?[0];
        if first & 0x80 == 0 {
            return Ok((first, 0));
        }
        let flags = self.take(1)?[0];
        Ok((first & 0x3F, flags))
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
                let id = self.u32()? as i32;
                (
                    Kind::String(lookup_string(&self.strings, id)?),
                    Storage::Inferred,
                )
            }
            node::BINARY_BLOB => {
                let len = self.u32()? as usize;
                (Kind::Blob(self.take(len)?.to_vec()), Storage::Inferred)
            }
            node::ARRAY => {
                let n = self.u32()? as usize;
                let mut items = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    let (ty, flags) = self.read_type()?;
                    items.push(self.read_value(ty, flags, next)?);
                }
                (Kind::Array(items), Storage::Array(ArrayForm::General))
            }
            node::ARRAY_TYPED => {
                let n = self.u32()? as usize;
                let (sub, sub_flags) = self.read_type()?;
                let mut items = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    items.push(self.read_value(sub, sub_flags, next)?);
                }
                (
                    Kind::Array(items),
                    Storage::Array(ArrayForm::Typed {
                        ty: sub,
                        flags: sub_flags,
                    }),
                )
            }
            node::OBJECT => {
                let n = self.u32()? as usize;
                let mut object = Object::new();
                for _ in 0..n {
                    let id = self.u32()? as i32;
                    let name = lookup_string(&self.strings, id)?;
                    let (ty, flags) = self.read_type()?;
                    object.push(name, self.read_value(ty, flags, next)?);
                }
                (Kind::Object(object), Storage::Inferred)
            }
            other => {
                let width = operand_width(other).ok_or_else(|| {
                    Error::Malformed(format!(
                        "KV3 type code {other} is not one this reader knows"
                    ))
                })?;
                let mut bits = [0u8; 8];
                bits[..width].copy_from_slice(self.take(width)?);
                return Ok(scalar_value(other, u64::from_le_bytes(bits), flags));
            }
        };
        Ok(Value::stored(kind, flags, storage))
    }
}
