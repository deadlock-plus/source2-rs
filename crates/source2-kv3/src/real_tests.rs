//! Rewrites every binary KV3 document found in a directory of sample files and compares the
//! payloads.
//!
//! Skipped unless `SOURCE2_KV3_SAMPLES` or `SOURCE2_KV3_CORPUS` is set. Point
//! `SOURCE2_KV3_SAMPLES` at a directory: every `*_dir.vpk` under it is walked for compiled
//! resources whose `DATA` block is binary KV3, and every other file that starts with a KV3
//! magic is taken as a block itself.
//!
//! ```text
//! SOURCE2_KV3_SAMPLES=/path/to/samples cargo test -p source2-kv3 --release \
//!     real_ -- --nocapture
//! ```
//!
//! `SOURCE2_KV3_CORPUS` may name a directory of already extracted blocks (`*.bin`) instead,
//! which skips walking the archives. `SOURCE2_KV3_FAST` stores the rewrite uncompressed, which
//! checks payloads quickly but skips the byte-identity count.
//!
//! The VPK and resource walking below exists for these tests only.

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::{Compression, Kind, Value, Version, decode, parse, parse_text};

struct Sample {
    name: String,
    block: Vec<u8>,
}

fn read_u32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn read_u16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(b[at..at + 2].try_into().unwrap())
}

fn cstr(b: &[u8], at: &mut usize) -> String {
    let end = b[*at..].iter().position(|&c| c == 0).unwrap();
    let s = String::from_utf8_lossy(&b[*at..*at + end]).into_owned();
    *at += end + 1;
    s
}

fn is_kv3(block: &[u8]) -> bool {
    block.len() > 4 && Version::from_magic(read_u32(block, 0)).is_some()
}

/// The `DATA` block of a compiled resource, if it holds binary KV3.
fn kv3_block(file: &[u8]) -> Option<Vec<u8>> {
    if file.len() < 16 {
        return None;
    }
    let table = 8 + read_u32(file, 8) as usize;
    let count = read_u32(file, 12) as usize;
    for i in 0..count {
        let at = table + i * 12;
        let tag = file.get(at..at + 4)?;
        if tag != b"DATA" {
            continue;
        }
        let start = at + 4 + read_u32(file, at + 4) as usize;
        let len = read_u32(file, at + 8) as usize;
        let block = file.get(start..start + len)?;
        return is_kv3(block).then(|| block.to_vec());
    }
    None
}

fn samples_from_archive(dir_vpk: &Path) -> Vec<Sample> {
    let dir = std::fs::read(dir_vpk).expect("read the directory vpk");
    assert_eq!(read_u32(&dir, 0), 0x55aa_1234);
    let version = read_u32(&dir, 4);
    let tree_size = read_u32(&dir, 8) as usize;
    let header_len = if version == 2 { 28 } else { 12 };
    let data_start = header_len + tree_size;
    let stem = dir_vpk
        .file_name()
        .unwrap()
        .to_string_lossy()
        .trim_end_matches("_dir.vpk")
        .to_string();
    let mut at = header_len;
    let mut out = Vec::new();
    let mut archives = std::collections::HashMap::new();
    loop {
        let ext = cstr(&dir, &mut at);
        if ext.is_empty() {
            break;
        }
        loop {
            let path = cstr(&dir, &mut at);
            if path.is_empty() {
                break;
            }
            loop {
                let file = cstr(&dir, &mut at);
                if file.is_empty() {
                    break;
                }
                let preload = read_u16(&dir, at + 4) as usize;
                let archive = read_u16(&dir, at + 6);
                let offset = read_u32(&dir, at + 8) as usize;
                let length = read_u32(&dir, at + 12) as usize;
                at += 18;
                let preload_bytes = &dir[at..at + preload];
                at += preload;
                if !ext.ends_with("_c") || length > 2 << 20 {
                    continue;
                }
                if ["vtex_c", "vmesh_c", "vsnd_c", "vmdl_c", "vanim_c"].contains(&&*ext)
                    && length > 512 * 1024
                {
                    continue;
                }
                let mut bytes = preload_bytes.to_vec();
                if archive == 0x7fff {
                    bytes
                        .extend_from_slice(&dir[data_start + offset..data_start + offset + length]);
                } else {
                    let file = archives.entry(archive).or_insert_with(|| {
                        let pak = dir_vpk.with_file_name(format!("{stem}_{archive:03}.vpk"));
                        std::fs::File::open(pak).expect("archive")
                    });
                    file.seek(SeekFrom::Start(offset as u64)).expect("seek");
                    let mut data = vec![0; length];
                    file.read_exact(&mut data).expect("read");
                    bytes.extend_from_slice(&data);
                }
                if let Some(block) = kv3_block(&bytes) {
                    let dir_name = if path == " " {
                        String::new()
                    } else {
                        format!("{path}/")
                    };
                    out.push(Sample {
                        name: format!("{stem}:{dir_name}{file}.{ext}"),
                        block,
                    });
                }
            }
        }
    }
    out
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, found);
        } else {
            found.push(path);
        }
    }
}

fn samples_from_tree(root: &Path) -> Vec<Sample> {
    let mut files = Vec::new();
    walk(root, &mut files);
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let name = path.to_string_lossy().into_owned();
        if name.ends_with("_dir.vpk") {
            out.extend(samples_from_archive(&path));
        } else if !name.ends_with(".vpk")
            && std::fs::metadata(&path).is_ok_and(|m| m.len() < 64 << 20)
            && let Ok(block) = std::fs::read(&path)
            && is_kv3(&block)
        {
            out.push(Sample { name, block });
        }
    }
    out
}

fn samples_from_corpus(dir: &Path) -> Vec<Sample> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "bin"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|p| Sample {
            name: p.file_name().unwrap().to_string_lossy().into_owned(),
            block: std::fs::read(&p).unwrap(),
        })
        .collect()
}

fn samples() -> Option<Vec<Sample>> {
    if let Ok(dir) = std::env::var("SOURCE2_KV3_CORPUS") {
        return Some(samples_from_corpus(Path::new(&dir)));
    }
    let root = std::env::var("SOURCE2_KV3_SAMPLES").ok()?;
    Some(samples_from_tree(Path::new(&root)))
}

fn first_difference(a: &[u8], b: &[u8]) -> Option<usize> {
    let n = a.len().min(b.len());
    (0..n)
        .find(|&i| a[i] != b[i])
        .or((a.len() != b.len()).then_some(n))
}

/// Whether two decodes of the same document differ only in the LZ4 blob chunk-size table at the
/// end of buffer 2. That table records what the compressor produced for each chunk, not how the
/// document is laid out, so it follows the compressor rather than the writer.
fn only_chunk_table_differs(a: &crate::Decoded, b: &crate::Decoded) -> bool {
    if a.header.compression != Compression::Lz4 || a.blobs != b.blobs {
        return false;
    }
    let chunk = usize::from(a.header.frame_size).max(1);
    let table = 2 * a
        .blobs
        .iter()
        .map(|x| x.len().div_ceil(chunk))
        .sum::<usize>();
    a.payload.len() == b.payload.len()
        && first_difference(&a.payload, &b.payload)
            .is_some_and(|at| at >= a.payload.len().saturating_sub(table))
}

/// Counts per revision and compression.
#[derive(Default)]
struct Tally {
    documents: usize,
    payload_mismatch: usize,
    /// Payloads that differ only in the LZ4 blob chunk-size table.
    chunk_table: usize,
    blob_mismatch: usize,
    tree_mismatch: usize,
    block_identical: usize,
    /// Blocks whose bytes up to the payload match, which for the old revisions is the whole
    /// header and so shows the derived sizes and counts are right even where the compressed
    /// bytes differ.
    head_identical: usize,
    text_unwritable: usize,
    text_mismatch: usize,
}

/// Equality that treats two NaNs with the same bits as equal, which `==` on a tree does not.
fn same_tree(a: &Value, b: &Value) -> bool {
    if a.flags() != b.flags() {
        return false;
    }
    match (a.kind(), b.kind()) {
        (Kind::Double(x), Kind::Double(y)) => x.to_bits() == y.to_bits(),
        (Kind::Array(x), Kind::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_tree(p, q))
        }
        (Kind::Object(x), Kind::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y.iter())
                    .all(|((k, p), (l, q))| k == l && same_tree(p, q))
        }
        (x, y) => x == y,
    }
}

fn has_nan(v: &Value) -> bool {
    match v.kind() {
        Kind::Double(d) => d.is_nan(),
        Kind::Array(items) => items.iter().any(has_nan),
        Kind::Object(o) => o.values().any(has_nan),
        _ => false,
    }
}

#[test]
fn real_documents_rewrite_to_identical_payloads() {
    let Some(samples) = samples() else {
        eprintln!("skipped: set SOURCE2_KV3_SAMPLES to a directory of VPKs or blocks");
        return;
    };
    assert!(!samples.is_empty(), "no binary KV3 documents found");
    let fast = std::env::var("SOURCE2_KV3_FAST").is_ok();

    let mut tallies: BTreeMap<(Version, String), Tally> = BTreeMap::new();
    let mut skipped = 0;
    let mut shown = 0;
    for s in &samples {
        let (Ok(original), Ok(doc)) = (decode(&s.block), parse(&s.block)) else {
            skipped += 1;
            continue;
        };
        let mut doc = doc;
        let key = (
            doc.options.version,
            format!("{:?}", doc.options.compression),
        );
        let t = tallies.entry(key).or_default();
        t.documents += 1;

        if fast && original.header.blob_count == 0 && doc.options.version != Version::Legacy {
            doc.options.compression = Compression::None;
        }
        let rewritten = doc.to_bytes().expect("write");
        let again = decode(&rewritten).expect("rewritten block decodes");
        if again.payload != original.payload && only_chunk_table_differs(&original, &again) {
            t.chunk_table += 1;
        } else if again.payload != original.payload {
            t.payload_mismatch += 1;
            if shown < 15 {
                shown += 1;
                println!(
                    "{}: payload {} vs {} bytes, first difference at {:?}",
                    s.name,
                    original.payload.len(),
                    again.payload.len(),
                    first_difference(&original.payload, &again.payload)
                );
            }
        }
        if again.blobs != original.blobs {
            t.blob_mismatch += 1;
        }
        if !same_tree(
            &parse(&rewritten).expect("rewritten block parses").root,
            &doc.root,
        ) {
            t.tree_mismatch += 1;
        }
        if rewritten == s.block {
            t.block_identical += 1;
        } else if std::env::var("SOURCE2_KV3_VERBOSE").is_ok()
            && doc.options.compression != Compression::Block
        {
            println!(
                "{}: block differs, {} vs {} bytes, first difference at {:?}",
                s.name,
                s.block.len(),
                rewritten.len(),
                first_difference(&s.block, &rewritten)
            );
        }
        let head = original.header.payload_offset;
        if rewritten.get(..head) == s.block.get(..head) {
            t.head_identical += 1;
        } else if std::env::var("SOURCE2_KV3_VERBOSE").is_ok() {
            println!(
                "{}: header differs
{:02x?}
{:02x?}",
                s.name,
                &s.block[..head],
                &rewritten[..head.min(rewritten.len())]
            );
        }

        if !has_nan(&doc.root) {
            match doc.to_text() {
                Err(_) => t.text_unwritable += 1,
                Ok(text) => {
                    if parse_text(&text).map(|d| d.root).ok().as_ref() != Some(&doc.root) {
                        t.text_mismatch += 1;
                    }
                }
            }
        }
    }

    println!("documents {}, skipped {skipped}", samples.len());
    let mut bad = 0;
    for ((version, compression), t) in &tallies {
        println!(
            "{version:?}/{compression}: {} docs, payload mismatches {} (+{} chunk table), \
             blob mismatches {}, tree mismatches {}, block-identical {}, header-identical {}, \
             text unwritable {}, text mismatches {}",
            t.documents,
            t.payload_mismatch,
            t.chunk_table,
            t.blob_mismatch,
            t.tree_mismatch,
            t.block_identical,
            t.head_identical,
            t.text_unwritable,
            t.text_mismatch
        );
        bad += t.payload_mismatch + t.blob_mismatch + t.tree_mismatch + t.text_mismatch;
    }
    assert_eq!(bad, 0);
}
