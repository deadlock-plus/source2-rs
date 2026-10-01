//! `KeyValues2` text writer.

use crate::text_read::MAX_DEPTH;
use crate::value_text::{array_type_name, escape, format_value, type_name};
use crate::{
    Attribute, Document, Element, ElementId, ElementRef, Encoding, Error, Result, Value, ValueType,
};

/// Without ids a shared element is written once per reference, so a chain of diamonds
/// grows exponentially. This bounds the output.
const MAX_NOIDS_ELEMENTS: usize = 1 << 22;

const PREFIX_CLASS: &str = "$prefix_element$";

struct Writer<'a> {
    doc: &'a Document,
    out: String,
    noids: bool,
    nl: &'static str,
    /// With ids: the element's body has been started. Without: it has been reached.
    defined: Vec<bool>,
    /// Without ids: elements on the current path, to catch cycles.
    on_path: Vec<bool>,
    /// Elements started so far with ids. A reader numbers elements in this order, so the
    /// element at this index is the one a nested block would become.
    started: usize,
    written: usize,
}

fn quoted(out: &mut String, s: &str) {
    out.push('"');
    escape(out, s);
    out.push('"');
}

enum Line<'a> {
    Id,
    Name,
    Attr(&'a Attribute),
}

impl Writer<'_> {
    fn indent(&mut self, n: usize) {
        for _ in 0..n {
            self.out.push('\t');
        }
    }

    fn line_end(&mut self) {
        self.out.push_str(self.nl);
    }

    fn blank_line(&mut self, ind: usize, wanted: bool) {
        if wanted {
            self.indent(ind);
            self.line_end();
        }
    }

    /// Writes `"Class" { ... }` followed by `trailing` (a comma in arrays). `key` is the
    /// attribute name when the block is a nested value.
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
            if self.on_path[idx] {
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
            self.on_path[idx] = true;
        } else {
            self.started += 1;
        }
        self.defined[idx] = true;
        self.indent(ind);
        if let Some(k) = key {
            quoted(&mut self.out, k);
            self.out.push(' ');
        }
        quoted(&mut self.out, &e.class);
        self.line_end();
        self.indent(ind);
        self.out.push('{');
        self.line_end();
        self.body(e, ind + 1, depth)?;
        self.indent(ind);
        self.out.push('}');
        self.out.push_str(trailing);
        self.line_end();
        if self.noids {
            self.on_path[idx] = false;
        }
        Ok(())
    }

    fn body(&mut self, e: &Element, ind: usize, depth: usize) -> Result<()> {
        let mut lines: Vec<Line<'_>> = e.attributes.iter().map(Line::Attr).collect();
        let id_at = e.text.id_position;
        let name_at = match e.text.name_position {
            Some(p) => Some(p),
            None if e.name.is_empty() => None,
            None if self.noids => Some(id_at),
            None => Some(id_at + 1),
        };
        let mut specials = Vec::new();
        if !self.noids {
            specials.push((id_at, 0, Line::Id));
        }
        if let Some(p) = name_at {
            specials.push((p, 1, Line::Name));
        }
        specials.sort_by_key(|(p, order, _)| (*p, *order));
        for (p, _, l) in specials {
            lines.insert(p.min(lines.len()), l);
        }
        for l in lines {
            match l {
                Line::Id => {
                    self.indent(ind);
                    self.out
                        .push_str(&format!("\"id\" \"elementid\" \"{}\"", e.id));
                    self.line_end();
                }
                Line::Name => {
                    self.indent(ind);
                    self.out.push_str("\"name\" \"string\" ");
                    quoted(&mut self.out, &e.name);
                    self.line_end();
                }
                Line::Attr(a) => self.attribute(a, ind, depth)?,
            }
        }
        Ok(())
    }

    fn attribute(&mut self, a: &Attribute, ind: usize, depth: usize) -> Result<()> {
        match &a.value {
            Value::Element(r) => match r {
                ElementRef::Element(id) if self.inline(*id) => {
                    self.element_block(*id, Some(&a.name), ind, "", depth + 1)?;
                    self.blank_line(ind, self.doc.text_style.blank_line_after_element);
                }
                other => {
                    let s = self.reference(other)?;
                    self.indent(ind);
                    quoted(&mut self.out, &a.name);
                    self.out.push_str(" \"element\" \"");
                    self.out.push_str(&s);
                    self.out.push('"');
                    self.line_end();
                }
            },
            Value::Array(t, items) => {
                self.indent(ind);
                quoted(&mut self.out, &a.name);
                self.out.push(' ');
                quoted(&mut self.out, &array_type_name(*t));
                let style = self.doc.text_style;
                if style.inline_arrays && (*t != ValueType::Element || items.is_empty()) {
                    self.out.push_str(" [ ");
                    for (i, item) in items.iter().enumerate() {
                        if i > 0 {
                            self.out.push_str(", ");
                        }
                        self.out.push('"');
                        format_value(&mut self.out, item, style.float_format);
                        self.out.push('"');
                    }
                    if !items.is_empty() {
                        self.out.push(' ');
                    }
                    self.out.push(']');
                    self.line_end();
                    return Ok(());
                }
                if style.space_after_array_type {
                    self.out.push(' ');
                }
                self.line_end();
                self.indent(ind);
                self.out.push('[');
                self.line_end();
                let comma = if style.space_after_comma { ", " } else { "," };
                for (i, item) in items.iter().enumerate() {
                    let trailing = if i + 1 < items.len() { comma } else { "" };
                    self.array_item(*t, item, ind + 1, trailing, depth)?;
                }
                self.indent(ind);
                self.out.push(']');
                self.line_end();
            }
            v => {
                self.indent(ind);
                quoted(&mut self.out, &a.name);
                self.out.push_str(" \"");
                self.out.push_str(type_name(v.value_type()));
                self.out.push_str("\" \"");
                format_value(&mut self.out, v, self.doc.text_style.float_format);
                self.out.push('"');
                self.line_end();
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
            self.line_end();
            return Ok(());
        }
        debug_assert_eq!(item.value_type(), t);
        self.indent(ind);
        self.out.push('"');
        format_value(&mut self.out, item, self.doc.text_style.float_format);
        self.out.push('"');
        self.out.push_str(trailing);
        self.line_end();
        Ok(())
    }

    /// Whether this occurrence of an element is where its body is written. With ids that
    /// is the place a reader would number the element, unless the element asks to be a
    /// top-level block.
    fn inline(&self, id: ElementId) -> bool {
        if self.noids {
            return true;
        }
        let i = id.0 as usize;
        !self.defined[i] && !self.doc.elements[i].text.standalone && i == self.started
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

    fn top_level(&mut self, idx: usize) -> Result<()> {
        let id = ElementId(u32::try_from(idx).map_err(|_| Error::TooDeep)?);
        self.element_block(id, None, 0, "", 0)?;
        self.blank_line(0, self.doc.text_style.blank_line_after_block);
        Ok(())
    }
}

pub(crate) fn write(doc: &Document, out: &mut Vec<u8>) -> Result<()> {
    let mut w = Writer {
        doc,
        out: String::new(),
        noids: doc.encoding == Encoding::KeyValues2NoIds,
        nl: doc.text_style.newline.as_str(),
        defined: vec![false; doc.elements.len()],
        on_path: vec![false; doc.elements.len()],
        started: 0,
        written: 0,
    };
    for p in &doc.prefix {
        w.out.push('"');
        w.out.push_str(PREFIX_CLASS);
        w.out.push('"');
        w.line_end();
        w.out.push('{');
        w.line_end();
        if let Some(id) = p.id
            && !w.noids
        {
            w.indent(1);
            w.out.push_str(&format!("\"id\" \"elementid\" \"{id}\""));
            w.line_end();
        }
        for a in &p.attributes {
            w.attribute(a, 1, 0)?;
        }
        w.out.push('}');
        w.line_end();
    }
    // The first element is the root; anything left over (extra top-level blocks, or
    // elements nothing nests) follows in arena order so no element is dropped.
    while let Some(i) = w.defined.iter().position(|d| !d) {
        w.top_level(i)?;
    }
    if !doc.text_style.final_newline && w.out.ends_with(w.nl) {
        w.out.truncate(w.out.len() - w.nl.len());
    }
    out.extend_from_slice(w.out.as_bytes());
    Ok(())
}
