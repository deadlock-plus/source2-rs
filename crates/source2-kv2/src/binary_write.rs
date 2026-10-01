//! Binary DMX writer. See the reader for the per-version layout.

use crate::binary_read::Layout;
use crate::{Attribute, Document, ElementRef, Error, Result, Value, ValueType};
use std::collections::HashMap;

fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::InvalidModel(msg.into()))
}

fn put_i32(buf: &mut Vec<u8>, n: usize) -> Result<()> {
    match i32::try_from(n) {
        Ok(n) => {
            buf.extend_from_slice(&n.to_le_bytes());
            Ok(())
        }
        Err(_) => invalid(format!("{n} does not fit a 32-bit count")),
    }
}

fn put_cstr(buf: &mut Vec<u8>, s: &str) -> Result<()> {
    if s.contains('\0') {
        return invalid("strings in binary documents cannot contain NUL");
    }
    buf.extend_from_slice(s.as_bytes());
    buf.push(0);
    Ok(())
}

#[derive(Default)]
struct Table {
    index: HashMap<String, usize>,
    list: Vec<String>,
}

impl Table {
    /// Starts from the strings a document already carries, keeping their order.
    fn seeded(strings: &[String]) -> Self {
        let mut t = Table::default();
        t.list.extend_from_slice(strings);
        for (i, s) in strings.iter().enumerate() {
            t.index.entry(s.clone()).or_insert(i);
        }
        t
    }

    fn intern(&mut self, s: &str) -> usize {
        if let Some(&i) = self.index.get(s) {
            return i;
        }
        let i = self.list.len();
        self.index.insert(s.to_string(), i);
        self.list.push(s.to_string());
        i
    }
}

struct Writer {
    l: Layout,
    table: Table,
}

impl Writer {
    fn put_index(&mut self, buf: &mut Vec<u8>, s: &str) -> Result<()> {
        if s.contains('\0') {
            return invalid("strings in binary documents cannot contain NUL");
        }
        let i = self.table.intern(s);
        if self.l.wide() {
            put_i32(buf, i)
        } else {
            match u16::try_from(i) {
                Ok(i) => {
                    buf.extend_from_slice(&i.to_le_bytes());
                    Ok(())
                }
                Err(_) => invalid("more strings than a 16-bit string table can index"),
            }
        }
    }

    /// A class or attribute name. `shared` is false for prefix attributes, whose strings
    /// are inline.
    fn symbol(&mut self, buf: &mut Vec<u8>, s: &str, shared: bool) -> Result<()> {
        if shared && self.l.table() {
            self.put_index(buf, s)
        } else {
            put_cstr(buf, s)
        }
    }

    /// An element name or a scalar string value.
    fn text(&mut self, buf: &mut Vec<u8>, s: &str, shared: bool) -> Result<()> {
        if shared && self.l.names_in_table() {
            self.put_index(buf, s)
        } else {
            put_cstr(buf, s)
        }
    }

    fn scalar(&mut self, buf: &mut Vec<u8>, v: &Value, in_array: bool, shared: bool) -> Result<()> {
        match v {
            Value::Element(r) => match r {
                ElementRef::Null => buf.extend_from_slice(&(-1i32).to_le_bytes()),
                ElementRef::External(u) => {
                    buf.extend_from_slice(&(-2i32).to_le_bytes());
                    put_cstr(buf, &u.to_string())?;
                }
                ElementRef::Element(id) => put_i32(buf, id.0 as usize)?,
            },
            Value::Int(n) => buf.extend_from_slice(&n.to_le_bytes()),
            Value::Float(x) => buf.extend_from_slice(&x.to_le_bytes()),
            Value::Bool(b) => buf.push(u8::from(*b)),
            Value::String(s) => {
                if in_array {
                    put_cstr(buf, s)?;
                } else {
                    self.text(buf, s, shared)?;
                }
            }
            Value::Binary(b) => {
                put_i32(buf, b.len())?;
                buf.extend_from_slice(b);
            }
            Value::ObjectId(u) => buf.extend_from_slice(&u.to_guid_bytes()),
            Value::Time(t) => buf.extend_from_slice(&t.0.to_le_bytes()),
            Value::Color(c) => buf.extend_from_slice(&[c.r, c.g, c.b, c.a]),
            Value::Vector2(f) => floats(buf, f),
            Value::Vector3(f) | Value::QAngle(f) => floats(buf, f),
            Value::Vector4(f) | Value::Quaternion(f) => floats(buf, f),
            Value::Matrix(f) => floats(buf, f),
            Value::UInt64(n) => buf.extend_from_slice(&n.to_le_bytes()),
            Value::UInt8(n) => buf.push(*n),
            Value::Array(..) => return invalid("arrays cannot nest"),
        }
        Ok(())
    }

    fn attributes(&mut self, buf: &mut Vec<u8>, attrs: &[Attribute], shared: bool) -> Result<()> {
        put_i32(buf, attrs.len())?;
        for a in attrs {
            self.symbol(buf, &a.name, shared)?;
            let t: ValueType = a.value.value_type();
            let Some(base) = self.l.base_code(t) else {
                return invalid(format!(
                    "attribute `{}` is {t:?}, which binary version {} cannot store",
                    a.name, self.l.version
                ));
            };
            match &a.value {
                Value::Array(_, items) => {
                    buf.push(base + self.l.array_offset());
                    put_i32(buf, items.len())?;
                    for item in items {
                        self.scalar(buf, item, true, shared)?;
                    }
                }
                v => {
                    buf.push(base);
                    self.scalar(buf, v, false, shared)?;
                }
            }
        }
        Ok(())
    }
}

fn floats(buf: &mut Vec<u8>, f: &[f32]) {
    for x in f {
        buf.extend_from_slice(&x.to_le_bytes());
    }
}

pub(crate) fn write(doc: &Document, out: &mut Vec<u8>) -> Result<()> {
    let l = Layout {
        version: doc.encoding_version,
    };
    if !doc.prefix.is_empty() && !l.source2() {
        return invalid("prefix elements need binary version 9");
    }
    let mut w = Writer {
        l,
        table: Table::seeded(&doc.string_table),
    };

    if l.source2() {
        put_i32(out, doc.prefix.len())?;
        for p in &doc.prefix {
            w.attributes(out, &p.attributes, false)?;
        }
    }

    let mut body = Vec::new();
    put_i32(&mut body, doc.elements.len())?;
    for e in &doc.elements {
        w.symbol(&mut body, &e.class, true)?;
        w.text(&mut body, &e.name, true)?;
        body.extend_from_slice(&e.id.to_guid_bytes());
    }
    for e in &doc.elements {
        w.attributes(&mut body, &e.attributes, true)?;
    }

    if l.table() {
        if l.wide_count() {
            put_i32(out, w.table.list.len())?;
        } else {
            let Ok(n) = u16::try_from(w.table.list.len()) else {
                return invalid("more strings than a 16-bit string table can hold");
            };
            out.extend_from_slice(&n.to_le_bytes());
        }
        for s in &w.table.list {
            put_cstr(out, s)?;
        }
    }
    out.extend_from_slice(&body);
    Ok(())
}
