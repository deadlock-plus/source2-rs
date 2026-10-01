//! The version 2 integrity sections: records, digests and the signature.

use crate::md5::md5;
use crate::round_trip_tests::{Dir, Ent, Sig, raw_v2, record, tree};
use crate::tests::TempDir;
use crate::*;

fn inline_tree() -> Vec<u8> {
    let dirs: Vec<Dir<'_>> = vec![(
        "docs",
        vec![Ent {
            name: "a",
            crc: crc32(b"abc"),
            preload: b"",
            archive: 0x7fff,
            offset: 0,
            length: 3,
        }],
    )];
    tree(&[("txt", dirs)])
}

#[test]
fn record_targets_follow_the_index_spellings_that_can_be_checked() {
    let rec = |archive_index| ArchiveMd5 {
        archive_index,
        offset: 0,
        length: 0,
        md5: [0; 16],
    };
    assert_eq!(rec(0).target(), Md5Target::Archive(0));
    assert_eq!(rec(5).target(), Md5Target::Archive(5));
    assert_eq!(rec(300).target(), Md5Target::Archive(300));
    assert_eq!(rec(0x1_0000).target(), Md5Target::Unknown);
    assert_eq!(rec(0x1_0005).target(), Md5Target::Unknown);
    assert_eq!(rec(0x8000_0000).target(), Md5Target::Inline);
    assert_eq!(rec(0x7fff).target(), Md5Target::Unknown);
    assert_eq!(rec(0x1_7fff).target(), Md5Target::Unknown);
    assert_eq!(rec(0x4000_0000).target(), Md5Target::Unknown);
}

#[test]
fn parsed_sections_are_public_data() {
    let t = inline_tree();
    let recs = record(0x8000_0000, 0, 3, b"abc");
    let bytes = raw_v2(&t, b"abc", &recs, true, Sig::None);
    let vpk = Vpk::parse(&bytes).unwrap();

    assert_eq!(
        vpk.archive_md5,
        Some(vec![ArchiveMd5::for_inline(0, 3, md5(b"abc"))])
    );
    assert_eq!(
        vpk.other_md5.unwrap().tree,
        md5(&t),
        "the stored digest is exposed as found"
    );
    assert_eq!(vpk.other_md5.unwrap().archive_md5_section, md5(&recs));
    assert_eq!(vpk.signature, None);
    assert!(vpk.verify_directory_md5().unwrap().all_ok());
}

#[test]
fn a_file_without_an_other_md5_section_has_nothing_to_verify() {
    let bytes = raw_v2(&inline_tree(), b"abc", b"", false, Sig::None);
    let vpk = Vpk::parse(&bytes).unwrap();
    assert_eq!(vpk.other_md5, None);
    assert_eq!(
        vpk.verify_directory_md5().unwrap(),
        DirectoryMd5Report::default()
    );
}

#[test]
fn inline_records_are_checked_against_the_inline_data() {
    let good = record(0x8000_0000, 0, 3, b"abc");
    let bad = record(0x8000_0000, 0, 3, b"xyz");
    let unknown = record(0x1_7fff, 0, 3, b"qqq");

    let vpk = Vpk::parse(&raw_v2(&inline_tree(), b"abc", &good, true, Sig::None)).unwrap();
    let report = vpk.verify_archive_md5().unwrap();
    assert_eq!((report.matched, report.all_ok()), (1, true));

    let mut recs = good.clone();
    recs.extend(&bad);
    recs.extend(&unknown);
    let vpk = Vpk::parse(&raw_v2(&inline_tree(), b"abc", &recs, true, Sig::None)).unwrap();
    let report = vpk.verify_archive_md5().unwrap();
    assert_eq!(report.matched, 1);
    assert_eq!(report.mismatched, [1]);
    assert_eq!(report.unchecked, [2]);
    assert!(!report.all_ok());
}

#[test]
fn archive_records_catch_a_changed_archive() {
    let t = TempDir::new("md5-archive");
    let mut v = Vpk::new(2);
    v.add("docs/a.bin", b"alpha".to_vec()).unwrap();
    v.add("docs/b.bin", b"beta".to_vec()).unwrap();
    v.max_archive_size = Some(5);
    let path = t.path().join("pack_dir.vpk");
    v.write(&path).unwrap();

    let vpk = Vpk::open(&path).unwrap();
    assert_eq!(vpk.archive_md5.as_ref().unwrap().len(), 2);
    let report = vpk.verify_archive_md5().unwrap();
    assert_eq!(report.matched, 2);

    std::fs::write(t.path().join("pack_001.vpk"), b"BETA").unwrap();
    let report = vpk.verify_archive_md5().unwrap();
    assert_eq!((report.matched, report.mismatched), (1, vec![1]));
}

#[test]
fn archive_records_that_run_past_the_archive_count_as_mismatches() {
    let t = TempDir::new("md5-short");
    let mut v = Vpk::new(2);
    v.add("docs/a.bin", b"alpha".to_vec()).unwrap();
    let path = t.path().join("pack_dir.vpk");
    v.write(&path).unwrap();
    std::fs::write(t.path().join("pack_000.vpk"), b"al").unwrap();
    let report = Vpk::open(&path).unwrap().verify_archive_md5().unwrap();
    assert_eq!(report.mismatched, [0]);
}

#[test]
fn a_missing_archive_is_an_io_error_when_verifying() {
    let t = TempDir::new("md5-missing");
    let mut v = Vpk::new(2);
    v.add("docs/a.bin", b"alpha".to_vec()).unwrap();
    let path = t.path().join("pack_dir.vpk");
    v.write(&path).unwrap();
    std::fs::remove_file(t.path().join("pack_000.vpk")).unwrap();
    let err = Vpk::open(&path).unwrap().verify_archive_md5().unwrap_err();
    assert!(matches!(err, Error::Io { .. }), "{err:?}");
}

#[test]
fn generated_directory_digests_verify() {
    let mut v = Vpk::new(2);
    v.add("docs/a.bin", b"alpha".to_vec()).unwrap();
    v.push(Entry::inline("docs/b.txt", b"beta".to_vec()));
    let parsed = Vpk::parse(&v.build().unwrap().directory).unwrap();
    assert!(parsed.verify_directory_md5().unwrap().all_ok());
    let other = parsed.other_md5.unwrap();
    assert_ne!(other.tree, [0; 16]);
    assert_ne!(other.whole_file, [0; 16]);
}

/// The digests cover the document as written: a changed entry gives different ones.
#[test]
fn digests_follow_the_document() {
    let mut v = Vpk::new(2);
    v.push(Entry::inline("a.txt", b"x".to_vec()));
    let a = Vpk::parse(&v.build().unwrap().directory).unwrap();
    v.push(Entry::inline("b.txt", b"y".to_vec()));
    let b = Vpk::parse(&v.build().unwrap().directory).unwrap();
    assert_ne!(a.other_md5.unwrap().tree, b.other_md5.unwrap().tree);
    assert_ne!(
        a.other_md5.unwrap().whole_file,
        b.other_md5.unwrap().whole_file
    );
}

#[test]
fn a_signature_survives_a_write_and_is_not_invented() {
    let plain = Vpk::new(2);
    assert_eq!(plain.signature, None);
    let bytes = plain.build().unwrap().directory;
    assert_eq!(bytes[24..28], 0u32.to_le_bytes());

    let mut signed = Vpk::new(2);
    signed.add("docs/a.bin", b"a".to_vec()).unwrap();
    signed.signature = Some(Signature {
        public_key: vec![1, 2, 3],
        signature: vec![4, 5, 6, 7],
        layout: SignatureLayout::Headed {
            version: 1,
            reserved: 0,
        },
    });
    let built = signed.build().unwrap().directory;
    assert_eq!(built[24..28], 20u32.to_le_bytes());
    let back = Vpk::parse(&built).unwrap();
    assert_eq!(back.signature, signed.signature);
}

#[test]
fn directory_report_ignores_missing_digests() {
    let report = DirectoryMd5Report {
        tree: Some(true),
        archive_md5_section: None,
        whole_file: Some(true),
    };
    assert!(report.all_ok());
    let report = DirectoryMd5Report {
        tree: Some(true),
        archive_md5_section: None,
        whole_file: Some(false),
    };
    assert!(!report.all_ok());
}
