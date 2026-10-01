//! Tests against real packs. They need a directory of `.vpk` files named by the
//! `SOURCE2_VPK_SAMPLES` environment variable and skip (pass without checking anything)
//! when it is unset. The test that reads every archive in full is also `#[ignore]`d.
//!
//! Run the full set with `cargo test -p source2-vpk --release -- --include-ignored`.

use crate::*;
use std::path::{Path, PathBuf};

fn root() -> Option<PathBuf> {
    std::env::var_os("SOURCE2_VPK_SAMPLES").map(PathBuf::from)
}

fn is_numbered_archive(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.strip_suffix(".vpk")
        .and_then(|s| s.rsplit_once('_'))
        .is_some_and(|(_, n)| n.len() == 3 && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Every directory file or standalone pack up to two levels below the root.
fn packs() -> Option<Vec<PathBuf>> {
    fn walk(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
        let Ok(read) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth > 0 {
                    walk(&path, depth - 1, out);
                }
            } else if path.extension().is_some_and(|e| e == "vpk") && !is_numbered_archive(&path) {
                out.push(path);
            }
        }
    }
    let Some(root) = root() else {
        eprintln!("SOURCE2_VPK_SAMPLES is unset; skipping");
        return None;
    };
    let mut out = Vec::new();
    walk(&root, 2, &mut out);
    out.sort();
    assert!(!out.is_empty(), "no packs below {}", root.display());
    Some(out)
}

fn archive_bytes(vpk: &Vpk) -> u64 {
    let mut total = 0;
    for n in 0..=0x7ffe_u16 {
        let Ok(path) = vpk.archive_path(n) else {
            break;
        };
        match std::fs::metadata(path) {
            Ok(m) => total += m.len(),
            Err(_) => break,
        }
    }
    total
}

#[test]
fn every_pack_writes_back_to_the_same_directory_bytes() {
    let Some(packs) = packs() else {
        return;
    };
    for path in &packs {
        let original = std::fs::read(path).unwrap();
        let vpk = Vpk::open(path).unwrap();
        let built = vpk.build().unwrap();
        assert!(built.archives.is_empty());
        assert!(
            built.directory == original,
            "{}: {} bytes written, {} read",
            path.display(),
            built.directory.len(),
            original.len()
        );
    }
}

#[test]
fn directory_digests_of_every_pack_verify() {
    let Some(packs) = packs() else {
        return;
    };
    for path in &packs {
        let report = Vpk::open(path).unwrap().verify_directory_md5().unwrap();
        assert!(report.all_ok(), "{}: {report:?}", path.display());
    }
}

#[test]
fn entries_read_with_matching_crcs() {
    let Some(packs) = packs() else {
        return;
    };
    for path in &packs {
        let vpk = Vpk::open(path).unwrap();
        let n = vpk.len();
        for e in vpk
            .entries()
            .iter()
            .take(100)
            .chain(vpk.entries().iter().skip(n.saturating_sub(100)))
        {
            let c = vpk.read_with_crc(e).unwrap();
            assert!(c.crc_ok(), "{}: {}", path.display(), e.path);
            assert_eq!(c.bytes.len(), e.size());
        }
    }
}

fn check_archive_records(limit: Option<u64>) {
    let Some(packs) = packs() else {
        return;
    };
    let mut matched = 0;
    for path in &packs {
        let vpk = Vpk::open(path).unwrap();
        if limit.is_some_and(|l| archive_bytes(&vpk) > l) {
            continue;
        }
        let report = vpk.verify_archive_md5().unwrap();
        assert!(
            report.all_ok(),
            "{}: {} records mismatched, e.g. {:?}",
            path.display(),
            report.mismatched.len(),
            report.mismatched.iter().take(5).collect::<Vec<_>>()
        );
        matched += report.matched;
    }
    assert!(matched > 0, "no record could be checked");
}

#[test]
fn checkable_archive_md5_records_of_the_smaller_packs_verify() {
    check_archive_records(Some(256 << 20));
}

#[test]
#[ignore = "reads every archive in full"]
fn checkable_archive_md5_records_of_every_pack_verify() {
    check_archive_records(None);
}

/// The records of numbered archives in shipped files use an index spelling whose digests
/// are not plain MD5 of the archive bytes, so they come back as unchecked rather than as
/// failures. This pins that behaviour to real data.
#[test]
fn shipped_numbered_archive_records_are_reported_unchecked() {
    let Some(packs) = packs() else {
        return;
    };
    let mut seen = 0;
    for path in &packs {
        let vpk = Vpk::open(path).unwrap();
        let has_flagged = vpk
            .archive_md5
            .as_ref()
            .is_some_and(|r| r.iter().any(|r| r.archive_index & 0x1_0000 != 0));
        if has_flagged && archive_bytes(&vpk) < (256 << 20) {
            let report = vpk.verify_archive_md5().unwrap();
            assert!(!report.unchecked.is_empty());
            assert!(report.mismatched.is_empty());
            seen += 1;
        }
    }
    assert!(seen > 0);
}

/// Dropping the records and writing again must regenerate what the tools that made an
/// inline-only pack wrote, which pins the chunking and index spelling used for the
/// inline data of hand-built documents.
#[test]
fn regenerated_inline_md5_records_match_the_shipped_ones() {
    let Some(packs) = packs() else {
        return;
    };
    let mut compared = 0;
    for path in &packs {
        let vpk = Vpk::open(path).unwrap();
        let records = vpk.archive_md5.clone().unwrap();
        let inline_only =
            !records.is_empty() && records.iter().all(|r| r.target() == Md5Target::Inline);
        let no_archives = vpk
            .entries()
            .iter()
            .all(|e| matches!(e.data, Data::Stored { archive: None, .. }));
        if !inline_only || !no_archives {
            continue;
        }
        let mut again = vpk.clone();
        again.archive_md5 = None;
        let rebuilt = Vpk::parse(&again.build().unwrap().directory).unwrap();
        assert!(
            rebuilt.archive_md5.as_deref() == Some(&records[..]),
            "{}",
            path.display()
        );
        compared += 1;
    }
    assert!(compared > 0, "nothing was comparable");
}
