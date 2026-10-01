//! Checks shared by the writers.

use crate::value_text::parse_type;
use crate::{Attribute, Document, ElementRef, Encoding, Error, Result, Value};
use std::collections::HashSet;

pub(crate) fn validate(doc: &Document) -> Result<()> {
    if doc.encoding != Encoding::KeyValues2NoIds {
        let mut seen = HashSet::with_capacity(doc.elements.len());
        for e in &doc.elements {
            if !seen.insert(e.id) {
                return Err(Error::DuplicateId(e.id));
            }
        }
    }
    let text = doc.encoding != Encoding::Binary;
    for e in &doc.elements {
        if e.class.is_empty() {
            return Err(Error::InvalidModel(
                "element with an empty class name".into(),
            ));
        }
        if text && parse_type(&e.class).is_some() {
            return Err(Error::InvalidModel(format!(
                "class name `{}` is a type word, which text would read as an attribute",
                e.class
            )));
        }
        attributes(doc, &e.attributes, true, text)?;
    }
    for p in &doc.prefix {
        attributes(doc, &p.attributes, false, text)?;
    }
    Ok(())
}

fn attributes(doc: &Document, attrs: &[Attribute], elements_ok: bool, text: bool) -> Result<()> {
    for a in attrs {
        if a.name.is_empty() {
            return Err(Error::InvalidModel("attribute with an empty name".into()));
        }
        if text && (a.name == "id" || a.name == "name") {
            return Err(Error::InvalidModel(format!(
                "attribute `{}` collides with the element's own `{}` in text",
                a.name, a.name
            )));
        }
        match &a.value {
            Value::Array(ty, items) => {
                for item in items {
                    if item.is_array() || item.value_type() != *ty {
                        return Err(Error::InvalidModel(format!(
                            "array `{}` holds an item that is not {ty:?}",
                            a.name
                        )));
                    }
                    value(doc, &a.name, item, elements_ok)?;
                }
            }
            v => value(doc, &a.name, v, elements_ok)?,
        }
    }
    Ok(())
}

fn value(doc: &Document, name: &str, v: &Value, elements_ok: bool) -> Result<()> {
    if let Value::Element(r) = v {
        if !elements_ok {
            return Err(Error::InvalidModel(format!(
                "prefix attribute `{name}` holds an element"
            )));
        }
        if let ElementRef::Element(id) = r
            && doc.element(*id).is_none()
        {
            return Err(Error::InvalidModel(format!(
                "attribute `{name}` points at missing element {}",
                id.0
            )));
        }
    }
    Ok(())
}
