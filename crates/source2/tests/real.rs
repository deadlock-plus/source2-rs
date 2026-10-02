//! Reads real packs read-only and edits in memory. Set `SOURCE2_VPK_SAMPLES` to a directory
//! holding `*_dir.vpk` files (searched two levels deep). Unset, the test passes without
//! checking anything. Nothing is ever written to disk.
#![cfg(all(feature = "vpk", feature = "resource", feature = "kv3"))]
#![allow(missing_docs)]

use std::path::{Path, PathBuf};

use source2::ext::{ResourceKv3, VpkResource, VpkResourceKv3};
use source2::resource::{BlockKind, Resource};
use source2::vpk::Vpk;
use source2::{Format, detect, kv3};

const MAX_PACKS: usize = 4;
const MAX_RESOURCES_PER_PACK: usize = 900;
const MAX_EDITS_PER_PACK: usize = 150;
const MAX_ENTRY_BYTES: usize = 8 << 20;
const MARKER: &str = "source2_facade_test_marker";

fn find_packs(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            if depth > 0 {
                find_packs(&path, depth - 1, out);
            }
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with("_dir.vpk"))
        {
            out.push(path);
        }
    }
}

fn payload(block: &[u8]) -> Vec<u8> {
    kv3::decode(block).expect("decode").payload
}

fn check_one(pack: &mut Vpk, path: &str) -> bool {
    let original = Resource::parse(&pack.read_path(path).unwrap()).unwrap();
    let Some(data) = original.block(BlockKind::DATA) else {
        return false;
    };
    if detect(&data.data) != Format::Kv3Binary {
        return false;
    }
    let data_payload = payload(&data.data);

    let mut doc = pack.read_resource_kv3(path, BlockKind::DATA).unwrap();

    let same_apart_from_data = |other: &Resource| {
        assert_eq!(other.versions, original.versions, "{path}");
        assert_eq!(other.pre_table, original.pre_table, "{path}");
        assert_eq!(other.trailing, original.trailing, "{path}");
        assert_eq!(other.blocks.len(), original.blocks.len(), "{path}");
        for (new, old) in other.blocks.iter().zip(&original.blocks) {
            assert_eq!((new.kind, &new.padding), (old.kind, &old.padding), "{path}");
            if new.kind != BlockKind::DATA {
                assert_eq!(new.data, old.data, "{path}: block {}", new.kind);
            }
        }
    };

    pack.put_resource_kv3(path, BlockKind::DATA, &doc).unwrap();
    let rewritten = pack.read_resource(path).unwrap();
    same_apart_from_data(&rewritten);
    assert_eq!(
        payload(&rewritten.block(BlockKind::DATA).unwrap().data),
        data_payload,
        "{path}: kv3 payload changed on put-back"
    );

    if let Some(root) = doc.root.as_object_mut() {
        let members = root.len();
        root.insert(MARKER, true);
        pack.put_resource_kv3(path, BlockKind::DATA, &doc).unwrap();
        let edited = pack.read_resource(path).unwrap();
        same_apart_from_data(&edited);
        let back = edited.read_kv3(BlockKind::DATA).unwrap();
        let back_root = back.root.as_object().unwrap();
        assert_eq!(back_root.len(), members + 1, "{path}");
        assert!(back_root.contains_key(MARKER), "{path}");
    }
    true
}

#[test]
fn real_packs_edit_through_the_helpers() {
    let Some(dir) = std::env::var_os("SOURCE2_VPK_SAMPLES") else {
        eprintln!("skipped: SOURCE2_VPK_SAMPLES is not set");
        return;
    };
    let mut packs = Vec::new();
    find_packs(Path::new(&dir), 2, &mut packs);
    if packs.is_empty() {
        eprintln!("skipped: no *_dir.vpk under {}", Path::new(&dir).display());
        return;
    }

    let mut checked = 0;
    let mut examined = 0;
    for dir_file in packs
        .iter()
        .step_by((packs.len() / MAX_PACKS).max(1))
        .take(MAX_PACKS)
    {
        let mut pack = Vpk::open(dir_file).expect("open");
        let candidates: Vec<String> = pack
            .entries()
            .iter()
            .filter(|e| e.path.ends_with("_c") && e.size() <= MAX_ENTRY_BYTES)
            .map(|e| e.path.clone())
            .collect();
        let step = (candidates.len() / MAX_RESOURCES_PER_PACK).max(1);
        let mut edited = 0;
        for path in candidates.iter().step_by(step).take(MAX_RESOURCES_PER_PACK) {
            examined += 1;
            if check_one(&mut pack, path) {
                edited += 1;
                if edited == MAX_EDITS_PER_PACK {
                    break;
                }
            }
        }
        eprintln!("{}: {edited} edited", dir_file.display());
        checked += edited;
    }
    eprintln!("examined {examined} resources, edited {checked} with a kv3 DATA block");
}
