//! Expected bytes here are assembled by hand from the VPK layout and never by the writer,
//! so a shared mistake in layout cannot make the writer and its own reader agree on
//! something wrong.

use super::*;
use crate::tests::TempDir;
use crate::{SIGNATURE, Vpk};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn le32(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

#[test]
fn v1_with_one_archived_file_is_exactly_header_tree_and_archive() {
    let built = VpkWriter::new(1)
        .add("dir/file.txt", b"hello")
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
    assert_eq!(built.archives, vec![b"hello".to_vec()]);
}

#[test]
fn v1_inline_file_sits_after_the_tree_with_the_inline_archive_index() {
    let built = VpkWriter::new(1)
        .add_inline("a.txt", b"hi")
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
fn v2_footer_carries_the_archive_md5_section_and_the_three_checksums() {
    let built = VpkWriter::new(2)
        .add_with_preload("x/y.bin", b"abc", 1)
        .build()
        .unwrap();

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
    expected.extend_from_slice(&le32(0));
    expected.extend_from_slice(&le32(0));
    expected.extend_from_slice(&le32(2));
    expected.extend_from_slice(&hex("5360af35bde9ebd8f01f492dc059593c"));
    expected.extend_from_slice(&hex("604615753d522cc55fc4cdb00a8c94ee"));
    expected.extend_from_slice(&hex("a428f841e6799e9dc69171a51ba4c883"));
    expected.extend_from_slice(&hex("c24c5092df2f0bd06e92ddc19e111180"));

    assert_eq!(built.directory, expected);
    assert_eq!(built.archives, vec![b"bc".to_vec()]);
}

#[test]
fn v2_with_no_archived_bytes_has_an_empty_archive_md5_section() {
    let built = VpkWriter::new(2)
        .add_inline("a.txt", b"hi")
        .build()
        .unwrap();
    let d = &built.directory;
    assert_eq!(d[0..4], le32(SIGNATURE));
    assert_eq!(d[12..16], le32(2), "inline data section size");
    assert_eq!(d[16..20], le32(0), "archive md5 section size");
    assert_eq!(d[20..24], le32(48), "other md5 section size");
    assert_eq!(d[24..28], le32(0), "signature section size");
    assert_eq!(d.len(), 28 + 29 + 2 + 48);
}

fn round_trip(writer: &VpkWriter, tag: &str) -> (TempDir, Vpk) {
    let t = TempDir::new(tag);
    let dir_path = t.path().join("pak01_dir.vpk");
    writer.write(&dir_path).expect("write");
    let vpk = Vpk::open(&dir_path).expect("open");
    (t, vpk)
}

#[test]
fn round_trips_through_the_reader_in_both_versions() {
    let big: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
    for version in [1, 2] {
        let writer = VpkWriter::new(version)
            .add("scripts/heroes.vdata_c", b"hero payload")
            .add_with_preload("scripts/split.bin", &big, 100)
            .add_with_preload("scripts/small.vdata_c", b"all preload", 11)
            .add_inline("scripts/gen/inline.vdata_c", b"in the dir")
            .add("toplevel.txt", b"root file")
            .add("LICENSE", b"no extension")
            .add("scripts/empty.txt", b"");
        let (_t, vpk) = round_trip(&writer, &format!("rt-v{version}"));

        assert_eq!(vpk.version(), version);
        assert_eq!(vpk.len(), 7);
        assert_eq!(
            vpk.read_path("scripts/heroes.vdata_c").unwrap(),
            b"hero payload"
        );
        assert_eq!(vpk.read_path("scripts/split.bin").unwrap(), big);
        assert_eq!(vpk.find("scripts/split.bin").unwrap().preload.len(), 100);
        let small = vpk.find("scripts/small.vdata_c").unwrap();
        assert_eq!(small.length, 0);
        assert_eq!(vpk.read(small).unwrap(), b"all preload");
        let inline = vpk.find("scripts/gen/inline.vdata_c").unwrap();
        assert_eq!(inline.archive, None);
        assert_eq!(vpk.read(inline).unwrap(), b"in the dir");
        assert_eq!(vpk.read_path("toplevel.txt").unwrap(), b"root file");
        assert_eq!(vpk.read_path("LICENSE").unwrap(), b"no extension");
        assert_eq!(vpk.read_path("scripts/empty.txt").unwrap(), b"");
    }
}

#[test]
fn inline_only_output_writes_just_the_directory_file() {
    let t = TempDir::new("dir-only");
    let dir_path = t.path().join("pak01_dir.vpk");
    VpkWriter::new(2)
        .add_inline("a.txt", b"hi")
        .write(&dir_path)
        .unwrap();
    assert!(dir_path.exists());
    assert!(!t.path().join("pak01_000.vpk").exists());
}

#[test]
fn a_size_limit_splits_files_across_numbered_archives() {
    let writer = VpkWriter::new(2)
        .max_archive_size(4)
        .add("a.bin", b"aaa")
        .add("b.bin", b"bbb")
        .add("c.bin", b"cc")
        .add("d.bin", b"d");
    let built = writer.build().unwrap();
    assert_eq!(
        built.archives,
        vec![b"aaa".to_vec(), b"bbb".to_vec(), b"ccd".to_vec()]
    );

    let (t, vpk) = round_trip(&writer, "split-archives");
    assert_eq!(vpk.find("a.bin").unwrap().archive, Some(0));
    assert_eq!(vpk.find("b.bin").unwrap().archive, Some(1));
    assert_eq!(vpk.find("c.bin").unwrap().archive, Some(2));
    assert_eq!(vpk.find("d.bin").unwrap().archive, Some(2));
    assert_eq!(vpk.find("d.bin").unwrap().offset, 2);
    assert!(t.path().join("pak01_002.vpk").exists());
    for name in ["a.bin", "b.bin", "c.bin", "d.bin"] {
        vpk.read_path(name).unwrap();
    }
}

#[test]
fn a_file_larger_than_the_limit_gets_an_archive_to_itself() {
    let built = VpkWriter::new(1)
        .max_archive_size(2)
        .add("a.bin", b"abcd")
        .add("b.bin", b"x")
        .build()
        .unwrap();
    assert_eq!(built.archives, vec![b"abcd".to_vec(), b"x".to_vec()]);
}

#[test]
fn v2_archive_md5_section_has_one_entry_per_archived_file() {
    let built = VpkWriter::new(2)
        .max_archive_size(3)
        .add("a.bin", b"aaa")
        .add("b.bin", b"bb")
        .build()
        .unwrap();
    let section_len = u32::from_le_bytes(built.directory[16..20].try_into().unwrap());
    assert_eq!(section_len, 56);
}

#[test]
fn rejects_invalid_input() {
    let cases: Vec<(&str, VpkWriter)> = vec![
        ("empty path", VpkWriter::new(2).add("", b"x")),
        ("empty name", VpkWriter::new(2).add("dir/.hidden", b"x")),
        ("trailing slash", VpkWriter::new(2).add("dir/", b"x")),
        (
            "duplicate",
            VpkWriter::new(2).add("a.txt", b"1").add("a.txt", b"2"),
        ),
        (
            "preload too large",
            VpkWriter::new(2).add_with_preload("a.bin", &vec![0; 70_000], 70_000),
        ),
        ("interior nul", VpkWriter::new(2).add("a\0b.txt", b"x")),
    ];
    for (what, writer) in cases {
        assert!(
            matches!(writer.build(), Err(Error::InvalidInput(_))),
            "{what}"
        );
    }
}

#[test]
fn rejects_an_unsupported_version() {
    assert!(matches!(
        VpkWriter::new(3).add("a.txt", b"x").build(),
        Err(Error::UnsupportedVersion(3))
    ));
}

#[test]
fn write_requires_a_dir_suffixed_file_name() {
    let t = TempDir::new("bad-name");
    let err = VpkWriter::new(2)
        .add("a.txt", b"x")
        .write(t.path().join("pak01.vpk"))
        .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)), "{err:?}");
}

#[test]
fn an_empty_writer_produces_a_readable_empty_archive() {
    let (_t, vpk) = round_trip(&VpkWriter::new(2), "empty-writer");
    assert!(vpk.is_empty());
}
