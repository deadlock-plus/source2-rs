//! `KeyValues2` text writer.

use crate::text_read::MAX_DEPTH;
use crate::value_text::{array_type_name, escape, format_value, type_name};
use crate::{
    Attribute, Document, ElementId, ElementRef, Encoding, Error, Result, Value, ValueType,
};

/// Without ids a shared element is written once per reference, so a chain of diamonds
/// grows exponentially. This bounds the output.
const MAX_NOIDS_ELEMENTS: usize = 1 << 22;

const PREFIX_CLASS: &str = "$prefix_element$";

struct Writer<'a> {
    doc: &'a Document,
    out: String,
    noids: bool,
    /// With ids: elements already written. Without: elements on the current path.
    marked: Vec<bool>,
    written: usize,
}

fn quoted(out: &mut String, s: &str) {
    out.push('"');
    escape(out, s);
    out.push('"');
}

impl Writer<'_> {
    fn indent(&mut self, n: usize) {
        for _ in 0..n {
            self.out.push('\t');
        }
    }

    fn element_block(
        &mut self,
        id: ElementId,
        key: Option<&str>,
        ind: usize,
        trailing: &str,
        depth: usize,
    ) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(Error::TooDeep);
        }
        let idx = id.0 as usize;
        let doc = self.doc;
        let e = &doc.elements[idx];
        if self.noids {
            if self.marked[idx] {
                return Err(Error::InvalidModel(
                    "elements refer to each other in a cycle, which keyvalues2_noids cannot express"
                        .into(),
                ));
            }
            self.written += 1;
            if self.written > MAX_NOIDS_ELEMENTS {
                return Err(Error::InvalidModel(
                    "shared elements expand to too many copies without ids".into(),
                ));
            }
        }
        self.marked[idx] = true;
        self.indent(ind);
        if let Some(k) = key {
            quoted(&mut self.out, k);
            self.out.push(' ');
        }
        quoted(&mut self.out, &e.class);
        self.out.push('\n');
        self.indent(ind);
        self.out.push_str("{\n");
        if !self.noids {
            self.indent(ind + 1);
            self.out
                .push_str(&format!("\"id\" \"elementid\" \"{}\"\n", e.id));
        }
        if !e.name.is_empty() {
            self.indent(ind + 1);
            self.out.push_str("\"name\" \"string\" ");
            quoted(&mut self.out, &e.name);
            self.out.push('\n');
        }
        self.attributes(&e.attributes, ind + 1, depth)?;
        self.indent(ind);
        self.out.push('}');
        self.out.push_str(trailing);
        self.out.push('\n');
        if self.noids {
            self.marked[idx] = false;
        }
        Ok(())
    }

    fn attributes(&mut self, attrs: &[Attribute], ind: usize, depth: usize) -> Result<()> {
        for a in attrs {
            match &a.value {
                Value::Element(r) => match r {
                    ElementRef::Element(id) if self.inline(*id) => {
                        self.element_block(*id, Some(&a.name), ind, "", depth + 1)?;
                    }
                    other => {
                        let s = self.reference(other)?;
                        self.indent(ind);
                        quoted(&mut self.out, &a.name);
                        self.out.push_str(" \"element\" \"");
                        self.out.push_str(&s);
                        self.out.push_str("\"\n");
                    }
                },
                Value::Array(t, items) => {
                    self.indent(ind);
                    quoted(&mut self.out, &a.name);
                    self.out.push(' ');
                    quoted(&mut self.out, &array_type_name(*t));
                    self.out.push('\n');
                    self.indent(ind);
                    self.out.push_str("[\n");
                    for (i, item) in items.iter().enumerate() {
                        let trailing = if i + 1 < items.len() { "," } else { "" };
                        self.array_item(*t, item, ind + 1, trailing, depth)?;
                    }
                    self.indent(ind);
                    self.out.push_str("]\n");
                }
                v => {
                    self.indent(ind);
                    quoted(&mut self.out, &a.name);
                    self.out.push_str(" \"");
                    self.out.push_str(type_name(v.value_type()));
                    self.out.push_str("\" \"");
                    format_value(&mut self.out, v);
                    self.out.push_str("\"\n");
                }
            }
        }
        Ok(())
    }

    fn array_item(
        &mut self,
        t: ValueType,
        item: &Value,
        ind: usize,
        trailing: &str,
        depth: usize,
    ) -> Result<()> {
        if let Value::Element(r) = item {
            if let ElementRef::Element(id) = r
                && self.inline(*id)
            {
                return self.element_block(*id, None, ind, trailing, depth + 1);
            }
            let s = self.reference(r)?;
            self.indent(ind);
            self.out.push_str("\"element\" \"");
            self.out.push_str(&s);
            self.out.push('"');
            self.out.push_str(trailing);
            self.out.push('\n');
            return Ok(());
        }
        debug_assert_eq!(item.value_type(), t);
        self.indent(ind);
        self.out.push('"');
        format_value(&mut self.out, item);
        self.out.push('"');
        self.out.push_str(trailing);
        self.out.push('\n');
        Ok(())
    }

    /// Whether this occurrence of an element is where its body is written.
    fn inline(&self, id: ElementId) -> bool {
        self.noids || !self.marked[id.0 as usize]
    }

    fn reference(&self, r: &ElementRef) -> Result<String> {
        Ok(match r {
            ElementRef::Null => String::new(),
            ElementRef::External(u) => {
                if self.noids {
                    return Err(Error::InvalidModel(
                        "keyvalues2_noids cannot refer to an element by id".into(),
                    ));
                }
                u.to_string()
            }
            ElementRef::Element(id) => self.doc.elements[id.0 as usize].id.to_string(),
        })
    }
}

pub(crate) fn write(doc: &Document, out: &mut Vec<u8>) -> Result<()> {
    let mut w = Writer {
        doc,
        out: String::new(),
        noids: doc.encoding == Encoding::KeyValues2NoIds,
        marked: vec![false; doc.elements.len()],
        written: 0,
    };
    for p in &doc.prefix {
        w.out.push('"');
        w.out.push_str(PREFIX_CLASS);
        w.out.push_str("\"\n{\n");
        w.attributes(p, 1, 0)?;
        w.out.push_str("}\n");
    }
    if !doc.elements.is_empty() {
        w.element_block(ElementId(0), None, 0, "", 0)?;
    }
    out.extend_from_slice(w.out.as_bytes());
    Ok(())
}
