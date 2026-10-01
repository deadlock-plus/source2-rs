//! The writer for the original `VKV\x03` encoding.
//!
//! The stream has no pools: a count of strings, the strings, one value with its operands
//! inline, and `0xFFFFFFFF`. See `legacy` for the reader.

use std::collections::HashMap;

use crate::compression::{compress_block, compress_lz4};
use crate::error::{Error, Result};
use crate::header::{
    ENCODING_BLOCK, ENCODING_LZ4, ENCODING_STORED, HEADER_LEN_LEGACY, LEGACY_SIZED_PAYLOAD,
};
use crate::legacy::LEGACY_END;
use crate::node::{self, operand_width, scalar_bits};
use crate::read::MAX_DEPTH;
use crate::value::{ArrayForm, Kind};
use crate::writer::{Layout, array_code, plan_array, scalar_code, size};
use crate::{Compression, Value, Version, WriteOptions};

/// String index of the empty string.
pub(crate) const EMPTY_STRING: u32 = u32::MAX;

/// Streams shorter than this are written stored rather than packed.
const STORED_BELOW: usize = 16;

/// The largest stream the block scheme's 24-bit length word can describe.
const BLOCK_MAX_LEN: usize = 0x00FF_FFFF;

pub(crate) fn write_legacy(root: &Value, options: &WriteOptions) -> Result<Vec<u8>> {
    let mut w = Stream {
        layout: Layout {
            version: Version::Legacy,
        },
        ids: HashMap::new(),
        strings: Vec::new(),
        body: Vec::new(),
    };
    w.value(root, 0)?;

    let mut stream = size(w.ids.len(), "string count")?.to_le_bytes().to_vec();
    stream.extend_from_slice(&w.strings);
    stream.extend_from_slice(&w.body);
    stream.extend_from_slice(&LEGACY_END.to_le_bytes());

    let encoding = match options.compression {
        Compression::None => ENCODING_STORED,
        Compression::Lz4 => ENCODING_LZ4,
        _ => ENCODING_BLOCK,
    };
    let mut out = Vec::with_capacity(LEGACY_SIZED_PAYLOAD + stream.len() / 2);
    out.extend_from_slice(&Version::Legacy.magic().to_le_bytes());
    out.extend_from_slice(&encoding);
    out.extend_from_slice(&options.format);
    debug_assert_eq!(out.len(), HEADER_LEN_LEGACY);

    match options.compression {
        Compression::None => out.extend_from_slice(&stream),
        Compression::Lz4 => {
            out.extend_from_slice(&size(stream.len(), "stream")?.to_le_bytes());
            out.extend_from_slice(&compress_lz4(&stream)?);
        }
        _ => {
            if stream.len() > BLOCK_MAX_LEN {
                return Err(Error::Invalid(format!(
                    "a block-compressed KV3 stream is {} bytes, past the 24-bit length word",
                    stream.len()
                )));
            }
            // The top bit of the length word marks a stream kept as-is. The one sample that is
            // stored is a 13-byte empty document; the cut-off between that and the packed
            // streams (the shortest of which is 316 bytes) is a guess.
            let len = size(stream.len(), "stream")?;
            if stream.len() >= STORED_BELOW {
                out.extend_from_slice(&len.to_le_bytes());
                out.extend_from_slice(&compress_block(&stream));
            } else {
                out.extend_from_slice(&(len | 0x8000_0000).to_le_bytes());
                out.extend_from_slice(&stream);
            }
        }
    }
    Ok(out)
}

struct Stream {
    layout: Layout,
    ids: HashMap<String, u32>,
    strings: Vec<u8>,
    body: Vec<u8>,
}

impl Stream {
    fn string_id(&mut self, s: &str) -> Result<u32> {
        if s.is_empty() {
            return Ok(EMPTY_STRING);
        }
        if let Some(&id) = self.ids.get(s) {
            return Ok(id);
        }
        if s.contains('\0') {
            return Err(Error::Invalid(
                "KV3 strings cannot contain NUL, it terminates them in the pool".into(),
            ));
        }
        let id = size(self.ids.len(), "string count")?;
        self.strings.extend_from_slice(s.as_bytes());
        self.strings.push(0);
        self.ids.insert(s.to_string(), id);
        Ok(id)
    }

    fn u32(&mut self, v: u32) {
        self.body.extend_from_slice(&v.to_le_bytes());
    }

    fn type_code(&mut self, ty: u8, flags: u8) {
        if flags == 0 {
            self.body.push(ty);
        } else {
            self.body.push(ty | 0x80);
            self.body.push(flags);
        }
    }

    /// A value with a type code of its own.
    fn value(&mut self, v: &Value, depth: u32) -> Result<()> {
        let code = self.code(v);
        self.type_code(code, v.flags());
        self.operands(v, code, depth)
    }

    fn code(&self, v: &Value) -> u8 {
        match v.kind() {
            Kind::String(_) => node::STRING,
            Kind::Blob(_) => node::BINARY_BLOB,
            Kind::Object(_) => node::OBJECT,
            Kind::Array(items) => array_code(plan_array(v, items, self.layout)),
            _ => scalar_code(v, self.layout),
        }
    }

    /// The operands and children of `v`, stored under type code `code`.
    fn operands(&mut self, v: &Value, code: u8, depth: u32) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(Error::Invalid(format!(
                "KV3 nesting deeper than {MAX_DEPTH}, which the reader would refuse"
            )));
        }
        match v.kind() {
            Kind::String(s) => {
                let id = self.string_id(s)?;
                self.u32(id);
            }
            Kind::Blob(bytes) => {
                self.u32(size(bytes.len(), "blob")?);
                self.body.extend_from_slice(bytes);
            }
            Kind::Object(object) => {
                self.u32(size(object.len(), "object member count")?);
                for (key, member) in object.iter() {
                    let id = self.string_id(key)?;
                    self.u32(id);
                    self.value(member, depth + 1)?;
                }
            }
            Kind::Array(items) => {
                self.u32(size(items.len(), "array length")?);
                match plan_array(v, items, self.layout) {
                    ArrayForm::Typed { ty, flags } => {
                        self.type_code(ty, flags);
                        for item in items {
                            self.element(item, ty, depth + 1)?;
                        }
                    }
                    _ => {
                        for item in items {
                            self.value(item, depth + 1)?;
                        }
                    }
                }
            }
            scalar => {
                let bits = scalar_bits(scalar, code).ok_or_else(|| {
                    Error::Invalid(format!(
                        "a KV3 array typed {code} holds a value it cannot store"
                    ))
                })?;
                let width = operand_width(code).expect("scalar codes have a width");
                self.body.extend_from_slice(&bits.to_le_bytes()[..width]);
            }
        }
        Ok(())
    }

    /// An element of a typed array, which states its type once for all of them.
    fn element(&mut self, v: &Value, ty: u8, depth: u32) -> Result<()> {
        let natural = match v.kind() {
            Kind::Array(_) | Kind::String(_) | Kind::Blob(_) | Kind::Object(_) => self.code(v),
            _ => ty,
        };
        if natural != ty {
            return Err(Error::Invalid(format!(
                "a KV3 array typed {ty} holds an element typed {natural}"
            )));
        }
        self.operands(v, ty, depth)
    }
}
