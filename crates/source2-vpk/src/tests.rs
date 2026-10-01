//! Reader tests against VPKs assembled by hand in this file.
//!
//! The bytes here are never produced by the writer, so a shared mistake in layout cannot
//! make the reader and writer agree on something wrong.

use super::*;
use std::path::{Path, PathBuf};

/// Files of one extension, grouped by their directory: `(directory, [(stem, index)])`.
type DirGroups = Vec<(String, Vec<(String, usize)>)>;

/// Builds a VPK v2 directory file plus one numbered archive.
#[derive(Default)]
pub(crate) struct Builder {
    /// `(path, bytes, preload_len, inline)`
    files: Vec<(String, Vec<u8>, usize, bool)>,
}

impl Builder {
    pub(crate) fn add(mut self, path: &str, body: &[u8]) -> Self {
        self.files.push((path.into(), body.to_vec(), 0, false));
        self
    }

    /// A file with its first `preload` bytes stored in the directory file.
    pub(crate) fn add_with_preload(mut self, path: &str, body: &[u8], preload: usize) -> Self {
        self.files
            .push((path.into(), body.to_vec(), preload, false));
        self
    }

    /// A file stored entirely in the directory file's own data section.
    pub(crate) fn add_inline(mut self, path: &str, body: &[u8]) -> Self {
        self.files.push((path.into(), body.to_vec(), 0, true));
        self
    }

    /// Returns the directory bytes and archive 000's bytes.
    pub(crate) fn build(self) -> (Vec<u8>, Vec<u8>) {
        let mut grouped: Vec<(String, DirGroups)> = Vec::new();
        for (i, (path, _, _, _)) in self.files.iter().enumerate() {
            let (dir, file) = match path.rfind('/') {
                Some(n) => (path[..n].to_string(), path[n + 1..].to_string()),
                None => (" ".to_string(), path.clone()),
            };
            let (stem, ext) = match file.rfind('.') {
                Some(n) => (file[..n].to_string(), file[n + 1..].to_string()),
                None => (file.clone(), " ".to_string()),
            };
            let e = match grouped.iter_mut().find(|(x, _)| *x == ext) {
                Some(e) => e,
                None => {
                    grouped.push((ext, Vec::new()));
                    grouped.last_mut().unwrap()
                }
            };
            match e.1.iter_mut().find(|(d, _)| *d == dir) {
                Some(d) => d.1.push((stem, i)),
                None => e.1.push((dir, vec![(stem, i)])),
            }
        }

        let mut archive = Vec::new();
        let mut inline = Vec::new();
        let mut tree = Vec::new();

        for (ext, dirs) in &grouped {
            push_cstr(&mut tree, ext);
            for (dir, files) in dirs {
                push_cstr(&mut tree, dir);
                for (stem, idx) in files {
                    let (_, body, preload_len, is_inline) = &self.files[*idx];
                    let preload_len = (*preload_len).min(body.len());
                    let (preload, rest) = body.split_at(preload_len);

                    let target = if *is_inline {
                        &mut inline
                    } else {
                        &mut archive
                    };
                    let offset = target.len() as u32;
                    target.extend_from_slice(rest);

                    push_cstr(&mut tree, stem);
                    tree.extend_from_slice(&crc32(body).to_le_bytes());
                    tree.extend_from_slice(&(preload_len as u16).to_le_bytes());
                    tree.extend_from_slice(
                        &if *is_inline { ARCHIVE_INLINE } else { 0u16 }.to_le_bytes(),
                    );
                    tree.extend_from_slice(&offset.to_le_bytes());
                    tree.extend_from_slice(&(rest.len() as u32).to_le_bytes());
                    tree.extend_from_slice(&ENTRY_TERMINATOR.to_le_bytes());
                    tree.extend_from_slice(preload);
                }
                tree.push(0);
            }
            tree.push(0);
        }
        tree.push(0);

        let mut dir = Vec::new();
        dir.extend_from_slice(&SIGNATURE.to_le_bytes());
        dir.extend_from_slice(&2u32.to_le_bytes());
        dir.extend_from_slice(&(tree.len() as u32).to_le_bytes());
        dir.extend_from_slice(&(inline.len() as u32).to_le_bytes());
        for _ in 0..3 {
            dir.extend_from_slice(&0u32.to_le_bytes());
        }
        dir.extend_from_slice(&tree);
        dir.extend_from_slice(&inline);
        (dir, archive)
    }

    /// Write the pair into a temp directory and open it.
    pub(crate) fn open_in(self, dir: &Path) -> Result<Vpk> {
        let (dir_bytes, archive) = self.build();
        std::fs::create_dir_all(dir).unwrap();
        let dir_path = dir.join("pack_dir.vpk");
        std::fs::write(&dir_path, &dir_bytes).unwrap();
        std::fs::write(dir.join("pack_000.vpk"), &archive).unwrap();
        Vpk::open(&dir_path)
    }
}

pub(crate) fn push_cstr(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
    out.push(0);
}

/// A scratch directory that cleans up after itself.
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    pub(crate) fn new(tag: &str) -> Self {
        let mut p = std::env::temp_dir();
        p.push(format!("source2-vpk-test-{tag}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stored(e: &Entry) -> (Option<u16>, u32, u32) {
    match e.data {
        Data::Stored {
            archive,
            offset,
            length,
        } => (archive, offset, length),
        Data::Memory { .. } => panic!("{} is not a parsed entry", e.path),
    }
}

#[test]
fn crc32_matches_known_vectors() {
    assert_eq!(crc32(b""), 0x0000_0000);
    assert_eq!(crc32(b"a"), 0xE8B7_BE43);
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(
        crc32(b"The quick brown fox jumps over the lazy dog"),
        0x414F_A339
    );
    assert_eq!(crc32_update(crc32(b"1234"), b"56789"), 0xCBF4_3926);
}

#[test]
fn reads_a_file_out_of_a_numbered_archive() {
    let t = TempDir::new("basic");
    let vpk = Builder::default()
        .add("docs/first.txt", b"first payload")
        .add("docs/second.txt", b"second payload")
        .open_in(t.path())
        .expect("open");

    assert_eq!(vpk.version, 2);
    assert_eq!(vpk.len(), 2);
    assert_eq!(vpk.read_path("docs/first.txt").unwrap(), b"first payload");
    assert_eq!(vpk.read_path("docs/second.txt").unwrap(), b"second payload");
}

#[test]
fn reads_a_file_stored_entirely_in_the_preload() {
    let t = TempDir::new("preload");
    let body = b"entirely preloaded";
    let vpk = Builder::default()
        .add_with_preload("docs/small.txt", body, body.len())
        .open_in(t.path())
        .expect("open");

    let e = vpk.find("docs/small.txt").expect("entry");
    assert_eq!(stored(e).2, 0, "nothing should be in the archive");
    assert_eq!(e.preload.len(), body.len());
    assert_eq!(vpk.read(e).unwrap(), body);
}

#[test]
fn reads_a_file_split_between_preload_and_archive() {
    let t = TempDir::new("split");
    let body: Vec<u8> = (0u8..200).collect();
    let vpk = Builder::default()
        .add_with_preload("a/b.bin", &body, 64)
        .open_in(t.path())
        .expect("open");

    let e = vpk.find("a/b.bin").expect("entry");
    assert_eq!(e.preload.len(), 64);
    assert_eq!(stored(e).2, 136);
    assert_eq!(e.size(), 200);
    assert_eq!(vpk.read(e).unwrap(), body);
}

/// `archive_index == 0x7fff` means the bytes sit in the directory file's own data
/// section, offset from the end of the tree rather than the start of the file.
#[test]
fn reads_a_file_inline_in_the_directory() {
    let t = TempDir::new("inline");
    let vpk = Builder::default()
        .add("other.bin", b"in the archive")
        .add_inline("docs/inline.txt", b"in the directory file")
        .open_in(t.path())
        .expect("open");

    let e = vpk.find("docs/inline.txt").expect("entry");
    assert_eq!(stored(e).0, None);
    assert_eq!(vpk.read(e).unwrap(), b"in the directory file");
    assert_eq!(vpk.read_path("other.bin").unwrap(), b"in the archive");
}

/// Inline bytes come from the parsed directory itself: no file beside it is consulted.
#[test]
fn inline_entries_read_without_any_source_file() {
    let (dir, _) = Builder::default()
        .add_inline("docs/inline.txt", b"in the directory file")
        .build();
    let vpk = Vpk::parse(&dir).unwrap();
    assert_eq!(
        vpk.read_path("docs/inline.txt").unwrap(),
        b"in the directory file"
    );
}

/// Valve writes a single space for the archive root; it must not become a `" /"` prefix.
#[test]
fn root_directory_entries_have_no_leading_separator() {
    let t = TempDir::new("root");
    let vpk = Builder::default()
        .add("toplevel.txt", b"x")
        .add("README", b"y")
        .open_in(t.path())
        .expect("open");
    assert_eq!(vpk.entries()[0].path, "toplevel.txt");
    assert_eq!(vpk.entries()[1].path, "README");
    assert!(vpk.find("README").is_some());
}

#[test]
fn finds_entries_by_extension() {
    let t = TempDir::new("byext");
    let vpk = Builder::default()
        .add("docs/a.cfg", b"1")
        .add("docs/b.cfg", b"2")
        .add("docs/c.txt", b"3")
        .open_in(t.path())
        .expect("open");

    let mut found: Vec<&str> = vpk
        .find_by_extension("cfg")
        .map(|e| e.path.as_str())
        .collect();
    found.sort_unstable();
    assert_eq!(found, ["docs/a.cfg", "docs/b.cfg"]);
    assert_eq!(vpk.find_by_extension(".txt").count(), 1);
    assert_eq!(vpk.find_by_extension("cf").count(), 0);
}

/// A name with no extension has an empty extension, not itself as the extension, and a
/// dotted directory is not an extension either.
#[test]
fn extension_lookup_only_looks_at_the_file_name() {
    let t = TempDir::new("noext");
    let vpk = Builder::default()
        .add("docs/notes", b"1")
        .add("v1.2/readme", b"2")
        .add("docs/a.txt", b"3")
        .open_in(t.path())
        .expect("open");

    assert_eq!(vpk.find_by_extension("notes").count(), 0);
    assert_eq!(vpk.find_by_extension("2/readme").count(), 0);
    assert_eq!(vpk.find_by_extension("2").count(), 0);
    let mut none: Vec<&str> = vpk.find_by_extension("").map(|e| e.path.as_str()).collect();
    none.sort_unstable();
    assert_eq!(none, ["docs/notes", "v1.2/readme"]);
}

/// A prefix names a directory, so everything below it comes back however deep it sits.
#[test]
fn lists_every_entry_below_a_directory_prefix_at_any_depth() {
    let t = TempDir::new("under");
    let vpk = Builder::default()
        .add("docs/first.txt", b"1")
        .add("docs/second.txt", b"2")
        .add("docs/gen/third.txt", b"3")
        .add("docs/gen/deep/leaf.txt", b"4")
        .add("other/layout.xml", b"5")
        .add("toplevel.txt", b"6")
        .open_in(t.path())
        .expect("open");

    let mut found: Vec<&str> = vpk.entries_under("docs").map(|e| e.path.as_str()).collect();
    found.sort_unstable();
    assert_eq!(
        found,
        [
            "docs/first.txt",
            "docs/gen/deep/leaf.txt",
            "docs/gen/third.txt",
            "docs/second.txt",
        ]
    );

    assert_eq!(vpk.entries_under("docs/").count(), 4);
    let mut nested: Vec<&str> = vpk
        .entries_under("docs/gen")
        .map(|e| e.path.as_str())
        .collect();
    nested.sort_unstable();
    assert_eq!(nested, ["docs/gen/deep/leaf.txt", "docs/gen/third.txt"]);
    assert_eq!(vpk.entries_under("").count(), vpk.len());
}

/// The match is on a path boundary: `docs` must not drag in `docs_old`, which a bare
/// `starts_with` would.
#[test]
fn a_directory_prefix_does_not_match_a_sibling_whose_name_extends_it() {
    let t = TempDir::new("sibling");
    let vpk = Builder::default()
        .add("docs/first.txt", b"1")
        .add("docs_old/first.txt", b"2")
        .add("docsx.txt", b"3")
        .open_in(t.path())
        .expect("open");

    let found: Vec<&str> = vpk.entries_under("docs").map(|e| e.path.as_str()).collect();
    assert_eq!(found, ["docs/first.txt"]);
    assert_eq!(vpk.entries_under("docs_old").count(), 1);
}

#[test]
fn a_prefix_no_entry_lives_below_yields_nothing() {
    let t = TempDir::new("nomatch");
    let vpk = Builder::default()
        .add("docs/first.txt", b"1")
        .open_in(t.path())
        .expect("open");

    assert_eq!(vpk.entries_under("other").count(), 0);
    assert_eq!(vpk.entries_under("docs/first.txt").count(), 0);
}

#[test]
fn corrupted_bytes_are_caught_by_the_crc() {
    let t = TempDir::new("crc");
    let vpk = Builder::default()
        .add("docs/x.txt", b"original payload")
        .open_in(t.path())
        .expect("open");

    std::fs::write(t.path().join("pack_000.vpk"), b"tampered payload!").unwrap();
    match vpk.read_path("docs/x.txt") {
        Err(Error::ChecksumMismatch { path, .. }) => assert_eq!(path, "docs/x.txt"),
        other => panic!("expected a checksum mismatch, got {other:?}"),
    }
}

/// A bad CRC is reportable: the bytes come back along with the verdict.
#[test]
fn read_with_crc_returns_the_bytes_and_the_verdict() {
    let t = TempDir::new("crc-status");
    let vpk = Builder::default()
        .add("docs/x.txt", b"original payload")
        .open_in(t.path())
        .expect("open");

    let good = vpk.read_with_crc(vpk.find("docs/x.txt").unwrap()).unwrap();
    assert!(good.crc_ok());
    assert_eq!(good.bytes, b"original payload");

    std::fs::write(t.path().join("pack_000.vpk"), b"tampered payload!").unwrap();
    let bad = vpk.read_with_crc(vpk.find("docs/x.txt").unwrap()).unwrap();
    assert!(!bad.crc_ok());
    assert_eq!(bad.bytes, b"tampered payload");
    assert_eq!(bad.expected_crc, crc32(b"original payload"));
    assert_eq!(bad.actual_crc, crc32(b"tampered payload"));
}

#[test]
fn rejects_a_file_that_is_not_a_vpk() {
    let err = Vpk::parse(b"not a vpk at all").unwrap_err();
    assert!(matches!(err, Error::NotAVpk { .. }), "{err:?}");
}

#[test]
fn rejects_an_unsupported_version() {
    for version in [0u32, 3, 99] {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&SIGNATURE.to_le_bytes());
        bytes.extend_from_slice(&version.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        match Vpk::parse(&bytes) {
            Err(Error::UnsupportedVersion(v)) => assert_eq!(v, version),
            other => panic!("expected UnsupportedVersion({version}), got {other:?}"),
        }
    }
}

/// A tree that stops mid-entry must be an error, not a panic or a silent truncation.
#[test]
fn a_truncated_tree_is_an_error_not_a_panic() {
    let (mut dir, _) = Builder::default().add("docs/first.txt", b"payload").build();
    dir.truncate(dir.len() - 12);
    let err = Vpk::parse(&dir).unwrap_err();
    assert!(matches!(err, Error::Malformed(_)), "{err:?}");
}

#[test]
fn missing_paths_report_rather_than_panic() {
    let t = TempDir::new("missing");
    let vpk = Builder::default()
        .add("docs/x.txt", b"x")
        .open_in(t.path())
        .expect("open");
    assert!(vpk.find("docs/nope.txt").is_none());
    assert!(matches!(
        vpk.read_path("docs/nope.txt"),
        Err(Error::NotFound(p)) if p == "docs/nope.txt"
    ));
}

#[test]
fn an_empty_archive_parses_to_nothing() {
    let t = TempDir::new("empty");
    let vpk = Builder::default().open_in(t.path()).expect("open");
    assert!(vpk.is_empty());
    assert_eq!(vpk.len(), 0);
}

fn dir_with_name(name: &[u8]) -> Vec<u8> {
    let mut tree = Vec::new();
    tree.extend_from_slice(b"txt\0 \0");
    tree.extend_from_slice(name);
    tree.push(0);
    tree.extend_from_slice(&[0; 4]);
    tree.extend_from_slice(&[0; 2]);
    tree.extend_from_slice(&ARCHIVE_INLINE.to_le_bytes());
    tree.extend_from_slice(&[0; 8]);
    tree.extend_from_slice(&ENTRY_TERMINATOR.to_le_bytes());
    tree.extend_from_slice(&[0, 0, 0]);
    let mut dir = Vec::new();
    dir.extend_from_slice(&SIGNATURE.to_le_bytes());
    dir.extend_from_slice(&1u32.to_le_bytes());
    dir.extend_from_slice(&(tree.len() as u32).to_le_bytes());
    dir.extend_from_slice(&tree);
    dir
}

#[test]
fn non_utf8_names_are_an_error_by_default() {
    let dir = dir_with_name(b"a\xff");
    match Vpk::parse(&dir) {
        Err(Error::InvalidName { lossy }) => assert_eq!(lossy, "a\u{fffd}"),
        other => panic!("expected InvalidName, got {other:?}"),
    }
}

#[test]
fn lossy_parsing_is_a_named_opt_in() {
    let dir = dir_with_name(b"a\xff");
    let vpk = Vpk::parse_lossy(&dir).unwrap();
    assert_eq!(vpk.entries()[0].path, "a\u{fffd}.txt");
}

/// The tree is `.txt` with a stem that itself contains a dot: that spelling cannot be
/// written back (the writer would split at the last dot), so it is refused up front.
#[test]
fn a_name_that_would_not_split_back_the_same_is_refused() {
    let dir = dir_with_name(b"a.b");
    // ext is "txt" so the path is "a.b.txt" and splits back to stem "a.b": fine.
    assert!(Vpk::parse(&dir).is_ok());

    let mut tree = Vec::new();
    tree.extend_from_slice(b" \0 \0a.b\0");
    tree.extend_from_slice(&[0; 4]);
    tree.extend_from_slice(&[0; 2]);
    tree.extend_from_slice(&ARCHIVE_INLINE.to_le_bytes());
    tree.extend_from_slice(&[0; 8]);
    tree.extend_from_slice(&ENTRY_TERMINATOR.to_le_bytes());
    tree.extend_from_slice(&[0, 0, 0]);
    let mut dir = Vec::new();
    dir.extend_from_slice(&SIGNATURE.to_le_bytes());
    dir.extend_from_slice(&1u32.to_le_bytes());
    dir.extend_from_slice(&(tree.len() as u32).to_le_bytes());
    dir.extend_from_slice(&tree);
    assert!(matches!(Vpk::parse(&dir), Err(Error::Malformed(_))));
}

/// Bytes between the last entry and the end of the declared tree are not silently
/// skipped.
#[test]
fn a_tree_longer_than_its_entries_is_an_error() {
    let (mut dir, _) = Builder::default().add("a.txt", b"x").build();
    let tree_size = u32::from_le_bytes(dir[8..12].try_into().unwrap());
    dir[8..12].copy_from_slice(&(tree_size + 1).to_le_bytes());
    dir.insert(28 + tree_size as usize, 0);
    assert!(matches!(Vpk::parse(&dir), Err(Error::Malformed(_))));
}

/// There is no default pack name to fall back on: without a `_dir.vpk` file name there
/// are no numbered archives to find.
#[test]
fn numbered_archives_need_a_source_named_dir() {
    let t = TempDir::new("no-fallback");
    let (dir, archive) = Builder::default().add("a.txt", b"x").build();
    std::fs::write(t.path().join("pack_000.vpk"), &archive).unwrap();

    let sourceless = Vpk::parse(&dir).unwrap();
    assert!(matches!(
        sourceless.read_path("a.txt"),
        Err(Error::InvalidInput(_))
    ));

    let odd = Vpk::parse(&dir)
        .unwrap()
        .with_source(t.path().join("weird.vpk"));
    assert!(matches!(
        odd.read_path("a.txt"),
        Err(Error::InvalidInput(_))
    ));

    let ok = Vpk::parse(&dir)
        .unwrap()
        .with_source(t.path().join("pack_dir.vpk"));
    assert_eq!(ok.read_path("a.txt").unwrap(), b"x");
}

#[test]
fn io_errors_keep_the_underlying_error() {
    let t = TempDir::new("io");
    let err = Vpk::open(t.path().join("absent_dir.vpk")).unwrap_err();
    let Error::Io { path, source } = &err else {
        panic!("expected Io, got {err:?}");
    };
    assert!(path.ends_with("absent_dir.vpk"));
    assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
    assert!(std::error::Error::source(&err).is_some());
}

/// A pack that lists a path twice keeps both entries; lookups find the first.
#[test]
fn duplicate_paths_are_kept() {
    let (dir, _) = Builder::default()
        .add_inline("a.txt", b"first")
        .add_inline("a.txt", b"second")
        .build();
    let vpk = Vpk::parse(&dir).unwrap();
    assert_eq!(vpk.len(), 2);
    assert_eq!(vpk.read_path("a.txt").unwrap(), b"first");
    assert_eq!(vpk.read(&vpk.entries()[1]).unwrap(), b"second");
}

#[test]
fn an_inline_range_past_the_data_section_is_an_error() {
    let (dir, _) = Builder::default().add_inline("a.txt", b"abc").build();
    let mut vpk = Vpk::parse(&dir).unwrap();
    vpk.inline_data.truncate(1);
    assert!(matches!(vpk.read_path("a.txt"), Err(Error::Malformed(_))));
}
