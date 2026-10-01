use crate::{Directive, DirectiveKind, Document, Entry, Error, Options, Result, Value};
use std::fmt::Write as _;

pub(crate) fn write(doc: &Document, options: &Options) -> Result<String> {
    let mut out = String::new();
    for Directive { kind, path } in &doc.directives {
        let name = match kind {
            DirectiveKind::Include => "#include",
            DirectiveKind::Base => "#base",
        };
        out.push_str(name);
        out.push(' ');
        quote(&mut out, path, options)?;
        out.push('\n');
    }
    if !doc.directives.is_empty() && !doc.roots.is_empty() {
        out.push('\n');
    }
    for e in &doc.roots {
        entry(&mut out, e, 0, options)?;
    }
    Ok(out)
}

fn entry(out: &mut String, e: &Entry, depth: usize, options: &Options) -> Result<()> {
    let indent = "\t".repeat(depth);
    out.push_str(&indent);
    quote(out, &e.key, options)?;
    if let Some(c) = &e.condition
        && c.contains([']', '\n'])
    {
        return Err(Error::InvalidInput(format!(
            "conditional tag {c:?} contains ']' or a newline"
        )));
    }
    match &e.value {
        Value::Section(children) => {
            if depth >= options.max_depth {
                return Err(Error::TooDeep {
                    limit: options.max_depth,
                });
            }
            condition(out, e);
            out.push('\n');
            out.push_str(&indent);
            out.push_str("{\n");
            for c in children {
                entry(out, c, depth + 1, options)?;
            }
            out.push_str(&indent);
            out.push_str("}\n");
        }
        scalar => {
            out.push_str("\t\t");
            quote(out, &scalar_text(scalar), options)?;
            condition(out, e);
            out.push('\n');
        }
    }
    Ok(())
}

fn condition(out: &mut String, e: &Entry) {
    if let Some(c) = &e.condition {
        let _ = write!(out, " [{c}]");
    }
}

fn scalar_text(v: &Value) -> String {
    match v {
        Value::String(s) | Value::WString(s) => s.clone(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Ptr(p) => p.to_string(),
        Value::UInt64(u) => u.to_string(),
        Value::Color([r, g, b, a]) => format!("{r} {g} {b} {a}"),
        Value::Section(_) => unreachable!("sections are written by the caller"),
    }
}

fn quote(out: &mut String, s: &str, options: &Options) -> Result<()> {
    out.push('"');
    for c in s.chars() {
        if options.escape_sequences {
            match c {
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                '\\' => out.push_str("\\\\"),
                '"' => out.push_str("\\\""),
                c => out.push(c),
            }
        } else if c == '"' {
            return Err(Error::InvalidInput(format!(
                "{s:?} contains '\"', which needs escape sequences"
            )));
        } else {
            out.push(c);
        }
    }
    out.push('"');
    Ok(())
}
