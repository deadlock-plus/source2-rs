//! Tests against VPKs built in-process.
//!
//! Synthetic rather than a vendored slice of a shipped archive: a generated fixture can
//! exercise the cases a real archive happens not to contain (inline data, empty preload, a
//! truncated tree).

use super::*;

/// Files of one extension, grouped by their directory: `(directory, [(stem, index)])`.
type DirGroups = Vec<(String, Vec<(String, usize)>)>;

/// Builds a VPK v2 directory file plus one numbered archive.
#[derive(Default)]
struct Builder {
    /// `(path, bytes, preload_len, inline)`
    files: Vec<(String, Vec<u8>, usize, bool)>,
}

impl Builder {
    fn add(mut self, path: &str, body: &[u8]) -> Self {
        self.files.push((path.into(), body.to_vec(), 0, false));
        self
    }

    /// A file with its first `preload` bytes stored in the directory file.
    fn add_with_preload(mut self, path: &str, body: &[u8], preload: usize) -> Self {
        self.files
            .push((path.into(), body.to_vec(), preload, false));
        self
    }

    /// A file stored entirely in the directory file's own data section.
    fn add_inline(mut self, path: &str, body: &[u8]) -> Self {
        self.files.push((path.into(), body.to_vec(), 0, true));
        self
    }

    /// Returns the directory bytes and archive 000's bytes.
    fn build(self) -> (Vec<u8>, Vec<u8>) {
        // Group by extension, then directory, the way the format nests them.
        let mut grouped: Vec<(String, DirGroups)> = Vec::new();
        for (i, (path, _, _, _)) in self.files.iter().enumerate() {
            let (dir, file) = match path.rfind('/') {
                Some(n) => (path[..n].to_string(), path[n + 1..].to_string()),
                None => (" ".to_string(), path.clone()),
            };
            let (stem, ext) = match file.rfind('.') {
                Some(n) => (file[..n].to_string(), file[n + 1..].to_string()),
                None => (file.clone(), String::new()),
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
                tree.push(0); // end of filenames
            }
            tree.push(0); // end of directories
        }
        tree.push(0); // end of extensions

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
    fn open_in(self, dir: &Path) -> Result<Vpk> {
        let (dir_bytes, archive) = self.build();
        std::fs::create_dir_all(dir).unwrap();
        let dir_path = dir.join("pak01_dir.vpk");
        std::fs::write(&dir_path, &dir_bytes).unwrap();
        std::fs::write(dir.join("pak01_000.vpk"), &archive).unwrap();
        Vpk::open(&dir_path)
    }
}

fn push_cstr(out: &mut Vec<u8>, s: &str) {
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

#[test]
fn crc32_matches_known_vectors() {
    assert_eq!(crc32(b""), 0x0000_0000);
    assert_eq!(crc32(b"a"), 0xE8B7_BE43);
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(
        crc32(b"The quick brown fox jumps over the lazy dog"),
        0x414F_A339
    );
}

#[test]
fn reads_a_file_out_of_a_numbered_archive() {
    let t = TempDir::new("basic");
    let vpk = Builder::default()
        .add("scripts/heroes.vdata_c", b"hero payload")
        .add("scripts/abilities.vdata_c", b"ability payload")
        .open_in(t.path())
        .expect("open");

    assert_eq!(vpk.version(), 2);
    assert_eq!(vpk.len(), 2);
    assert_eq!(
        vpk.read_path("scripts/heroes.vdata_c").unwrap(),
        b"hero payload"
    );
    assert_eq!(
        vpk.read_path("scripts/abilities.vdata_c").unwrap(),
        b"ability payload"
    );
}

/// Small files are stored wholly in the directory file, with `length` zero. A reader that
/// only ever looked in the numbered archives would return empty for these.
#[test]
fn reads_a_file_stored_entirely_in_the_preload() {
    let t = TempDir::new("preload");
    let body = b"entirely preloaded";
    let vpk = Builder::default()
        .add_with_preload("scripts/small.vdata_c", body, body.len())
        .open_in(t.path())
        .expect("open");

    let e = vpk.find("scripts/small.vdata_c").expect("entry");
    assert_eq!(e.length, 0, "nothing should be in the archive");
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
    assert_eq!(e.length, 136);
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
        .add_inline("scripts/inline.vdata_c", b"in the directory file")
        .open_in(t.path())
        .expect("open");

    let e = vpk.find("scripts/inline.vdata_c").expect("entry");
    assert_eq!(e.archive, None);
    assert_eq!(vpk.read(e).unwrap(), b"in the directory file");
    assert_eq!(vpk.read_path("other.bin").unwrap(), b"in the archive");
}

/// Valve writes a single space for the archive root; it must not become a `" /"` prefix.
#[test]
fn root_directory_entries_have_no_leading_separator() {
    let t = TempDir::new("root");
    let vpk = Builder::default()
        .add("toplevel.vdata_c", b"x")
        .open_in(t.path())
        .expect("open");
    assert_eq!(vpk.entries()[0].path, "toplevel.vdata_c");
    assert!(vpk.find("toplevel.vdata_c").is_some());
}

#[test]
fn finds_entries_by_extension() {
    let t = TempDir::new("byext");
    let vpk = Builder::default()
        .add("scripts/a.vdata_c", b"1")
        .add("scripts/b.vdata_c", b"2")
        .add("scripts/c.txt", b"3")
        .open_in(t.path())
        .expect("open");

    let mut found: Vec<&str> = vpk
        .find_by_extension("vdata_c")
        .map(|e| e.path.as_str())
        .collect();
    found.sort_unstable();
    assert_eq!(found, ["scripts/a.vdata_c", "scripts/b.vdata_c"]);
    assert_eq!(vpk.find_by_extension(".txt").count(), 1);
    assert_eq!(vpk.find_by_extension("vdata").count(), 0);
}

/// A prefix names a directory, so everything below it comes back however deep it sits.
#[test]
fn lists_every_entry_below_a_directory_prefix_at_any_depth() {
    let t = TempDir::new("under");
    let vpk = Builder::default()
        .add("scripts/heroes.vdata_c", b"1")
        .add("scripts/abilities.vdata_c", b"2")
        .add("scripts/gen/subclasses.vdata_c", b"3")
        .add("scripts/gen/deep/leaf.vdata_c", b"4")
        .add("panorama/layout.xml", b"5")
        .add("toplevel.vdata_c", b"6")
        .open_in(t.path())
        .expect("open");

    let mut found: Vec<&str> = vpk
        .entries_under("scripts")
        .map(|e| e.path.as_str())
        .collect();
    found.sort_unstable();
    assert_eq!(
        found,
        [
            "scripts/abilities.vdata_c",
            "scripts/gen/deep/leaf.vdata_c",
            "scripts/gen/subclasses.vdata_c",
            "scripts/heroes.vdata_c",
        ]
    );

    assert_eq!(vpk.entries_under("scripts/").count(), 4);
    let mut nested: Vec<&str> = vpk
        .entries_under("scripts/gen")
        .map(|e| e.path.as_str())
        .collect();
    nested.sort_unstable();
    assert_eq!(
        nested,
        [
            "scripts/gen/deep/leaf.vdata_c",
            "scripts/gen/subclasses.vdata_c"
        ]
    );
    assert_eq!(vpk.entries_under("").count(), vpk.len());
}

/// The match is on a path boundary: `scripts` must not drag in `scripts_old`, which a
/// bare `starts_with` would.
#[test]
fn a_directory_prefix_does_not_match_a_sibling_whose_name_extends_it() {
    let t = TempDir::new("sibling");
    let vpk = Builder::default()
        .add("scripts/heroes.vdata_c", b"1")
        .add("scripts_old/heroes.vdata_c", b"2")
        .add("scriptsx.vdata_c", b"3")
        .open_in(t.path())
        .expect("open");

    let found: Vec<&str> = vpk
        .entries_under("scripts")
        .map(|e| e.path.as_str())
        .collect();
    assert_eq!(found, ["scripts/heroes.vdata_c"]);
    assert_eq!(vpk.entries_under("scripts_old").count(), 1);
}

#[test]
fn a_prefix_no_entry_lives_below_yields_nothing() {
    let t = TempDir::new("nomatch");
    let vpk = Builder::default()
        .add("scripts/heroes.vdata_c", b"1")
        .open_in(t.path())
        .expect("open");

    assert_eq!(vpk.entries_under("materials").count(), 0);
    assert_eq!(vpk.entries_under("scripts/heroes.vdata_c").count(), 0);
}

#[test]
fn corrupted_bytes_are_caught_by_the_crc() {
    let t = TempDir::new("crc");
    let vpk = Builder::default()
        .add("scripts/x.vdata_c", b"original payload")
        .open_in(t.path())
        .expect("open");

    std::fs::write(t.path().join("pak01_000.vpk"), b"tampered payload!").unwrap();
    match vpk.read_path("scripts/x.vdata_c") {
        Err(Error::ChecksumMismatch { path, .. }) => assert_eq!(path, "scripts/x.vdata_c"),
        other => panic!("expected a checksum mismatch, got {other:?}"),
    }
}

#[test]
fn rejects_a_file_that_is_not_a_vpk() {
    let err = Vpk::parse(b"not a vpk at all", "pak01_dir.vpk").unwrap_err();
    assert!(matches!(err, Error::NotAVpk { .. }), "{err:?}");
}

#[test]
fn rejects_an_unsupported_version() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&SIGNATURE.to_le_bytes());
    bytes.extend_from_slice(&99u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    match Vpk::parse(&bytes, "pak01_dir.vpk") {
        Err(Error::UnsupportedVersion(99)) => {}
        other => panic!("expected UnsupportedVersion(99), got {other:?}"),
    }
}

/// A tree that stops mid-entry must be an error, not a panic or a silent truncation.
#[test]
fn a_truncated_tree_is_an_error_not_a_panic() {
    let t = TempDir::new("trunc");
    let (mut dir, _) = Builder::default()
        .add("scripts/heroes.vdata_c", b"payload")
        .build();
    dir.truncate(dir.len() - 12);
    let err = Vpk::parse(&dir, t.path().join("pak01_dir.vpk")).unwrap_err();
    assert!(matches!(err, Error::Malformed(_)), "{err:?}");
}

#[test]
fn missing_paths_report_rather_than_panic() {
    let t = TempDir::new("missing");
    let vpk = Builder::default()
        .add("scripts/x.vdata_c", b"x")
        .open_in(t.path())
        .expect("open");
    assert!(vpk.find("scripts/nope.vdata_c").is_none());
    assert!(vpk.read_path("scripts/nope.vdata_c").is_err());
}

#[test]
fn an_empty_archive_parses_to_nothing() {
    let t = TempDir::new("empty");
    let vpk = Builder::default().open_in(t.path()).expect("open");
    assert!(vpk.is_empty());
    assert_eq!(vpk.len(), 0);
}
