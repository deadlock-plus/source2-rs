//! Round-trip tests over real files, read-only. Set `SOURCE2_KV1_SAMPLES` to a directory of
//! sample files (searched recursively). Without it the tests pass without checking anything.
//!
//! - Files with a text extension (`vdf`, `vcfg`, `acf`, `txt`, `lst`, `gi`) are parsed as
//!   text; files that are not KV1 text are reported and skipped.
//! - Files with the extension `bin` are parsed as binary KV1.

use crate::{Document, Encoding};
use std::path::{Path, PathBuf};

const TEXT_EXTENSIONS: &[&str] = &["vdf", "vcfg", "acf", "txt", "lst", "gi"];
const BINARY_EXTENSIONS: &[&str] = &["bin"];
const MAX_WALK_DEPTH: usize = 8;

fn samples_dir() -> Option<PathBuf> {
    std::env::var_os("SOURCE2_KV1_SAMPLES").map(PathBuf::from)
}

fn collect(dir: &Path, exts: &[&str], depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth < MAX_WALK_DEPTH {
                collect(&path, exts, depth + 1, out);
            }
        } else if path
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| exts.contains(&x.to_ascii_lowercase().as_str()))
        {
            out.push(path);
        }
    }
}

fn samples(dir: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(dir, exts, 0, &mut out);
    out.sort();
    out
}

#[test]
fn real_text_files_are_byte_identical() {
    let Some(dir) = samples_dir() else {
        eprintln!("SOURCE2_KV1_SAMPLES not set; skipping");
        return;
    };
    let mut utf8 = 0;
    let mut windows_1252 = 0;
    let mut not_kv1 = Vec::new();
    let mut mismatched = Vec::new();
    for path in samples(&dir, TEXT_EXTENSIONS) {
        let bytes = std::fs::read(&path).unwrap();
        let doc = match Document::parse_bytes(&bytes) {
            Ok(doc) => doc,
            Err(e) => {
                not_kv1.push(format!("{} ({e})", path.display()));
                continue;
            }
        };
        if doc.to_text_bytes().ok().as_deref() != Some(bytes.as_slice()) {
            mismatched.push(path.display().to_string());
        } else if doc.encoding == Encoding::Utf8 {
            utf8 += 1;
        } else {
            windows_1252 += 1;
        }
    }
    eprintln!(
        "byte-identical: {} ({utf8} UTF-8, {windows_1252} Windows-1252)",
        utf8 + windows_1252
    );
    eprintln!("not read as KV1 text: {not_kv1:#?}");
    assert!(mismatched.is_empty(), "not byte-identical: {mismatched:#?}");
}

#[test]
fn real_binary_files_are_byte_identical() {
    let Some(dir) = samples_dir() else {
        eprintln!("SOURCE2_KV1_SAMPLES not set; skipping");
        return;
    };
    for path in samples(&dir, BINARY_EXTENSIONS) {
        let bytes = std::fs::read(&path).unwrap();
        let doc =
            Document::from_binary(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(doc.to_binary().unwrap(), bytes, "{}", path.display());
        eprintln!(
            "binary byte-identical: {} ({} bytes)",
            path.display(),
            bytes.len()
        );
    }
}
