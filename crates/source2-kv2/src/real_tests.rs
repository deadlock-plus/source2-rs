//! Tests against real DMX files. They need `SOURCE2_KV2_SAMPLES`: one or more directories,
//! separated like `PATH`, searched recursively. They pass without checking anything when it
//! is unset.
//!
//! Run with `SOURCE2_KV2_SAMPLES=... cargo test -p source2-kv2 -- --nocapture`.

use crate::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const MAX_FILES: usize = 20_000;

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if out.len() >= MAX_FILES {
            return;
        }
        if p.is_dir() {
            walk(&p, out);
        } else if p.is_file() {
            out.push(p);
        }
    }
}

fn samples() -> Option<Vec<(PathBuf, Vec<u8>)>> {
    let var = std::env::var_os("SOURCE2_KV2_SAMPLES")?;
    let mut files = Vec::new();
    for dir in std::env::split_paths(&var) {
        walk(&dir, &mut files);
    }
    let mut out = Vec::new();
    for f in files {
        if std::fs::metadata(&f).is_ok_and(|m| m.len() > 512 * 1024 * 1024) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&f) else {
            continue;
        };
        if bytes.starts_with(b"<!-- dmx ") {
            out.push((f, bytes));
        }
    }
    Some(out)
}

fn first_difference(a: &[u8], b: &[u8]) -> usize {
    a.iter()
        .zip(b)
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()))
}

#[test]
fn sample_files_write_back_byte_identical() {
    let Some(files) = samples() else {
        eprintln!("skipped: SOURCE2_KV2_SAMPLES is not set");
        return;
    };
    assert!(!files.is_empty(), "no DMX files under SOURCE2_KV2_SAMPLES");
    let mut by_kind: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut failures = Vec::new();
    for (path, bytes) in &files {
        let doc = match Document::parse(bytes) {
            Ok(d) => d,
            Err(e) => {
                failures.push(format!("{}: parse: {e}", path.display()));
                continue;
            }
        };
        let kind = format!("{} {}", doc.encoding.name(), doc.encoding_version);
        let entry = by_kind.entry(kind).or_default();
        entry.1 += 1;
        match doc.to_bytes() {
            Ok(out) if out == *bytes => entry.0 += 1,
            Ok(out) => failures.push(format!(
                "{}: differs at byte {} (sizes {} vs {})",
                path.display(),
                first_difference(&out, bytes),
                out.len(),
                bytes.len()
            )),
            Err(e) => failures.push(format!("{}: write: {e}", path.display())),
        }
        let again = Document::parse(&doc.to_bytes().unwrap_or_default());
        assert!(
            again.is_ok_and(|d| d == doc),
            "{}: second parse differs",
            path.display()
        );
    }
    for (kind, (ok, total)) in &by_kind {
        eprintln!("{kind}: {ok}/{total} byte-identical");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
