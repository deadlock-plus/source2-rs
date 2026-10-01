//! The binary writer: a value tree to a block the reader can decode.
//!
//! The mirror of [`read`](crate::read). Encoding walks the tree once, in the order the reader
//! will walk the type stream, and appends each operand to the pool its type reads from. The
//! pools and the type stream are then laid out as the header describes them; see the reader's
//! module docs for why the revisions differ.
//!
//! # Choices the format leaves open
//!
//! The reader accepts many encodings of the same value. A value read from a file remembers the
//! one the file used - flag bytes, integer and float widths, array forms - and the writer
//! repeats it, so a document read with [`parse`](crate::parse) and written back is the same
//! payload, byte for byte. Where a value carries no such memory, or can no longer hold it, or
//! the target revision cannot store it, the writer picks the way Valve's shipped files usually
//! do:
//!
//! - Integers take the 4-byte type when they fit and the 8-byte one when not, with `0` and `1`
//!   as type codes alone. Doubles are never narrowed, and `0.0` and `1.0` are type codes alone.
//! - An array whose elements are all numbers of one type, strings, objects or arrays with one
//!   type code and flag byte is stored as a typed array: element type once, then the elements,
//!   which do not use the `0` and `1` shorthands. Anything else, including an empty array,
//!   keeps a type code per element.
//! - A typed array of up to 255 elements takes a 1-byte length, where the revision has one. On
//!   v5, one of 4-byte integers, unsigned integers or doubles keeps its elements in the
//!   auxiliary buffer. Longer arrays take a 4-byte length.
//! - The 2-byte pool is written only when a tree has 2-byte scalars, and only where the
//!   revision has one; v1 and v2 widen them to 4 bytes.
//! - Strings are pooled by first use and shared between member names and values. The empty
//!   string is the index `-1`, and takes no pool space.
//!
//! # Header counts
//!
//! v5 states three counts the reader ignores. They are written as Valve's files state them:
//! offset 104 is the number of values that carry a type code of their own (typed array
//! elements do not), 112 the number of arrays and 116 their element count with an empty array
//! counting for one. Auxiliary-buffer arrays are left out of both unless they hold 32 elements
//! or more.
//!
//! # Compression
//!
//! Both follow what Valve's files use. LZ4 is the reference high-compression encoder at level
//! 12, from a built-in port, one block per buffer or chunk; the buffers of every LZ4 file
//! checked re-encode to Valve's exact bytes. zstd is level 7 with the content checksum and
//! size, one frame per buffer on v5; its output is about the size of Valve's but not the same
//! bytes.
//!
//! # What is verified
//!
//! v4 and v5 are checked against shipped files, v1 and the original `VKV\x03` encoding against
//! a sample set. v2 and v3 have no known sample: they are written from the layout the reader
//! infers for them (v2 as v1 with the dictionary and frame fields in the header, v3 as v4).

use std::collections::HashMap;

use crate::compression::{LZ4_FRAME_SIZE, compress, compress_lz4_chunk};
use crate::error::{Error, Result};
use crate::guid::GENERIC_FORMAT;
use crate::header::{HEADER_LEN_V1, HEADER_LEN_V2, HEADER_LEN_V4, HEADER_LEN_V5};
use crate::node::{self, default_scalar_type, scalar_bits};
use crate::read::MAX_DEPTH;
use crate::value::{ArrayForm, Kind, Storage};
use crate::{Compression, Header, Value, Version};

/// How to encode a document.
///
/// The defaults are Valve's usual choice: v5, the generic format, LZ4 (or the first codec the
/// build has when `lz4` is off). Set only what differs:
///
/// ```
/// use source2_kv3::{Compression, Version, WriteOptions};
///
/// let options = WriteOptions {
///     version: Version::V4,
///     compression: Compression::None,
///     ..WriteOptions::default()
/// };
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteOptions {
    /// Revision to write. All of [`Version`] can be written; `V2` and `V3` follow the layout the
    /// reader infers, with no sample to check them against.
    pub version: Version,
    /// Payload compression. [`Version::Legacy`] takes [`None`](Compression::None),
    /// [`Lz4`](Compression::Lz4) or [`Block`](Compression::Block); every other revision takes
    /// `None`, `Lz4` or [`Zstd`](Compression::Zstd). [`Compression::Unknown`] is refused.
    pub compression: Compression,
    /// Format GUID stored in the header, identifying the schema the payload follows.
    pub format: [u8; 16],
    /// Dictionary id stored in the header of revisions that have the field (v2 to v5). 0 in
    /// every file seen.
    pub dictionary_id: u16,
    /// Frame size stored in the header of revisions that have the field (v2 to v5). With LZ4 a
    /// value of 0 means 16384, the figure every LZ4 file seen declares, and it is also the
    /// chunk length blobs are compressed in; with another compression the value is written as
    /// given.
    pub frame_size: u16,
}

impl Default for WriteOptions {
    fn default() -> Self {
        WriteOptions {
            version: Version::V5,
            compression: default_compression(),
            format: GENERIC_FORMAT,
            dictionary_id: 0,
            frame_size: 0,
        }
    }
}

fn default_compression() -> Compression {
    if cfg!(feature = "lz4") {
        Compression::Lz4
    } else if cfg!(feature = "zstd") {
        Compression::Zstd
    } else {
        Compression::None
    }
}

impl TryFrom<&Header> for WriteOptions {
    type Error = Error;

    /// The options that reproduce a block's own layout: its revision, compression, format GUID,
    /// dictionary id and frame size. Nothing is substituted.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if the header names a compression method this crate does not
    /// know, which no writer could repeat.
    fn try_from(header: &Header) -> Result<Self> {
        if let Compression::Unknown(v) = header.compression {
            return Err(Error::Unsupported(format!(
                "cannot write KV3 compression method {v}"
            )));
        }
        Ok(WriteOptions {
            version: header.version,
            compression: header.compression,
            format: header.format,
            dictionary_id: header.dictionary_id,
            frame_size: header.frame_size,
        })
    }
}

impl WriteOptions {
    /// The frame size LZ4 output uses, and so the chunk length of LZ4-compressed blobs.
    fn lz4_chunk(&self) -> usize {
        usize::from(if self.frame_size == 0 {
            LZ4_FRAME_SIZE
        } else {
            self.frame_size
        })
    }

    fn check(&self) -> Result<()> {
        let ok = match (self.version, self.compression) {
            (_, Compression::Unknown(v)) => {
                return Err(Error::Unsupported(format!(
                    "cannot write KV3 compression method {v}"
                )));
            }
            (Version::Legacy, Compression::None | Compression::Lz4 | Compression::Block) => true,
            (Version::Legacy, _) => false,
            (_, Compression::Block) => false,
            _ => true,
        };
        if ok {
            Ok(())
        } else {
            Err(Error::Unsupported(format!(
                "no known KV3 file pairs {:?} with {:?} compression",
                self.version, self.compression
            )))
        }
    }
}

/// Encode a value tree as a binary KV3 block.
///
/// The result is what [`parse`](crate::parse) takes: a whole block, header included. A tree
/// read with [`parse`](crate::parse) and written with the options it came with has the same
/// decompressed payload as the block it came from. Values built by hand are stored the way
/// Valve's files show; see the module docs.
///
/// Prefer [`Document::to_bytes`](crate::Document::to_bytes), which carries the options along.
///
/// # Errors
///
/// [`Error::Invalid`] if the tree holds a string containing NUL, which the string pool cannot represent, nesting deeper than the
/// reader allows, or more data than the header's 32-bit sizes can describe;
/// [`Error::Unsupported`] for an unknown compression method or one the revision does not pair
/// with; [`Error::Compression`] if the codec is not enabled.
pub fn write(root: &Value, options: &WriteOptions) -> Result<Vec<u8>> {
    options.check()?;
    match options.version {
        Version::Legacy => crate::write_legacy::write_legacy(root, options),
        version => write_pooled(root, options, version),
    }
}

fn write_pooled(root: &Value, options: &WriteOptions, version: Version) -> Result<Vec<u8>> {
    let mut enc = Encoder::new(version);
    enc.value(root, None, 0, Slot::Free)?;

    let header_len = match version {
        Version::V5 => HEADER_LEN_V5,
        Version::V3 | Version::V4 => HEADER_LEN_V4,
        Version::V2 => HEADER_LEN_V2,
        _ => HEADER_LEN_V1,
    };
    let mut header = vec![0u8; header_len];
    put_u32(&mut header, field::MAGIC, version.magic());
    header[field::FORMAT..field::FORMAT + 16].copy_from_slice(&options.format);
    put_u32(&mut header, field::COMPRESSION, raw_compression(options)?);
    let frame = if options.compression == Compression::Lz4 {
        u16::try_from(options.lz4_chunk()).expect("frame size is a u16")
    } else {
        options.frame_size
    };
    if version != Version::V1 {
        header[field::DICTIONARY_ID..field::DICTIONARY_ID + 2]
            .copy_from_slice(&options.dictionary_id.to_le_bytes());
        header[field::FRAME_SIZE..field::FRAME_SIZE + 2].copy_from_slice(&frame.to_le_bytes());
    }

    let payload = match version {
        Version::V5 => enc.finish_v5(&mut header, options)?,
        Version::V3 | Version::V4 => enc.finish_v4(&mut header, options)?,
        _ => enc.finish_flat(&mut header, options.compression)?,
    };

    header.extend_from_slice(&payload);
    Ok(header)
}

fn raw_compression(options: &WriteOptions) -> Result<u32> {
    Ok(match options.compression {
        Compression::None => 0,
        Compression::Lz4 => 1,
        Compression::Zstd => 2,
        other => {
            return Err(Error::Unsupported(format!(
                "cannot write KV3 compression {other:?} in a numbered revision"
            )));
        }
    })
}

/// Byte offsets of the header fields the writer sets.
///
/// Fields the reader does not use are left zero, as they are in every shipped file this was
/// checked against.
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
    pub const OBJECT_COUNT: usize = 44;
    pub const ARRAY_COUNT: usize = 46;
    pub const UNCOMPRESSED_SIZE: usize = 48;
    pub const COMPRESSED_SIZE: usize = 52;
    pub const BLOB_COUNT: usize = 56;
    pub const BLOB_TOTAL_SIZE: usize = 60;
    pub const TWO_BYTES: usize = 64;
    pub const BLOB_CHUNK_TABLE: usize = 68;
    pub const BUFFER1_UNCOMPRESSED: usize = 72;
    pub const BUFFER1_COMPRESSED: usize = 76;
    pub const BUFFER2_UNCOMPRESSED: usize = 80;
    pub const BUFFER2_COMPRESSED: usize = 84;
    pub const B2_BYTES: usize = 88;
    pub const B2_TWO_BYTES: usize = 92;
    pub const B2_INTEGERS: usize = 96;
    pub const B2_EIGHT_BYTES: usize = 100;
    pub const VALUE_COUNT: usize = 104;
    pub const B2_OBJECTS: usize = 108;
    pub const B2_ARRAYS: usize = 112;
    pub const B2_ARRAY_ELEMENTS: usize = 116;
}

/// An auxiliary-buffer array this long or longer is counted among the arrays in the header.
const COUNTED_AUXILIARY_LEN: usize = 32;

pub(crate) fn put_u32(header: &mut [u8], at: usize, v: u32) {
    header[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

pub(crate) fn size(n: usize, what: &str) -> Result<u32> {
    u32::try_from(n)
        .map_err(|_| Error::Invalid(format!("KV3 {what} is {n}, too large for a 32-bit field")))
}

fn put_size(header: &mut [u8], at: usize, n: usize, what: &str) -> Result<()> {
    put_u32(header, at, size(n, what)?);
    Ok(())
}

fn pad_to(out: &mut Vec<u8>, width: usize) {
    out.resize(out.len().next_multiple_of(width), 0);
}

/// What a revision's layout can store.
#[derive(Clone, Copy)]
pub(crate) struct Layout {
    pub version: Version,
}

impl Layout {
    /// Whether arrays may state their length in one byte.
    pub fn byte_length_arrays(self) -> bool {
        !matches!(self.version, Version::Legacy | Version::V1 | Version::V2)
    }

    /// Whether arrays may keep their elements' operands in the auxiliary buffer.
    pub fn auxiliary_arrays(self) -> bool {
        self.version == Version::V5
    }

    /// Whether a scalar stored under type code `ty` has somewhere to live.
    pub fn can_store_scalar(self, ty: u8) -> bool {
        match ty {
            node::INT16 | node::UINT16 => {
                self.version.has_two_byte_pool() || self.version == Version::Legacy
            }
            _ => true,
        }
    }

    /// Whether blobs live in an area after the buffers, rather than inline in the pools.
    pub fn blob_area(self) -> bool {
        matches!(self.version, Version::V3 | Version::V4 | Version::V5)
    }
}

/// The 1, 2, 4 and 8-byte operand pools one buffer holds.
#[derive(Default)]
struct Pool {
    bytes: Vec<u8>,
    twos: Vec<u16>,
    ints: Vec<u32>,
    eights: Vec<u64>,
}

impl Pool {
    /// Append the 2, 4 and 8-byte pools, each aligned to its width when it has entries.
    /// `first_int` is a word the 4-byte pool opens with.
    fn append_wide(&self, out: &mut Vec<u8>, first_int: Option<u32>) {
        if !self.twos.is_empty() {
            pad_to(out, 2);
        }
        for t in &self.twos {
            out.extend_from_slice(&t.to_le_bytes());
        }
        if !self.ints.is_empty() || first_int.is_some() {
            pad_to(out, 4);
        }
        if let Some(first) = first_int {
            out.extend_from_slice(&first.to_le_bytes());
        }
        for i in &self.ints {
            out.extend_from_slice(&i.to_le_bytes());
        }
        if !self.eights.is_empty() {
            pad_to(out, 8);
        }
        for e in &self.eights {
            out.extend_from_slice(&e.to_le_bytes());
        }
    }
}

/// Whether a value opens with a type code of its own, or takes the one its array states.
#[derive(Clone, Copy)]
enum Slot {
    Free,
    Element(u8),
}

/// The longest array a 1-byte length can describe.
pub(crate) const BYTE_LENGTH_MAX: usize = 255;

/// Element types a typed array is given when nothing says how to store it.
fn can_be_typed(ty: u8) -> bool {
    matches!(
        ty,
        node::INT64
            | node::UINT64
            | node::DOUBLE
            | node::STRING
            | node::INT32
            | node::UINT32
            | node::OBJECT
            | node::ARRAY
            | node::ARRAY_TYPED
            | node::ARRAY_TYPE_BYTE_LENGTH
            | node::ARRAY_TYPE_AUXILIARY_BUFFER
    )
}

pub(crate) fn array_code(form: ArrayForm) -> u8 {
    match form {
        ArrayForm::General => node::ARRAY,
        ArrayForm::Typed { .. } => node::ARRAY_TYPED,
        ArrayForm::ByteLength { .. } => node::ARRAY_TYPE_BYTE_LENGTH,
        ArrayForm::Auxiliary { .. } => node::ARRAY_TYPE_AUXILIARY_BUFFER,
    }
}

/// The layout an array is written in: the one it was read with while that still holds its
/// elements, otherwise the default.
pub(crate) fn plan_array(array: &Value, items: &[Value], layout: Layout) -> ArrayForm {
    // Planned once per element and shared by both branches below, so the work does not double
    // at every level of nesting.
    let nested: Vec<Option<u8>> = items
        .iter()
        .map(|item| match item.kind() {
            Kind::Array(inner) => Some(array_code(plan_array(item, inner, layout))),
            _ => None,
        })
        .collect();
    match array.storage() {
        Storage::Array(form) if form_fits(form, items, &nested, layout) => form,
        _ => default_form(items, &nested, layout),
    }
}

fn form_fits(form: ArrayForm, items: &[Value], nested: &[Option<u8>], layout: Layout) -> bool {
    let (ty, flags) = match form {
        ArrayForm::General => return true,
        ArrayForm::Typed { ty, flags } => (ty, flags),
        ArrayForm::ByteLength { ty, flags } => {
            if items.len() > BYTE_LENGTH_MAX || !layout.byte_length_arrays() {
                return false;
            }
            (ty, flags)
        }
        ArrayForm::Auxiliary { ty, flags } => {
            if items.len() > BYTE_LENGTH_MAX || !layout.auxiliary_arrays() {
                return false;
            }
            (ty, flags)
        }
    };
    items
        .iter()
        .zip(nested)
        .all(|(item, &array)| item.flags() == flags && fits_element(item, array, ty, layout))
}

/// Whether `item` can be stored as an element of a typed array of type `ty`. `array` is the
/// code of the item's own array layout, if it is an array.
fn fits_element(item: &Value, array: Option<u8>, ty: u8, layout: Layout) -> bool {
    match item.kind() {
        Kind::String(_) => ty == node::STRING,
        Kind::Blob(_) => ty == node::BINARY_BLOB,
        Kind::Object(_) => ty == node::OBJECT,
        Kind::Array(_) => array == Some(ty),
        scalar => layout.can_store_scalar(ty) && scalar_bits(scalar, ty).is_some(),
    }
}

/// The type code `item` gets as a member of a new typed array.
fn default_element_code(item: &Value, array: Option<u8>) -> u8 {
    match item.kind() {
        Kind::String(_) => node::STRING,
        Kind::Blob(_) => node::BINARY_BLOB,
        Kind::Object(_) => node::OBJECT,
        Kind::Array(_) => array.expect("arrays have a layout"),
        scalar => default_scalar_type(scalar, false).expect("scalar kinds have a type"),
    }
}

fn default_form(items: &[Value], nested: &[Option<u8>], layout: Layout) -> ArrayForm {
    let (Some(first), Some(&first_array)) = (items.first(), nested.first()) else {
        return ArrayForm::General;
    };
    let ty = default_element_code(first, first_array);
    let flags = first.flags();
    let uniform = can_be_typed(ty)
        && items
            .iter()
            .zip(nested)
            .all(|(item, &array)| item.flags() == flags && default_element_code(item, array) == ty);
    if !uniform {
        ArrayForm::General
    } else if items.len() > BYTE_LENGTH_MAX || !layout.byte_length_arrays() {
        ArrayForm::Typed { ty, flags }
    } else if layout.auxiliary_arrays() && matches!(ty, node::DOUBLE | node::INT32 | node::UINT32) {
        ArrayForm::Auxiliary { ty, flags }
    } else {
        ArrayForm::ByteLength { ty, flags }
    }
}

/// The type code a scalar value is stored under: the one it was read with while that still
/// holds it, otherwise Valve's usual.
pub(crate) fn scalar_code(v: &Value, layout: Layout) -> u8 {
    match v.storage() {
        Storage::Scalar(ty)
            if layout.can_store_scalar(ty) && scalar_bits(v.kind(), ty).is_some() =>
        {
            ty
        }
        _ => default_scalar_type(v.kind(), true).expect("scalar kinds have a type"),
    }
}

/// The pools and type stream a document's values are drawn into.
struct Encoder {
    version: Version,
    layout: Layout,
    string_ids: HashMap<String, u32>,
    /// NUL-terminated strings, in pool order.
    string_blob: Vec<u8>,
    types: Vec<u8>,
    /// The pools type codes draw from now, and the other set `Auxiliary` arrays swap to. Only
    /// the first is used outside v5.
    pools: [Pool; 2],
    current: usize,
    /// v5's table. Empty elsewhere, where counts go in the 4-byte pool.
    object_lengths: Vec<u32>,
    objects: usize,
    arrays: usize,
    /// Values that carry a type code of their own, which excludes the elements of typed arrays.
    values: usize,
    counted_arrays: usize,
    counted_elements: usize,
    /// v5 only: stored after the two buffers rather than in a pool.
    blobs: Vec<Vec<u8>>,
}

impl Encoder {
    fn new(version: Version) -> Self {
        Encoder {
            version,
            layout: Layout { version },
            string_ids: HashMap::new(),
            string_blob: Vec::new(),
            types: Vec::new(),
            pools: [Pool::default(), Pool::default()],
            current: 0,
            object_lengths: Vec::new(),
            objects: 0,
            arrays: 0,
            values: 0,
            counted_arrays: 0,
            counted_elements: 0,
            blobs: Vec::new(),
        }
    }

    fn pool(&mut self) -> &mut Pool {
        &mut self.pools[self.current]
    }

    fn string_id(&mut self, s: &str) -> Result<u32> {
        if s.is_empty() {
            return Ok(crate::write_legacy::EMPTY_STRING);
        }
        if let Some(&id) = self.string_ids.get(s) {
            return Ok(id);
        }
        if s.contains('\0') {
            return Err(Error::Invalid(
                "KV3 strings cannot contain NUL, it terminates them in the pool".into(),
            ));
        }
        let id = size(self.string_ids.len(), "string count")?;
        self.string_blob.extend_from_slice(s.as_bytes());
        self.string_blob.push(0);
        self.string_ids.insert(s.to_string(), id);
        Ok(id)
    }

    fn object_length(&mut self, n: usize) -> Result<()> {
        let n = size(n, "object member count")?;
        match self.version {
            Version::V5 => self.object_lengths.push(n),
            _ => self.pool().ints.push(n),
        }
        Ok(())
    }

    fn type_code(&mut self, ty: u8, flags: u8) {
        if flags == 0 {
            self.types.push(ty);
        } else {
            self.types.push(ty | 0x80);
            self.types.push(flags);
        }
    }

    fn scalar(&mut self, ty: u8, bits: u64) {
        let pool = self.pool();
        match ty {
            node::BOOLEAN | node::INT32_AS_BYTE => pool.bytes.push(bits as u8),
            node::INT16 | node::UINT16 => pool.twos.push(bits as u16),
            node::INT32 | node::UINT32 | node::FLOAT => pool.ints.push(bits as u32),
            node::INT64 | node::UINT64 | node::DOUBLE => pool.eights.push(bits),
            _ => {}
        }
    }

    /// Append one value: its type code unless it is an element of a typed array, its member
    /// name if it has one, then its operands and children. This is the order the reader draws
    /// from the pools.
    fn value(&mut self, v: &Value, name: Option<&str>, depth: u32, slot: Slot) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(Error::Invalid(format!(
                "KV3 nesting deeper than {MAX_DEPTH}, which the reader would refuse"
            )));
        }
        let form = match v.kind() {
            Kind::Array(items) => Some(plan_array(v, items, self.layout)),
            _ => None,
        };
        let code = match (v.kind(), form, slot) {
            (Kind::String(_), ..) => node::STRING,
            (Kind::Blob(_), ..) => node::BINARY_BLOB,
            (Kind::Object(_), ..) => node::OBJECT,
            (Kind::Array(_), Some(form), _) => array_code(form),
            (_, _, Slot::Element(ty)) => ty,
            (_, _, Slot::Free) => scalar_code(v, self.layout),
        };
        match slot {
            Slot::Free => {
                self.type_code(code, v.flags());
                self.values += 1;
            }
            Slot::Element(ty) if ty != code => {
                return Err(Error::Invalid(format!(
                    "a KV3 array typed {ty} holds an element typed {code}"
                )));
            }
            Slot::Element(_) => {}
        }
        if let Some(name) = name {
            let id = self.string_id(name)?;
            self.pool().ints.push(id);
        }

        match (v.kind(), form) {
            (Kind::String(s), _) => {
                let id = self.string_id(s)?;
                self.pool().ints.push(id);
            }
            (Kind::Blob(bytes), _) => {
                if self.layout.blob_area() {
                    self.blobs.push(bytes.clone());
                } else {
                    let len = size(bytes.len(), "blob")?;
                    let pool = self.pool();
                    pool.ints.push(len);
                    pool.bytes.extend_from_slice(bytes);
                }
            }
            (Kind::Array(items), Some(form)) => self.array(items, form, depth)?,
            (Kind::Object(object), _) => {
                self.objects += 1;
                self.object_length(object.len())?;
                for (key, member) in object.iter() {
                    self.value(member, Some(key), depth + 1, Slot::Free)?;
                }
            }
            (scalar, _) => {
                let bits = scalar_bits(scalar, code).ok_or_else(|| {
                    Error::Invalid(format!(
                        "a KV3 array typed {code} holds a value it cannot store"
                    ))
                })?;
                self.scalar(code, bits);
            }
        }
        Ok(())
    }

    fn array(&mut self, items: &[Value], form: ArrayForm, depth: u32) -> Result<()> {
        let len = items.len();
        self.arrays += 1;
        if !matches!(form, ArrayForm::Auxiliary { .. }) || len >= COUNTED_AUXILIARY_LEN {
            self.counted_arrays += 1;
            self.counted_elements += len.max(1);
        }

        match form {
            ArrayForm::General | ArrayForm::Typed { .. } => {
                let n = size(len, "array length")?;
                self.pool().ints.push(n);
            }
            ArrayForm::ByteLength { .. } | ArrayForm::Auxiliary { .. } => {
                let n = u8::try_from(len).map_err(|_| {
                    Error::Invalid(format!(
                        "a byte-length KV3 array cannot hold {len} elements"
                    ))
                })?;
                self.pool().bytes.push(n);
            }
        }
        let (ty, flags) = match form {
            ArrayForm::General => {
                for item in items {
                    self.value(item, None, depth + 1, Slot::Free)?;
                }
                return Ok(());
            }
            ArrayForm::Typed { ty, flags }
            | ArrayForm::ByteLength { ty, flags }
            | ArrayForm::Auxiliary { ty, flags } => (ty, flags),
        };

        self.type_code(ty, flags);
        let auxiliary = matches!(form, ArrayForm::Auxiliary { .. });
        if auxiliary {
            self.current ^= 1;
        }
        let result = items
            .iter()
            .try_for_each(|item| self.value(item, None, depth + 1, Slot::Element(ty)));
        if auxiliary {
            self.current ^= 1;
        }
        result
    }

    /// The counts every pooled revision from v3 states: how many objects and arrays the
    /// document holds.
    ///
    /// Two 16-bit fields that real files fill in and this reader ignores. They saturate rather
    /// than wrap, so a document past 65535 states a wrong figure that is at least the right
    /// order of magnitude.
    fn put_shape_counts(&self, header: &mut [u8]) {
        let clamp = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
        header[field::OBJECT_COUNT..field::OBJECT_COUNT + 2]
            .copy_from_slice(&clamp(self.objects).to_le_bytes());
        header[field::ARRAY_COUNT..field::ARRAY_COUNT + 2]
            .copy_from_slice(&clamp(self.arrays).to_le_bytes());
    }

    /// Lay out buffers 1 and 2, compress each, and fill in the v5 header.
    fn finish_v5(self, header: &mut [u8], options: &WriteOptions) -> Result<Vec<u8>> {
        let compression = options.compression;
        let string_count = size(self.string_ids.len(), "string count")?;
        let [main, aux] = &self.pools;

        let mut buf1 = self.string_blob.clone();
        buf1.extend_from_slice(&aux.bytes);
        let binary_bytes = buf1.len();
        aux.append_wide(&mut buf1, Some(string_count));

        let mut buf2 = Vec::new();
        for l in &self.object_lengths {
            buf2.extend_from_slice(&l.to_le_bytes());
        }
        buf2.extend_from_slice(&main.bytes);
        main.append_wide(&mut buf2, None);
        buf2.extend_from_slice(&self.types);
        for b in &self.blobs {
            buf2.extend_from_slice(&size(b.len(), "blob")?.to_le_bytes());
        }
        buf2.extend_from_slice(&crate::decode::TRAILER_BYTES);
        let (area, chunk_sizes) = blob_area(&self.blobs, compression, options.lz4_chunk())?;
        for c in &chunk_sizes {
            buf2.extend_from_slice(&c.to_le_bytes());
        }

        put_size(header, field::BINARY_BYTES, binary_bytes, "buffer 1 bytes")?;
        put_size(
            header,
            field::INTEGERS,
            aux.ints.len() + 1,
            "buffer 1 4-byte pool",
        )?;
        put_size(
            header,
            field::EIGHT_BYTES,
            aux.eights.len(),
            "buffer 1 8-byte pool",
        )?;
        put_size(
            header,
            field::TWO_BYTES,
            aux.twos.len(),
            "buffer 1 2-byte pool",
        )?;
        self.put_shape_counts(header);
        put_size(header, field::VALUE_COUNT, self.values, "value count")?;
        put_size(header, field::B2_ARRAYS, self.counted_arrays, "array count")?;
        put_size(
            header,
            field::B2_ARRAY_ELEMENTS,
            self.counted_elements,
            "array elements",
        )?;
        put_size(header, field::TYPE_COUNT, self.types.len(), "type stream")?;
        put_size(header, field::B2_BYTES, main.bytes.len(), "1-byte pool")?;
        put_size(header, field::B2_TWO_BYTES, main.twos.len(), "2-byte pool")?;
        put_size(header, field::B2_INTEGERS, main.ints.len(), "4-byte pool")?;
        put_size(
            header,
            field::B2_EIGHT_BYTES,
            main.eights.len(),
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
        put_size(
            header,
            field::BLOB_CHUNK_TABLE,
            chunk_sizes.len() * 2,
            "blob chunk table",
        )?;

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
            payload.extend_from_slice(&crate::decode::TRAILER_BYTES);
        }
        Ok(payload)
    }

    /// Lay out the single v3 or v4 buffer, compress it, and fill in the header.
    fn finish_v4(mut self, header: &mut [u8], options: &WriteOptions) -> Result<Vec<u8>> {
        let compression = options.compression;
        let string_count = size(self.string_ids.len(), "string count")?;
        let pool = &mut self.pools[0];
        pool.ints.insert(0, string_count);

        let mut payload = pool.bytes.clone();
        pool.append_wide(&mut payload, None);
        // The string blob starts on an 8-byte boundary even when the 8-byte pool is empty.
        pad_to(&mut payload, 8);
        let region = self.string_blob.len() + self.types.len();
        payload.extend_from_slice(&self.string_blob);
        payload.extend_from_slice(&self.types);
        for b in &self.blobs {
            payload.extend_from_slice(&size(b.len(), "blob")?.to_le_bytes());
        }
        payload.extend_from_slice(&crate::decode::TRAILER_BYTES);
        let (area, chunk_sizes) = blob_area(&self.blobs, compression, options.lz4_chunk())?;
        for c in &chunk_sizes {
            payload.extend_from_slice(&c.to_le_bytes());
        }

        let blob_total: usize = self.blobs.iter().map(Vec::len).sum();
        put_size(header, field::BLOB_COUNT, self.blobs.len(), "blob count")?;
        put_size(header, field::BLOB_TOTAL_SIZE, blob_total, "blob total")?;
        put_size(header, field::BINARY_BYTES, pool.bytes.len(), "1-byte pool")?;
        put_size(header, field::INTEGERS, pool.ints.len(), "4-byte pool")?;
        put_size(header, field::EIGHT_BYTES, pool.eights.len(), "8-byte pool")?;
        put_size(header, field::TWO_BYTES, pool.twos.len(), "2-byte pool")?;
        self.put_shape_counts(header);
        put_size(header, field::TYPE_COUNT, region, "string and type region")?;

        let compressed = compress(&payload, compression)?;
        put_size(header, field::UNCOMPRESSED_SIZE, payload.len(), "payload")?;
        // Only zstd counts the blob frames here; LZ4 and stored files stop at the buffer.
        let counted_area = if compression == Compression::Zstd {
            area.len()
        } else {
            0
        };
        put_size(
            header,
            field::COMPRESSED_SIZE,
            compressed.len() + counted_area,
            "compressed payload",
        )?;
        let mut block = compressed;
        if !self.blobs.is_empty() {
            block.extend_from_slice(&area);
            block.extend_from_slice(&crate::decode::TRAILER_BYTES);
        }
        Ok(block)
    }

    /// Lay out the v1 or v2 payload: bytes, 4-byte integers led by the string count, 8-byte
    /// values, strings, the type stream and the trailer.
    fn finish_flat(mut self, header: &mut [u8], compression: Compression) -> Result<Vec<u8>> {
        let string_count = size(self.string_ids.len(), "string count")?;
        let pool = &mut self.pools[0];
        pool.ints.insert(0, string_count);

        let mut payload = pool.bytes.clone();
        pad_to(&mut payload, 4);
        for i in &pool.ints {
            payload.extend_from_slice(&i.to_le_bytes());
        }
        // The strings start on an 8-byte boundary whether or not there are 8-byte values.
        pad_to(&mut payload, 8);
        for e in &pool.eights {
            payload.extend_from_slice(&e.to_le_bytes());
        }
        payload.extend_from_slice(&self.string_blob);
        payload.extend_from_slice(&self.types);
        payload.extend_from_slice(&crate::decode::TRAILER_BYTES);

        let counts = header.len() - 16;
        put_size(header, counts, pool.bytes.len(), "1-byte pool")?;
        put_size(header, counts + 4, pool.ints.len(), "4-byte pool")?;
        put_size(header, counts + 8, pool.eights.len(), "8-byte pool")?;
        put_size(header, counts + 12, payload.len(), "payload")?;
        compress(&payload, compression)
    }
}

/// The compressed blob area, and on LZ4 the compressed length of each chunk.
///
/// LZ4 compresses each blob in chunks of the frame size, one block per chunk, which the reader
/// undoes chunk by chunk. zstd takes the blobs back to back as a single stream.
fn blob_area(
    blobs: &[Vec<u8>],
    compression: Compression,
    chunk_len: usize,
) -> Result<(Vec<u8>, Vec<u16>)> {
    let mut chunk_sizes = Vec::new();
    if blobs.is_empty() {
        return Ok((Vec::new(), chunk_sizes));
    }
    let area = match compression {
        Compression::Lz4 => {
            let mut area = Vec::new();
            let mut stream: Vec<u8> = Vec::new();
            for chunk in blobs.iter().flat_map(|b| b.chunks(chunk_len)) {
                let block = compress_lz4_chunk(chunk, &stream)?;
                stream.extend_from_slice(chunk);
                chunk_sizes.push(u16::try_from(block.len()).map_err(|_| {
                    Error::Invalid("KV3 blob chunk does not fit a 16-bit length".into())
                })?);
                area.extend_from_slice(&block);
            }
            area
        }
        other => compress(&blobs.concat(), other)?,
    };
    Ok((area, chunk_sizes))
}
