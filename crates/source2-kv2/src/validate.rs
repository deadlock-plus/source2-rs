//! Checks shared by the writers.

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
    for e in &doc.elements {
        if e.class.is_empty() {
            return Err(Error::InvalidModel(
                "element with an empty class name".into(),
            ));
        }
        attributes(doc, &e.attributes, true)?;
    }
    for p in &doc.prefix {
        attributes(doc, p, false)?;
    }
    Ok(())
}

fn attributes(doc: &Document, attrs: &[Attribute], elements_ok: bool) -> Result<()> {
    for a in attrs {
        if a.name.is_empty() {
            return Err(Error::InvalidModel("attribute with an empty name".into()));
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
