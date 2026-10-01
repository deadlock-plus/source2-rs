//! Byte-identity checks against real compiled resources.
//!
//! Set `SOURCE2_RESOURCE_SAMPLES` to a directory. Every file under it whose name ends in
//! `_c` is parsed, written back, and compared with the original bytes. The test passes
//! without checking anything when the variable is unset.
//!
//! ```text
//! SOURCE2_RESOURCE_SAMPLES=/path/to/samples cargo test -p source2-resource real_ -- --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::{Padding, Resource};

const ENV_SAMPLES: &str = "SOURCE2_RESOURCE_SAMPLES";

fn collect_samples(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_samples(&path, out);
        } else if path.to_string_lossy().ends_with("_c") {
            out.push(path);
        }
    }
}

#[test]
fn real_files_round_trip_byte_for_byte() {
    let Some(root) = std::env::var_os(ENV_SAMPLES).map(PathBuf::from) else {
        eprintln!("skipped: {ENV_SAMPLES} is not set");
        return;
    };
    let mut files = Vec::new();
    collect_samples(&root, &mut files);
    files.sort();
    assert!(!files.is_empty(), "no *_c files under {root:?}");

    let mut header_versions: BTreeMap<u16, usize> = BTreeMap::new();
    let mut resource_versions: BTreeMap<u16, usize> = BTreeMap::new();
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let (mut with_trailing, mut with_declared, mut exact_padding) = (0, 0, 0);
    let mut failures = Vec::new();

    for path in &files {
        let name = path.display();
        let bytes = std::fs::read(path).expect("read sample");
        let parsed = match Resource::parse(&bytes) {
            Ok(r) => r,
            Err(e) => {
                failures.push(format!("{name}: parse: {e}"));
                continue;
            }
        };
        *header_versions.entry(parsed.versions.header).or_default() += 1;
        *resource_versions
            .entry(parsed.versions.resource)
            .or_default() += 1;
        for b in &parsed.blocks {
            *kinds.entry(b.kind.to_string()).or_default() += 1;
            if matches!(b.padding, Padding::Exact(_)) {
                exact_padding += 1;
            }
        }
        with_trailing += usize::from(!parsed.trailing.is_empty());
        with_declared += usize::from(parsed.declared_size.is_some());

        match parsed.to_bytes() {
            Ok(out) if out == bytes => {}
            Ok(_) => failures.push(format!("{name}: bytes differ after write")),
            Err(e) => failures.push(format!("{name}: write: {e}")),
        }
        match parsed.to_bytes().and_then(|out| Resource::parse(&out)) {
            Ok(again) if again == parsed => {}
            Ok(_) => failures.push(format!("{name}: model differs after rewrite")),
            Err(e) => failures.push(format!("{name}: reparse: {e}")),
        }
    }

    println!("{} files under {root:?}", files.len());
    println!("  header versions: {header_versions:?}");
    println!("  resource versions: {resource_versions:?}");
    println!("  with trailing data: {with_trailing}, with odd size field: {with_declared}");
    println!("  blocks with non-default padding: {exact_padding}");
    println!("  block kinds: {kinds:?}");
    for f in failures.iter().take(20) {
        println!("  FAIL {f}");
    }
    assert!(failures.is_empty(), "{} failures", failures.len());
}
