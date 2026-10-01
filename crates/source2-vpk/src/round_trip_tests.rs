//! Parse then write must give the same bytes. The inputs are assembled here from the
//! layout, with layouts a generating writer would never choose: unsorted trees, repeated
//! groups, offset gaps, shared and overlapping ranges, duplicates, wrong CRCs.

use crate::md5::md5;
use crate::tests::push_cstr;
use crate::*;

pub(crate) struct Ent<'a> {
    pub name: &'a str,
    pub crc: u32,
    pub preload: &'a [u8],
    pub archive: u16,
    pub offset: u32,
    pub length: u32,
}

pub(crate) type Dir<'a> = (&'a str, Vec<Ent<'a>>);
pub(crate) type Group<'a> = (&'a str, Vec<Dir<'a>>);

pub(crate) fn tree(groups: &[Group<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for (ext, dirs) in groups {
        push_cstr(&mut out, ext);
        for (dir, ents) in dirs {
            push_cstr(&mut out, dir);
            for e in ents {
                push_cstr(&mut out, e.name);
                out.extend_from_slice(&e.crc.to_le_bytes());
                out.extend_from_slice(&(e.preload.len() as u16).to_le_bytes());
                out.extend_from_slice(&e.archive.to_le_bytes());
                out.extend_from_slice(&e.offset.to_le_bytes());
                out.extend_from_slice(&e.length.to_le_bytes());
                out.extend_from_slice(&0xffffu16.to_le_bytes());
                out.extend_from_slice(e.preload);
            }
            out.push(0);
        }
        out.push(0);
    }
    out.push(0);
    out
}

pub(crate) fn record(index: u32, offset: u32, length: u32, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&index.to_le_bytes());
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(&md5(data));
    out
}

/// What goes after the digests of a version 2 file.
pub(crate) enum Sig {
    None,
    /// Section bytes, then the bytes that follow the section.
    Raw(Vec<u8>, Vec<u8>),
}

/// A version 2 directory file with correct digests.
pub(crate) fn raw_v2(tree: &[u8], data: &[u8], records: &[u8], other: bool, sig: Sig) -> Vec<u8> {
    let (section, tail) = match sig {
        Sig::None => (Vec::new(), Vec::new()),
        Sig::Raw(s, t) => (s, t),
    };
    let mut out = Vec::new();
    out.extend_from_slice(&SIGNATURE.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(tree.len() as u32).to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&(records.len() as u32).to_le_bytes());
    out.extend_from_slice(&(if other { 48u32 } else { 0 }).to_le_bytes());
    out.extend_from_slice(&(section.len() as u32).to_le_bytes());
    out.extend_from_slice(tree);
    out.extend_from_slice(data);
    out.extend_from_slice(records);
    if other {
        out.extend_from_slice(&md5(tree));
        out.extend_from_slice(&md5(records));
        let whole = md5(&out);
        out.extend_from_slice(&whole);
    }
    out.extend_from_slice(&section);
    out.extend_from_slice(&tail);
    out
}

fn headed(version: u32, key: usize, sig: usize, reserved: u32) -> Vec<u8> {
    let mut h = Vec::new();
    for v in [SIGNATURE, version, key as u32, sig as u32, reserved] {
        h.extend_from_slice(&v.to_le_bytes());
    }
    h
}

fn assert_same(bytes: &[u8]) -> Vpk {
    let vpk = Vpk::parse(bytes).expect("parse");
    let built = vpk.build().expect("build");
    assert!(built.archives.is_empty(), "parsed entries make no archives");
    assert_eq!(built.directory, bytes);
    vpk
}

fn awkward_tree() -> Vec<u8> {
    tree(&[
        (
            "txt",
            vec![
                (
                    "zeta",
                    vec![
                        Ent {
                            name: "late",
                            crc: 0xdead_beef,
                            preload: b"",
                            archive: 300,
                            offset: 4_000_000_000,
                            length: 7,
                        },
                        Ent {
                            name: "early",
                            crc: 1,
                            preload: b"pre",
                            archive: 0,
                            offset: 50,
                            length: 10,
                        },
                    ],
                ),
                (
                    " ",
                    vec![Ent {
                        name: "root",
                        crc: 2,
                        preload: b"",
                        archive: 0x7fff,
                        offset: 0,
                        length: 3,
                    }],
                ),
            ],
        ),
        (
            " ",
            vec![(
                "alpha/beta",
                vec![
                    Ent {
                        name: "noext",
                        crc: 3,
                        preload: b"",
                        archive: 0x7fff,
                        offset: u32::MAX,
                        length: 0,
                    },
                    Ent {
                        name: "noext",
                        crc: 4,
                        preload: b"dup",
                        archive: 0x7fff,
                        offset: 0,
                        length: 0,
                    },
                ],
            )],
        ),
        (
            "txt",
            vec![(
                "zeta",
                vec![Ent {
                    name: "again",
                    crc: 5,
                    preload: b"",
                    archive: 1,
                    offset: 10,
                    length: 10,
                }],
            )],
        ),
    ])
}

#[test]
fn an_awkward_version_2_file_writes_back_to_the_same_bytes() {
    let bytes = raw_v2(&awkward_tree(), b"abc", b"", true, Sig::None);
    let vpk = assert_same(&bytes);
    let paths: Vec<_> = vpk.entries().iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "zeta/late.txt",
            "zeta/early.txt",
            "root.txt",
            "alpha/beta/noext",
            "alpha/beta/noext",
            "zeta/again.txt"
        ]
    );
    assert_eq!(vpk.entries()[1].preload, b"pre");
    assert_eq!(vpk.entries()[0].crc, 0xdead_beef, "bad CRCs are kept");
}

#[test]
fn a_version_2_file_without_the_other_md5_section_writes_back_the_same() {
    let bytes = raw_v2(&awkward_tree(), b"abc", b"", false, Sig::None);
    let vpk = assert_same(&bytes);
    assert_eq!(vpk.other_md5, None);
}

#[test]
fn archive_md5_records_survive_as_found() {
    let mut recs = record(0x1_0000, 0, 5, b"hello");
    recs.extend(record(0x1_0000, 5, 3, b"abc"));
    recs.extend(record(0x8000_0000, 0, 3, b"abc"));
    recs.extend(record(0x1_7fff, 0, 3, b"zzz"));
    let bytes = raw_v2(&awkward_tree(), b"abc", &recs, true, Sig::None);
    let vpk = assert_same(&bytes);
    assert_eq!(vpk.archive_md5.as_ref().unwrap().len(), 4);
}

#[test]
fn a_shipped_style_signed_file_writes_back_to_the_same_bytes() {
    let key = vec![0x30, 0x82, 1, 2, 3];
    let sig = vec![9u8; 7];
    let mut tail = key.clone();
    tail.extend_from_slice(&sig);
    let bytes = raw_v2(
        &awkward_tree(),
        b"abc",
        &record(0x8000_0000, 0, 3, b"abc"),
        true,
        Sig::Raw(headed(1, key.len(), sig.len(), 0), tail),
    );
    let vpk = assert_same(&bytes);
    let s = vpk.signature.unwrap();
    assert_eq!(s.public_key, key);
    assert_eq!(s.signature, sig);
    assert_eq!(
        s.layout,
        SignatureLayout::Headed {
            version: 1,
            reserved: 0
        }
    );
}

#[test]
fn an_unsigned_header_only_signature_section_is_kept() {
    let bytes = raw_v2(
        &awkward_tree(),
        b"abc",
        b"",
        true,
        Sig::Raw(headed(1, 0, 0, 0), Vec::new()),
    );
    let vpk = assert_same(&bytes);
    assert_eq!(vpk.signature, Some(Signature::unsigned()));
}

#[test]
fn header_words_this_crate_does_not_interpret_are_kept() {
    let bytes = raw_v2(
        &awkward_tree(),
        b"abc",
        b"",
        true,
        Sig::Raw(headed(7, 1, 1, 0xabcd), vec![1, 2]),
    );
    let vpk = assert_same(&bytes);
    assert_eq!(
        vpk.signature.unwrap().layout,
        SignatureLayout::Headed {
            version: 7,
            reserved: 0xabcd
        }
    );
}

#[test]
fn a_classic_signature_section_writes_back_to_the_same_bytes() {
    let mut section = Vec::new();
    section.extend_from_slice(&3u32.to_le_bytes());
    section.extend_from_slice(&[1, 2, 3]);
    section.extend_from_slice(&2u32.to_le_bytes());
    section.extend_from_slice(&[4, 5]);
    let bytes = raw_v2(
        &awkward_tree(),
        b"abc",
        b"",
        true,
        Sig::Raw(section, Vec::new()),
    );
    let vpk = assert_same(&bytes);
    let s = vpk.signature.unwrap();
    assert_eq!((s.public_key, s.signature), (vec![1, 2, 3], vec![4, 5]));
    assert_eq!(s.layout, SignatureLayout::Classic);
}

#[test]
fn a_version_1_file_writes_back_to_the_same_bytes() {
    let t = awkward_tree();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&SIGNATURE.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&(t.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&t);
    bytes.extend_from_slice(b"inline data after the tree, to the end of the file");
    let vpk = assert_same(&bytes);
    assert_eq!(vpk.version, 1);
    assert_eq!(
        vpk.inline_data,
        b"inline data after the tree, to the end of the file"
    );
    assert_eq!(vpk.archive_md5, None);
}

#[test]
fn an_empty_tree_writes_back_to_the_same_bytes() {
    assert_same(&raw_v2(&[0], b"", b"", true, Sig::None));
}

/// Digests describe the file they sit in, so a file whose stored digests are wrong is
/// reported by verification and comes back corrected rather than reproduced.
#[test]
fn wrong_stored_digests_are_corrected_on_write() {
    let mut bytes = raw_v2(&awkward_tree(), b"abc", b"", true, Sig::None);
    let tree_len = awkward_tree().len();
    let digest_at = 28 + tree_len + 3;
    bytes[digest_at] ^= 0xff;

    let vpk = Vpk::parse(&bytes).unwrap();
    let report = vpk.verify_directory_md5().unwrap();
    assert_eq!(report.tree, Some(false));
    assert_eq!(report.archive_md5_section, Some(true));
    assert!(!report.all_ok());

    let fixed = Vpk::parse(&vpk.build().unwrap().directory).unwrap();
    assert!(fixed.verify_directory_md5().unwrap().all_ok());
}

#[test]
fn trailing_bytes_the_sections_do_not_account_for_are_an_error() {
    let mut bytes = raw_v2(&awkward_tree(), b"abc", b"", true, Sig::None);
    bytes.push(0);
    assert!(matches!(Vpk::parse(&bytes), Err(Error::Malformed(_))));

    let mut v1 = Vec::new();
    v1.extend_from_slice(&SIGNATURE.to_le_bytes());
    v1.extend_from_slice(&1u32.to_le_bytes());
    v1.extend_from_slice(&1u32.to_le_bytes());
    v1.extend_from_slice(&[0, 9, 9]);
    // Version 1 has no section sizes: everything after the tree is inline data.
    assert!(Vpk::parse(&v1).is_ok());
}

#[test]
fn inconsistent_section_sizes_are_errors() {
    let good = raw_v2(&awkward_tree(), b"abc", b"", true, Sig::None);

    let mut ragged = good.clone();
    ragged[16..20].copy_from_slice(&27u32.to_le_bytes());
    assert!(matches!(Vpk::parse(&ragged), Err(Error::Malformed(_))));

    let mut odd_other = good.clone();
    odd_other[20..24].copy_from_slice(&16u32.to_le_bytes());
    assert!(matches!(Vpk::parse(&odd_other), Err(Error::Malformed(_))));

    let mut past_end = good;
    past_end[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(Vpk::parse(&past_end), Err(Error::Malformed(_))));
}

#[test]
fn a_header_signature_whose_lengths_disagree_with_the_file_is_an_error() {
    let bytes = raw_v2(
        &awkward_tree(),
        b"abc",
        b"",
        true,
        Sig::Raw(headed(1, 4, 4, 0), vec![0; 7]),
    );
    assert!(matches!(Vpk::parse(&bytes), Err(Error::Malformed(_))));
}

#[test]
fn removing_an_entry_keeps_the_rest_of_the_document() {
    let key = vec![1u8, 2, 3];
    let bytes = raw_v2(
        &awkward_tree(),
        b"abc",
        &record(0x8000_0000, 0, 3, b"abc"),
        true,
        Sig::Raw(headed(1, 3, 0, 0), key.clone()),
    );
    let mut vpk = Vpk::parse(&bytes).unwrap();
    vpk.remove("zeta/late.txt").unwrap();
    let again = Vpk::parse(&vpk.build().unwrap().directory).unwrap();
    assert_eq!(again.len(), 5);
    assert_eq!(again.signature.unwrap().public_key, key);
    assert_eq!(again.archive_md5.unwrap().len(), 1);
    assert_eq!(again.inline_data, b"abc");
}
