//! Writer tests. Expected bytes are assembled by hand from the VPK layout (digests from an
//! independent MD5/CRC implementation), never by the writer under test.

use crate::tests::{Builder, TempDir};
use crate::*;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn le32(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

fn doc(version: u32, entries: Vec<Entry>) -> Vpk {
    let mut v = Vpk::new(version);
    for e in entries {
        v.push(e);
    }
    v
}

#[test]
fn v1_with_one_archived_file_is_exactly_header_tree_and_archive() {
    let built = doc(1, vec![Entry::new("dir/file.txt", b"hello".to_vec())])
        .build()
        .unwrap();

    let mut expected = Vec::new();
    expected.extend_from_slice(&le32(0x55aa_1234));
    expected.extend_from_slice(&le32(1));
    expected.extend_from_slice(&le32(34));
    expected.extend_from_slice(b"txt\0dir\0file\0");
    expected.extend_from_slice(&le32(0x3610_a686));
    expected.extend_from_slice(&[0, 0]);
    expected.extend_from_slice(&[0, 0]);
    expected.extend_from_slice(&le32(0));
    expected.extend_from_slice(&le32(5));
    expected.extend_from_slice(&[0xff, 0xff]);
    expected.extend_from_slice(&[0, 0, 0]);

    assert_eq!(built.directory, expected);
    assert_eq!(
        built.archives,
        vec![BuiltArchive {
            index: 0,
            bytes: b"hello".to_vec()
        }]
    );
}

#[test]
fn v1_inline_file_sits_after_the_tree_with_the_inline_archive_index() {
    let built = doc(1, vec![Entry::inline("a.txt", b"hi".to_vec())])
        .build()
        .unwrap();

    let mut expected = Vec::new();
    expected.extend_from_slice(&le32(0x55aa_1234));
    expected.extend_from_slice(&le32(1));
    expected.extend_from_slice(&le32(29));
    expected.extend_from_slice(b"txt\0 \0a\0");
    expected.extend_from_slice(&le32(0xd893_2aac));
    expected.extend_from_slice(&[0, 0]);
    expected.extend_from_slice(&[0xff, 0x7f]);
    expected.extend_from_slice(&le32(0));
    expected.extend_from_slice(&le32(2));
    expected.extend_from_slice(&[0xff, 0xff]);
    expected.extend_from_slice(&[0, 0, 0]);
    expected.extend_from_slice(b"hi");

    assert_eq!(built.directory, expected);
    assert!(built.archives.is_empty());
}

#[test]
fn v2_footer_carries_the_archive_md5_section_and_the_three_digests() {
    let entry = Entry::new("x/y.bin", b"abc".to_vec())
        .with_preload_len(1)
        .unwrap();
    let built = doc(2, vec![entry]).build().unwrap();

    let mut expected = Vec::new();
    expected.extend_from_slice(&le32(0x55aa_1234));
    expected.extend_from_slice(&le32(2));
    expected.extend_from_slice(&le32(30));
    expected.extend_from_slice(&le32(0));
    expected.extend_from_slice(&le32(28));
    expected.extend_from_slice(&le32(48));
    expected.extend_from_slice(&le32(0));
    expected.extend_from_slice(b"bin\0x\0y\0");
    expected.extend_from_slice(&le32(0x3524_41c2));
    expected.extend_from_slice(&[1, 0]);
    expected.extend_from_slice(&[0, 0]);
    expected.extend_from_slice(&le32(0));
    expected.extend_from_slice(&le32(2));
    expected.extend_from_slice(&[0xff, 0xff]);
    expected.extend_from_slice(b"a");
    expected.extend_from_slice(&[0, 0, 0]);
    // One record for the one chunk of archive 0.
    expected.extend_from_slice(&le32(0));
    expected.extend_from_slice(&le32(0));
    expected.extend_from_slice(&le32(2));
    expected.extend_from_slice(&hex("5360af35bde9ebd8f01f492dc059593c"));
    expected.extend_from_slice(&hex("604615753d522cc55fc4cdb00a8c94ee"));
    expected.extend_from_slice(&hex("a428f841e6799e9dc69171a51ba4c883"));
    expected.extend_from_slice(&hex("c24c5092df2f0bd06e92ddc19e111180"));

    assert_eq!(built.directory, expected);
    assert_eq!(built.archives[0].bytes, b"bc");
}

#[test]
fn v2_inline_data_gets_an_inline_md5_record() {
    let built = doc(2, vec![Entry::inline("a.txt", b"hi".to_vec())])
        .build()
        .unwrap();

    let mut expected = Vec::new();
    expected.extend_from_slice(&le32(0x55aa_1234));
    expected.extend_from_slice(&le32(2));
    expected.extend_from_slice(&le32(29));
    expected.extend_from_slice(&le32(2));
    expected.extend_from_slice(&le32(28));
    expected.extend_from_slice(&le32(48));
    expected.extend_from_slice(&le32(0));
    expected.extend_from_slice(b"txt\0 \0a\0");
    expected.extend_from_slice(&le32(0xd893_2aac));
    expected.extend_from_slice(&[0, 0]);
    expected.extend_from_slice(&[0xff, 0x7f]);
    expected.extend_from_slice(&le32(0));
    expected.extend_from_slice(&le32(2));
    expected.extend_from_slice(&[0xff, 0xff]);
    expected.extend_from_slice(&[0, 0, 0]);
    expected.extend_from_slice(b"hi");
    expected.extend_from_slice(&le32(0x8000_0000));
    expected.extend_from_slice(&le32(0));
    expected.extend_from_slice(&le32(2));
    expected.extend_from_slice(&hex("49f68a5c8493ec2c0bf489821c21fc3b"));
    expected.extend_from_slice(&hex("b823eeeb99ffe3824eec10f45e4516d4"));
    expected.extend_from_slice(&hex("c42be6e82c80d7a3326ea6a440a2ccbe"));
    expected.extend_from_slice(&hex("002f3ee36e4a14b3793df0734822582b"));

    assert_eq!(built.directory, expected);
}

/// Valve hashes each archive in 1 MiB pieces, so a bigger archive gets several records.
#[test]
fn v2_archive_md5_records_are_per_one_mib_chunk() {
    let big = vec![7u8; MD5_CHUNK_SIZE as usize + 5];
    let mut v = Vpk::new(2);
    v.add("big.bin", big.clone()).unwrap();
    let built = v.build().unwrap();
    let section_len = u32::from_le_bytes(built.directory[16..20].try_into().unwrap());
    assert_eq!(section_len, 56);

    let parsed = Vpk::parse(&built.directory).unwrap();
    let records = parsed.archive_md5.unwrap();
    assert_eq!(
        records
            .iter()
            .map(|r| (r.target(), r.offset, r.length))
            .collect::<Vec<_>>(),
        vec![
            (Md5Target::Archive(0), 0, MD5_CHUNK_SIZE),
            (Md5Target::Archive(0), MD5_CHUNK_SIZE, 5),
        ]
    );
}

#[test]
fn v2_without_archived_bytes_or_inline_data_has_no_archive_md5_records() {
    let mut e = Entry::new("a.txt", b"hi".to_vec())
        .with_preload_len(2)
        .unwrap();
    e.crc = crc32(b"hi");
    let built = doc(2, vec![e]).build().unwrap();
    let d = &built.directory;
    assert_eq!(d[12..16], le32(0), "inline data section size");
    assert_eq!(d[16..20], le32(0), "archive md5 section size");
    assert_eq!(d[20..24], le32(48), "other md5 section size");
    assert_eq!(d[24..28], le32(0), "signature section size");
    assert!(built.archives.is_empty());
}

fn round_trip(doc: &Vpk, tag: &str) -> (TempDir, Vpk) {
    let t = TempDir::new(tag);
    let dir_path = t.path().join("pack_dir.vpk");
    doc.write(&dir_path).expect("write");
    let vpk = Vpk::open(&dir_path).expect("open");
    (t, vpk)
}

#[test]
fn hand_built_documents_read_back_in_both_versions() {
    let big: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
    for version in [1, 2] {
        let mut v = Vpk::new(version);
        v.add("docs/first.txt", b"first payload".to_vec()).unwrap();
        v.push(
            Entry::new("docs/split.bin", big.clone())
                .with_preload_len(100)
                .unwrap(),
        );
        v.push(
            Entry::new("docs/small.txt", b"all preload".to_vec())
                .with_preload_len(11)
                .unwrap(),
        );
        v.push(Entry::inline("docs/gen/inline.txt", b"in the dir".to_vec()));
        v.add("toplevel.txt", b"root file".to_vec()).unwrap();
        v.add("LICENSE", b"no extension".to_vec()).unwrap();
        v.add("docs/empty.txt", Vec::new()).unwrap();
        let (_t, vpk) = round_trip(&v, &format!("rt-v{version}"));

        assert_eq!(vpk.version, version);
        assert_eq!(vpk.len(), 7);
        assert_eq!(vpk.read_path("docs/first.txt").unwrap(), b"first payload");
        assert_eq!(vpk.read_path("docs/split.bin").unwrap(), big);
        assert_eq!(vpk.find("docs/split.bin").unwrap().preload.len(), 100);
        let small = vpk.find("docs/small.txt").unwrap();
        assert_eq!(small.data_len(), 0);
        assert_eq!(vpk.read(small).unwrap(), b"all preload");
        let inline = vpk.find("docs/gen/inline.txt").unwrap();
        assert!(matches!(inline.data, Data::Stored { archive: None, .. }));
        assert_eq!(vpk.read(inline).unwrap(), b"in the dir");
        assert_eq!(vpk.read_path("toplevel.txt").unwrap(), b"root file");
        assert_eq!(vpk.read_path("LICENSE").unwrap(), b"no extension");
        assert_eq!(vpk.read_path("docs/empty.txt").unwrap(), b"");
        assert_eq!(
            vpk.find("docs/empty.txt").unwrap().data,
            Data::Stored {
                archive: None,
                offset: u32::MAX,
                length: 0
            }
        );
        assert_eq!(vpk.find("LICENSE").unwrap().extension(), None);
    }
}

/// Writing what was read, then reading it again, gives the same document: the names the
/// reader accepts are the names the writer produces.
#[test]
fn reader_and_writer_agree_on_root_and_extensionless_spelling() {
    let mut v = Vpk::new(2);
    v.add("LICENSE", b"a".to_vec()).unwrap();
    v.add("dir/NOTES", b"b".to_vec()).unwrap();
    v.add("top.txt", b"c".to_vec()).unwrap();
    let built = v.build().unwrap();
    let again = Vpk::parse(&built.directory).unwrap();
    assert_eq!(
        again
            .entries()
            .iter()
            .map(|e| &e.path[..])
            .collect::<Vec<_>>(),
        ["LICENSE", "dir/NOTES", "top.txt"]
    );
    assert_eq!(again.build().unwrap().directory, built.directory);

    assert!(
        built.directory[28..].starts_with(b" \0 \0LICENSE\0"),
        "no extension and no directory are both the single-space spelling"
    );
}

#[test]
fn inline_only_output_writes_just_the_directory_file() {
    let t = TempDir::new("dir-only");
    let dir_path = t.path().join("pack_dir.vpk");
    doc(2, vec![Entry::inline("a.txt", b"hi".to_vec())])
        .write(&dir_path)
        .unwrap();
    assert!(dir_path.exists());
    assert!(!t.path().join("pack_000.vpk").exists());
}

#[test]
fn an_inline_only_pack_can_have_any_file_name() {
    let t = TempDir::new("standalone");
    let path = t.path().join("standalone.vpk");
    doc(2, vec![Entry::inline("a.txt", b"hi".to_vec())])
        .write(&path)
        .unwrap();
    assert_eq!(Vpk::open(&path).unwrap().read_path("a.txt").unwrap(), b"hi");
}

#[test]
fn to_bytes_is_the_directory_file_of_a_pack_without_archives() {
    let v = doc(2, vec![Entry::inline("a.txt", b"hi".to_vec())]);
    assert_eq!(v.to_bytes().unwrap(), v.build().unwrap().directory);

    let archived = doc(2, vec![Entry::new("a.txt", b"hi".to_vec())]);
    assert!(matches!(archived.to_bytes(), Err(Error::InvalidInput(_))));
}

#[test]
fn a_size_limit_splits_files_across_numbered_archives() {
    let mut v = Vpk::new(2);
    v.max_archive_size = Some(4);
    v.add("a.bin", b"aaa".to_vec()).unwrap();
    v.add("b.bin", b"bbb".to_vec()).unwrap();
    v.add("c.bin", b"cc".to_vec()).unwrap();
    v.add("d.bin", b"d".to_vec()).unwrap();
    let built = v.build().unwrap();
    assert_eq!(
        built
            .archives
            .iter()
            .map(|a| (a.index, a.bytes.clone()))
            .collect::<Vec<_>>(),
        vec![
            (0, b"aaa".to_vec()),
            (1, b"bbb".to_vec()),
            (2, b"ccd".to_vec())
        ]
    );

    let (t, vpk) = round_trip(&v, "split-archives");
    let at = |p: &str| match vpk.find(p).unwrap().data {
        Data::Stored {
            archive, offset, ..
        } => (archive, offset),
        Data::Memory { .. } => unreachable!(),
    };
    assert_eq!(at("a.bin"), (Some(0), 0));
    assert_eq!(at("b.bin"), (Some(1), 0));
    assert_eq!(at("c.bin"), (Some(2), 0));
    assert_eq!(at("d.bin"), (Some(2), 2));
    assert!(t.path().join("pack_002.vpk").exists());
    for name in ["a.bin", "b.bin", "c.bin", "d.bin"] {
        vpk.read_path(name).unwrap();
    }
}

#[test]
fn a_file_larger_than_the_limit_gets_an_archive_to_itself() {
    let mut v = Vpk::new(1);
    v.max_archive_size = Some(2);
    v.add("a.bin", b"abcd".to_vec()).unwrap();
    v.add("b.bin", b"x".to_vec()).unwrap();
    let built = v.build().unwrap();
    assert_eq!(
        built
            .archives
            .iter()
            .map(|a| a.bytes.clone())
            .collect::<Vec<_>>(),
        vec![b"abcd".to_vec(), b"x".to_vec()]
    );
}

#[test]
fn add_places_an_entry_beside_its_extension_and_directory_siblings() {
    let mut v = Vpk::new(2);
    v.add("a/one.txt", Vec::new()).unwrap();
    v.add("b/two.cfg", Vec::new()).unwrap();
    v.add("a/three.txt", Vec::new()).unwrap();
    v.add("c/four.txt", Vec::new()).unwrap();
    v.add("b/five.cfg", Vec::new()).unwrap();
    let paths: Vec<_> = v.entries().iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "a/one.txt",
            "a/three.txt",
            "c/four.txt",
            "b/two.cfg",
            "b/five.cfg"
        ]
    );
    let built = v.build().unwrap();
    // One extension group per extension: "txt" and "cfg" each appear once in the tree.
    let tree = &built.directory[28..];
    assert_eq!(tree.windows(4).filter(|w| w == b"txt\0").count(), 1);
    assert_eq!(tree.windows(4).filter(|w| w == b"cfg\0").count(), 1);
}

#[test]
fn rejects_invalid_input() {
    let cases: Vec<(&str, &str)> = vec![
        ("empty path", ""),
        ("empty name", "dir/.hidden"),
        ("trailing slash", "dir/"),
        ("leading slash", "/a.txt"),
        ("double slash", "a//b.txt"),
        ("interior nul", "a\0b.txt"),
        ("empty extension", "dir/a."),
    ];
    for (what, path) in cases {
        let mut v = Vpk::new(2);
        assert!(
            matches!(v.add(path, b"x".to_vec()), Err(Error::InvalidInput(_))),
            "add: {what}"
        );
        let mut v = Vpk::new(2);
        v.push(Entry::new(path, b"x".to_vec()));
        assert!(
            matches!(v.build(), Err(Error::InvalidInput(_))),
            "build: {what}"
        );
    }
}

#[test]
fn add_rejects_a_duplicate_path() {
    let mut v = Vpk::new(2);
    v.add("a.txt", b"1".to_vec()).unwrap();
    assert!(matches!(
        v.add("a.txt", b"2".to_vec()),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(v.len(), 1);
}

/// Pushed duplicates are representable, so they are written, not rejected or merged.
#[test]
fn pushed_duplicates_are_written_as_given() {
    let mut v = Vpk::new(2);
    v.push(Entry::inline("a.txt", b"1".to_vec()));
    v.push(Entry::inline("a.txt", b"2".to_vec()));
    let again = Vpk::parse(&v.build().unwrap().directory).unwrap();
    assert_eq!(again.len(), 2);
    assert_eq!(again.read(&again.entries()[1]).unwrap(), b"2");
}

#[test]
fn a_preload_that_does_not_fit_is_an_error_not_a_clamp() {
    let e = Entry::new("a.txt", b"abc".to_vec());
    assert!(matches!(
        e.clone().with_preload_len(4),
        Err(Error::InvalidInput(_))
    ));
    assert!(e.with_preload_len(3).is_ok());

    let big = Entry::new("a.bin", vec![0u8; 70_000]);
    assert!(matches!(
        big.clone().with_preload_len(70_000),
        Err(Error::InvalidInput(_))
    ));
    assert!(big.with_preload_len(65_535).is_ok());

    let mut over = Entry::new("a.bin", Vec::new());
    over.preload = vec![0; 65_536];
    assert!(matches!(
        doc(2, vec![over]).build(),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn a_preload_can_be_resized() {
    let e = Entry::new("a.bin", b"abcdef".to_vec())
        .with_preload_len(4)
        .unwrap()
        .with_preload_len(2)
        .unwrap();
    assert_eq!(e.preload, b"ab");
    assert_eq!(e.data_len(), 4);
    assert_eq!(e.size(), 6);
}

#[test]
fn rejects_an_unsupported_version() {
    for v in [0, 3, 4] {
        let mut d = Vpk::new(v);
        d.add("a.txt", b"x".to_vec()).unwrap();
        assert!(matches!(d.build(), Err(Error::UnsupportedVersion(x)) if x == v));
    }
}

#[test]
fn version_one_cannot_carry_version_two_sections() {
    let mut v = Vpk::new(1);
    v.add("a.txt", b"x".to_vec()).unwrap();
    v.other_md5 = Some(OtherMd5::default());
    assert!(matches!(v.build(), Err(Error::InvalidInput(_))));
    v.other_md5 = None;
    v.signature = Some(Signature::unsigned());
    assert!(matches!(v.build(), Err(Error::InvalidInput(_))));
}

#[test]
fn write_requires_a_dir_suffixed_file_name_when_archives_are_needed() {
    let t = TempDir::new("bad-name");
    let mut v = Vpk::new(2);
    v.add("a.txt", b"x".to_vec()).unwrap();
    let err = v.write(t.path().join("pack.vpk")).unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)), "{err:?}");
    assert!(!t.path().join("pack.vpk").exists(), "nothing half-written");
}

#[test]
fn an_empty_document_produces_a_readable_empty_archive() {
    let (_t, vpk) = round_trip(&Vpk::new(2), "empty-doc");
    assert!(vpk.is_empty());
}

#[test]
fn a_parsed_document_can_take_new_files_and_still_read_everything() {
    let src = TempDir::new("grow-src");
    let parsed = Builder::default()
        .add("docs/old.txt", b"old payload")
        .add_inline("docs/old_inline.txt", b"old inline")
        .open_in(src.path())
        .unwrap();

    let mut v = parsed.clone();
    v.add("docs/new.txt", b"new payload".to_vec()).unwrap();
    v.push(Entry::inline("docs/new_inline.txt", b"new inline".to_vec()));

    let built = v.build().unwrap();
    assert_eq!(built.archives.len(), 1);
    assert_eq!(
        built.archives[0].index, 1,
        "after the archive already in use"
    );

    let dest = TempDir::new("grow-dest");
    let path = dest.path().join("copy_dir.vpk");
    v.write(&path).unwrap();
    let back = Vpk::open(&path).unwrap();
    assert_eq!(back.len(), 4);
    assert_eq!(back.read_path("docs/old.txt").unwrap(), b"old payload");
    assert_eq!(back.read_path("docs/new.txt").unwrap(), b"new payload");
    assert_eq!(
        back.read_path("docs/old_inline.txt").unwrap(),
        b"old inline"
    );
    assert_eq!(
        back.read_path("docs/new_inline.txt").unwrap(),
        b"new inline"
    );
    assert!(dest.path().join("copy_000.vpk").exists(), "copied through");
    assert!(dest.path().join("copy_001.vpk").exists());
}

/// Records are regenerated when memory entries change the data they describe, and every
/// record then matches.
#[test]
fn archive_md5_records_follow_the_data_after_edits() {
    let src = TempDir::new("regen-src");
    let mut v = Builder::default()
        .add("docs/old.txt", b"old payload")
        .open_in(src.path())
        .unwrap();
    v.archive_md5 = Some(Vec::new());
    v.other_md5 = Some(OtherMd5::default());
    v.add("docs/new.txt", b"new payload".to_vec()).unwrap();

    let dest = TempDir::new("regen-dest");
    let path = dest.path().join("copy_dir.vpk");
    v.write(&path).unwrap();
    let back = Vpk::open(&path).unwrap();
    let report = back.verify_archive_md5().unwrap();
    assert_eq!(report.matched, 2, "one chunk per archive");
    assert!(report.all_ok());
    assert!(back.verify_directory_md5().unwrap().all_ok());
}

/// Writing parsed entries back into the pack they came from rewrites only the directory.
#[test]
fn writing_over_the_source_does_not_touch_the_archives() {
    let t = TempDir::new("in-place");
    let mut v = Builder::default()
        .add("docs/old.txt", b"old payload")
        .open_in(t.path())
        .unwrap();
    let before = std::fs::read(t.path().join("pack_000.vpk")).unwrap();
    v.add("docs/new.txt", b"new payload".to_vec()).unwrap();
    v.write(t.path().join("pack_dir.vpk")).unwrap();
    assert_eq!(
        std::fs::read(t.path().join("pack_000.vpk")).unwrap(),
        before
    );
    let back = Vpk::open(t.path().join("pack_dir.vpk")).unwrap();
    assert_eq!(back.read_path("docs/new.txt").unwrap(), b"new payload");
    assert_eq!(back.read_path("docs/old.txt").unwrap(), b"old payload");
}

#[test]
fn a_parsed_document_without_a_source_cannot_be_copied() {
    let (dir, _) = Builder::default().add("a.txt", b"x").build();
    let v = Vpk::parse(&dir).unwrap();
    let t = TempDir::new("no-source");
    assert!(matches!(
        v.write(t.path().join("copy_dir.vpk")),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn entries_can_be_removed_and_looked_up_afterwards() {
    let mut v = Vpk::new(2);
    v.add("a.txt", b"1".to_vec()).unwrap();
    v.add("b.txt", b"2".to_vec()).unwrap();
    assert!(v.find("a.txt").is_some());
    let removed = v.remove("a.txt").unwrap();
    assert_eq!(removed.path, "a.txt");
    assert!(v.find("a.txt").is_none());
    assert!(v.find("b.txt").is_some());
    assert!(v.remove("a.txt").is_none());
}

#[test]
fn refresh_crc_follows_edited_bytes() {
    let mut e = Entry::new("a.txt", b"abc".to_vec());
    assert_eq!(e.crc, crc32(b"abc"));
    if let Data::Memory { bytes, .. } = &mut e.data {
        bytes.push(b'd');
    }
    e.refresh_crc().unwrap();
    assert_eq!(e.crc, crc32(b"abcd"));

    let (dir, _) = Builder::default().add_inline("a.txt", b"x").build();
    let mut parsed = Vpk::parse(&dir).unwrap();
    assert!(parsed.entries_mut()[0].refresh_crc().is_err());
}
