//! Binary DMX reader.
//!
//! What changes between versions:
//!
//! - 1: every string is inline and NUL-terminated.
//! - 2 and up: a string table follows the prefix; class and attribute names are indices
//!   into it.
//! - 3 and up: type 7 is a time, not an object id.
//! - 4 and up: element names and scalar string values are table indices too. String
//!   arrays stay inline.
//! - 4: the table count is 4 bytes; indices stay 2 bytes.
//! - 5 and up: every index is 4 bytes as well.
//! - 9: Source 2. Prefix elements come first, written with inline strings because the
//!   table is not read yet. Adds `uint64` and `uint8`, and array types are offset by 32
//!   instead of 14.

use crate::{
    Attribute, Color, Document, Element, ElementId, ElementRef, Error, Prefix, Result, Time, Uuid,
    Value, ValueType,
};
use std::collections::HashSet;

/// Initial capacity cap for vectors sized by untrusted counts, so memory use tracks the
/// data actually present rather than what a header claims.
const PREALLOC: usize = 1024;

#[derive(Clone, Copy)]
pub(crate) struct Layout {
    pub version: u32,
}

impl Layout {
    pub fn table(self) -> bool {
        self.version >= 2
    }

    pub fn wide(self) -> bool {
        self.version >= 5
    }

    pub fn wide_count(self) -> bool {
        self.version >= 4
    }

    pub fn names_in_table(self) -> bool {
        self.version >= 4
    }

    pub fn source2(self) -> bool {
        self.version >= 9
    }

    pub fn index_width(self) -> usize {
        if self.wide() { 4 } else { 2 }
    }

    pub fn array_offset(self) -> u8 {
        if self.source2() { 32 } else { 14 }
    }

    fn max_base(self) -> u8 {
        if self.source2() { 16 } else { 14 }
    }

    /// Type tag of a scalar, if this version can store it.
    pub fn base_code(self, t: ValueType) -> Option<u8> {
        Some(match t {
            ValueType::Element => 1,
            ValueType::Int => 2,
            ValueType::Float => 3,
            ValueType::Bool => 4,
            ValueType::String => 5,
            ValueType::Binary => 6,
            ValueType::ObjectId if self.version < 3 => 7,
            ValueType::Time if self.version >= 3 => 7,
            ValueType::Color => 8,
            ValueType::Vector2 => 9,
            ValueType::Vector3 => 10,
            ValueType::Vector4 => 11,
            ValueType::QAngle => 12,
            ValueType::Quaternion => 13,
            ValueType::Matrix => 14,
            ValueType::UInt64 if self.source2() => 15,
            ValueType::UInt8 if self.source2() => 16,
            _ => return None,
        })
    }

    /// Scalar type and array flag for a type tag.
    fn decode_tag(self, tag: u8) -> Option<(ValueType, bool)> {
        let off = self.array_offset();
        let (base, array) = if tag > off {
            (tag - off, true)
        } else {
            (tag, false)
        };
        if base == 0 || base > self.max_base() {
            return None;
        }
        let t = match base {
            1 => ValueType::Element,
            2 => ValueType::Int,
            3 => ValueType::Float,
            4 => ValueType::Bool,
            5 => ValueType::String,
            6 => ValueType::Binary,
            7 if self.version < 3 => ValueType::ObjectId,
            7 => ValueType::Time,
            8 => ValueType::Color,
            9 => ValueType::Vector2,
            10 => ValueType::Vector3,
            11 => ValueType::Vector4,
            12 => ValueType::QAngle,
            13 => ValueType::Quaternion,
            14 => ValueType::Matrix,
            15 => ValueType::UInt64,
            _ => ValueType::UInt8,
        };
        Some((t, array))
    }
}

/// Smallest number of bytes one array item of this type occupies.
fn min_item_size(t: ValueType) -> usize {
    match t {
        ValueType::Bool | ValueType::String | ValueType::UInt8 => 1,
        ValueType::Element
        | ValueType::Int
        | ValueType::Float
        | ValueType::Binary
        | ValueType::Time
        | ValueType::Color => 4,
        ValueType::Vector2 | ValueType::UInt64 => 8,
        ValueType::Vector3 | ValueType::QAngle => 12,
        ValueType::Vector4 | ValueType::Quaternion | ValueType::ObjectId => 16,
        ValueType::Matrix => 64,
    }
}

fn malformed<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::Malformed(msg.into()))
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.b.len() - self.pos
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.remaining() {
            return malformed("unexpected end of data");
        }
        let s = &self.b[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    fn floats<const N: usize>(&mut self) -> Result<[f32; N]> {
        let mut out = [0f32; N];
        for x in &mut out {
            *x = self.f32()?;
        }
        Ok(out)
    }

    fn cstr(&mut self) -> Result<String> {
        let rest = &self.b[self.pos..];
        let Some(end) = rest.iter().position(|&c| c == 0) else {
            return malformed("string is not NUL-terminated");
        };
        let s = std::str::from_utf8(&rest[..end])
            .map_err(|_| Error::Malformed("string is not valid UTF-8".into()))?
            .to_string();
        self.pos += end + 1;
        Ok(s)
    }

    /// Reads an `i32` count and checks that `count` items of at least `min_item` bytes
    /// could fit in what is left.
    fn count(&mut self, min_item: usize) -> Result<usize> {
        let n = self.i32()?;
        let Ok(n) = usize::try_from(n) else {
            return malformed(format!("negative count {n}"));
        };
        if n.checked_mul(min_item)
            .is_none_or(|bytes| bytes > self.remaining())
        {
            return malformed(format!("count {n} exceeds the data present"));
        }
        Ok(n)
    }

    fn uuid(&mut self) -> Result<Uuid> {
        Ok(Uuid::from_guid_bytes(self.array()?))
    }
}

struct Ctx<'t> {
    l: Layout,
    /// `None` while reading the prefix, which uses inline strings throughout.
    table: Option<&'t [String]>,
    element_count: usize,
    elements_ok: bool,
}

impl Ctx<'_> {
    fn index(&self, r: &mut Reader<'_>, table: &[String]) -> Result<String> {
        let i = if self.l.wide() {
            let n = r.i32()?;
            match usize::try_from(n) {
                Ok(i) => i,
                Err(_) => return malformed(format!("negative string index {n}")),
            }
        } else {
            usize::from(r.u16()?)
        };
        match table.get(i) {
            Some(s) => Ok(s.clone()),
            None => malformed(format!(
                "string index {i} outside the table of {}",
                table.len()
            )),
        }
    }

    /// A class or attribute name.
    fn symbol(&self, r: &mut Reader<'_>) -> Result<String> {
        match self.table {
            Some(t) => self.index(r, t),
            None => r.cstr(),
        }
    }

    /// An element name or a scalar string value.
    fn text(&self, r: &mut Reader<'_>) -> Result<String> {
        match self.table {
            Some(t) if self.l.names_in_table() => self.index(r, t),
            _ => r.cstr(),
        }
    }

    fn symbol_width(&self) -> usize {
        if self.table.is_some() {
            self.l.index_width()
        } else {
            1
        }
    }

    fn scalar(&self, r: &mut Reader<'_>, t: ValueType, in_array: bool) -> Result<Value> {
        Ok(match t {
            ValueType::Element => {
                if !self.elements_ok {
                    return malformed("prefix attribute holds an element");
                }
                let n = r.i32()?;
                match n {
                    -1 => Value::Element(ElementRef::Null),
                    -2 => {
                        let s = r.cstr()?;
                        match Uuid::parse(&s) {
                            Some(u) => Value::Element(ElementRef::External(u)),
                            None => return malformed(format!("`{s}` is not an element id")),
                        }
                    }
                    n => match usize::try_from(n) {
                        Ok(i) if i < self.element_count => {
                            Value::Element(ElementRef::Element(ElementId(i as u32)))
                        }
                        _ => return malformed(format!("element index {n} out of range")),
                    },
                }
            }
            ValueType::Int => Value::Int(r.i32()?),
            ValueType::Float => Value::Float(r.f32()?),
            ValueType::Bool => Value::Bool(r.u8()? != 0),
            ValueType::String => Value::String(if in_array { r.cstr()? } else { self.text(r)? }),
            ValueType::Binary => {
                let n = r.count(1)?;
                Value::Binary(r.take(n)?.to_vec())
            }
            ValueType::ObjectId => Value::ObjectId(r.uuid()?),
            ValueType::Time => Value::Time(Time(r.i32()?)),
            ValueType::Color => {
                let c = r.array::<4>()?;
                Value::Color(Color {
                    r: c[0],
                    g: c[1],
                    b: c[2],
                    a: c[3],
                })
            }
            ValueType::Vector2 => Value::Vector2(r.floats()?),
            ValueType::Vector3 => Value::Vector3(r.floats()?),
            ValueType::Vector4 => Value::Vector4(r.floats()?),
            ValueType::QAngle => Value::QAngle(r.floats()?),
            ValueType::Quaternion => Value::Quaternion(r.floats()?),
            ValueType::Matrix => Value::Matrix(r.floats()?),
            ValueType::UInt64 => Value::UInt64(r.u64()?),
            ValueType::UInt8 => Value::UInt8(r.u8()?),
        })
    }

    fn attributes(&self, r: &mut Reader<'_>) -> Result<Vec<Attribute>> {
        let n = r.count(self.symbol_width() + 1)?;
        let mut attrs = Vec::with_capacity(n.min(PREALLOC));
        for _ in 0..n {
            let name = self.symbol(r)?;
            let tag = r.u8()?;
            let Some((t, array)) = self.l.decode_tag(tag) else {
                return malformed(format!("attribute `{name}` has unknown type {tag}"));
            };
            let value = if array {
                let n = r.count(min_item_size(t))?;
                let mut items = Vec::with_capacity(n.min(PREALLOC));
                for _ in 0..n {
                    items.push(self.scalar(r, t, true)?);
                }
                Value::Array(t, items)
            } else {
                self.scalar(r, t, false)?
            };
            attrs.push(Attribute { name, value });
        }
        Ok(attrs)
    }
}

pub(crate) fn parse(mut doc: Document, body: &[u8]) -> Result<Document> {
    let l = Layout {
        version: doc.encoding_version,
    };
    let mut r = Reader { b: body, pos: 0 };

    if l.source2() {
        let prefix = Ctx {
            l,
            table: None,
            element_count: 0,
            elements_ok: false,
        };
        let n = r.count(4)?;
        for _ in 0..n {
            doc.prefix.push(Prefix {
                id: None,
                attributes: prefix.attributes(&mut r)?,
            });
        }
    }

    let mut table = Vec::new();
    if l.table() {
        let n = if l.wide_count() {
            r.count(1)?
        } else {
            let n = usize::from(r.u16()?);
            if n > r.remaining() {
                return malformed(format!("string table of {n} exceeds the data present"));
            }
            n
        };
        table.reserve(n.min(PREALLOC));
        for _ in 0..n {
            table.push(r.cstr()?);
        }
    }
    doc.string_table.clone_from(&table);

    let ctx = Ctx {
        l,
        table: l.table().then_some(table.as_slice()),
        element_count: 0,
        elements_ok: true,
    };
    let name_width = if l.names_in_table() {
        l.index_width()
    } else {
        1
    };
    let count = r.count(ctx.symbol_width() + name_width + 16)?;
    let ctx = Ctx {
        element_count: count,
        ..ctx
    };

    let mut ids = HashSet::with_capacity(count.min(PREALLOC));
    doc.elements.reserve(count.min(PREALLOC));
    for _ in 0..count {
        let class = ctx.symbol(&mut r)?;
        let name = ctx.text(&mut r)?;
        let id = r.uuid()?;
        if !ids.insert(id) {
            return Err(Error::DuplicateId(id));
        }
        doc.elements.push(Element::from_parts(class, name, id));
    }
    for i in 0..count {
        doc.elements[i].attributes = ctx.attributes(&mut r)?;
    }
    if r.remaining() != 0 {
        return malformed(format!(
            "{} unread bytes after the last element",
            r.remaining()
        ));
    }
    Ok(doc)
}
