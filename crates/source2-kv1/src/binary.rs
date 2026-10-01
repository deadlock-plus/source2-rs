use crate::encoding::decode_1252;
use crate::layout::DocumentLayout;
use crate::{Document, Encoding, Entry, Error, Options, Result, Value};

const TYPE_SECTION: u8 = 0;
const TYPE_STRING: u8 = 1;
const TYPE_INT: u8 = 2;
const TYPE_FLOAT: u8 = 3;
const TYPE_PTR: u8 = 4;
const TYPE_WSTRING: u8 = 5;
const TYPE_COLOR: u8 = 6;
const TYPE_UINT64: u8 = 7;
const TYPE_END: u8 = 8;
const TYPE_INT64: u8 = 10;

fn malformed(msg: impl Into<String>) -> Error {
    Error::MalformedBinary(msg.into())
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    max_depth: usize,
    encoding: Encoding,
    saw_non_utf8: bool,
}

pub(crate) fn read(data: &[u8], options: &Options) -> Result<Document> {
    let mut encoding = Encoding::Utf8;
    loop {
        let mut r = Reader {
            data,
            pos: 0,
            max_depth: options.max_depth,
            encoding,
            saw_non_utf8: false,
        };
        let (roots, end_marker) = r.top_level()?;
        if r.saw_non_utf8 {
            encoding = Encoding::Windows1252;
            continue;
        }
        if r.pos < data.len() {
            return Err(malformed(format!(
                "{} trailing bytes after the final end marker",
                data.len() - r.pos
            )));
        }
        return Ok(Document {
            roots,
            encoding,
            layout: DocumentLayout {
                end_marker,
                ..DocumentLayout::default()
            },
            ..Document::default()
        });
    }
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.data.len())
            .ok_or_else(|| malformed(format!("truncated at byte {}", self.pos)))?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut a = [0; N];
        a.copy_from_slice(self.take(N)?);
        Ok(a)
    }

    fn cstring(&mut self) -> Result<String> {
        let rest = &self.data[self.pos..];
        let len = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| malformed(format!("unterminated string at byte {}", self.pos)))?;
        let bytes = &rest[..len];
        let s = match (self.encoding, std::str::from_utf8(bytes)) {
            (Encoding::Utf8, Ok(s)) => s.to_owned(),
            (Encoding::Utf8, Err(_)) => {
                self.saw_non_utf8 = true;
                String::new()
            }
            (Encoding::Windows1252, _) => decode_1252(bytes),
        };
        self.pos += len + 1;
        Ok(s)
    }

    fn top_level(&mut self) -> Result<(Vec<Entry>, bool)> {
        let mut end_marker = false;
        let roots = self.entries(0, &mut end_marker)?;
        Ok((roots, end_marker))
    }

    // `depth` is the nesting of the list being read; the implicit top level is 0. Only the
    // top level may end at EOF instead of an end marker.
    fn entries(&mut self, depth: usize, ended: &mut bool) -> Result<Vec<Entry>> {
        if depth > self.max_depth {
            return Err(Error::TooDeep {
                limit: self.max_depth,
            });
        }
        let mut out = Vec::new();
        loop {
            if self.pos >= self.data.len() {
                if depth == 0 {
                    *ended = false;
                    return Ok(out);
                }
                return Err(malformed("truncated inside a section"));
            }
            let offset = self.pos;
            let ty = self.take(1)?[0];
            if ty == TYPE_END {
                *ended = true;
                return Ok(out);
            }
            if !matches!(
                ty,
                TYPE_SECTION
                    | TYPE_STRING
                    | TYPE_INT
                    | TYPE_FLOAT
                    | TYPE_PTR
                    | TYPE_WSTRING
                    | TYPE_COLOR
                    | TYPE_UINT64
                    | TYPE_INT64
            ) {
                return Err(Error::UnsupportedType {
                    type_byte: ty,
                    offset,
                });
            }
            let key = self.cstring()?;
            let value = match ty {
                TYPE_SECTION => Value::Section(self.entries(depth + 1, &mut false)?),
                TYPE_STRING => Value::String(self.cstring()?),
                TYPE_INT => Value::Int(i32::from_le_bytes(self.array()?)),
                TYPE_FLOAT => Value::Float(f32::from_le_bytes(self.array()?)),
                TYPE_PTR => Value::Ptr(u32::from_le_bytes(self.array()?)),
                TYPE_WSTRING => {
                    let len = usize::from(u16::from_le_bytes(self.array()?));
                    let raw = self.take(len * 2)?;
                    let units = raw
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| u16::from_le_bytes(*c));
                    let s = char::decode_utf16(units)
                        .collect::<std::result::Result<String, _>>()
                        .map_err(|_| malformed("wide string is not valid UTF-16"))?;
                    Value::WString(s)
                }
                TYPE_COLOR => Value::Color(self.array()?),
                TYPE_UINT64 => Value::UInt64(u64::from_le_bytes(self.array()?)),
                _ => Value::Int64(i64::from_le_bytes(self.array()?)),
            };
            out.push(Entry::new(key, value));
        }
    }
}

pub(crate) fn write(doc: &Document, options: &Options) -> Result<Vec<u8>> {
    if !doc.directives.is_empty() {
        return Err(Error::InvalidInput(
            "binary KV1 cannot hold #include or #base".into(),
        ));
    }
    let mut out = Vec::new();
    entries(&mut out, &doc.roots, 0, options, doc.encoding)?;
    if doc.layout.end_marker {
        out.push(TYPE_END);
    }
    Ok(out)
}

fn cstring(out: &mut Vec<u8>, s: &str, encoding: Encoding) -> Result<()> {
    if s.contains('\0') {
        return Err(Error::InvalidInput(format!("{s:?} contains a NUL byte")));
    }
    let bytes = encoding
        .encode(s)
        .map_err(|c| Error::InvalidInput(format!("{c:?} cannot be written in Windows-1252")))?;
    out.extend_from_slice(&bytes);
    out.push(0);
    Ok(())
}

fn entries(
    out: &mut Vec<u8>,
    list: &[Entry],
    depth: usize,
    options: &Options,
    encoding: Encoding,
) -> Result<()> {
    if depth > options.max_depth {
        return Err(Error::TooDeep {
            limit: options.max_depth,
        });
    }
    for e in list {
        if e.condition.is_some() {
            return Err(Error::InvalidInput(
                "binary KV1 cannot hold conditional tags".into(),
            ));
        }
        let ty = match &e.value {
            Value::Section(_) => TYPE_SECTION,
            Value::String(_) => TYPE_STRING,
            Value::Int(_) => TYPE_INT,
            Value::Float(_) => TYPE_FLOAT,
            Value::Ptr(_) => TYPE_PTR,
            Value::WString(_) => TYPE_WSTRING,
            Value::Color(_) => TYPE_COLOR,
            Value::UInt64(_) => TYPE_UINT64,
            Value::Int64(_) => TYPE_INT64,
        };
        out.push(ty);
        cstring(out, &e.key, encoding)?;
        match &e.value {
            Value::Section(children) => {
                entries(out, children, depth + 1, options, encoding)?;
                out.push(TYPE_END);
            }
            Value::String(s) => cstring(out, s, encoding)?,
            Value::Int(i) => out.extend_from_slice(&i.to_le_bytes()),
            Value::Float(f) => out.extend_from_slice(&f.to_le_bytes()),
            Value::Ptr(p) => out.extend_from_slice(&p.to_le_bytes()),
            Value::WString(s) => {
                let units: Vec<u16> = s.encode_utf16().collect();
                let len = u16::try_from(units.len()).map_err(|_| {
                    Error::InvalidInput(format!(
                        "wide string of {} units exceeds 65535",
                        units.len()
                    ))
                })?;
                out.extend_from_slice(&len.to_le_bytes());
                for u in units {
                    out.extend_from_slice(&u.to_le_bytes());
                }
            }
            Value::Color(c) => out.extend_from_slice(c),
            Value::UInt64(u) => out.extend_from_slice(&u.to_le_bytes()),
            Value::Int64(i) => out.extend_from_slice(&i.to_le_bytes()),
        }
    }
    Ok(())
}
