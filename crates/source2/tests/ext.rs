#![allow(missing_docs)]

use source2::{Error, Format};

#[test]
fn errors_are_send_sync_and_display() {
    fn check<T: std::error::Error + Send + Sync + 'static>() {}
    check::<Error>();

    let missing = Error::EntryNotFound("a/b.txt".into());
    assert!(missing.to_string().contains("a/b.txt"));
    let wrong = Error::UnrecognisedFormat {
        expected: Format::Kv3Binary,
        found: Format::Unknown,
    };
    assert!(wrong.to_string().contains("kv3 binary"));
    assert!(std::error::Error::source(&wrong).is_none());
}

#[cfg(feature = "vpk")]
#[test]
fn wrapped_errors_expose_their_source() {
    let inner = source2::vpk::Error::NotFound("x".into());
    let text = inner.to_string();
    let err = Error::from(inner);
    assert!(matches!(err, Error::Vpk(_)));
    assert_eq!(
        std::error::Error::source(&err).map(ToString::to_string),
        Some(text)
    );
}

#[cfg(feature = "kv3")]
#[test]
fn kv3_errors_convert() {
    let err: Error = source2::kv3::Error::Malformed("x".into()).into();
    assert!(matches!(err, Error::Kv3(_)));
    assert!(std::error::Error::source(&err).is_some());
}

#[cfg(all(feature = "resource", feature = "kv3"))]
mod resource_kv3 {
    use source2::ext::ResourceKv3;
    use source2::kv3::{Document, Object};
    use source2::resource::{Block, BlockKind, Padding, Resource, Versions};
    use source2::{Error, Format};

    pub fn kv3(n: i64) -> Document {
        let mut root = Object::new();
        root.insert("count", n);
        root.insert("name", "demo");
        Document::new(root)
    }

    pub fn resource(n: i64) -> Resource {
        Resource::new()
            .with_versions(Versions::new(12, 3))
            .with_block(Block::new(BlockKind::RERL, vec![1, 2, 3]))
            .with_block(
                Block::new(BlockKind::DATA, kv3(n).to_bytes().unwrap())
                    .with_padding(Padding::Exact(vec![0xaa; 5])),
            )
            .with_block(Block::new(BlockKind::NTRO, vec![9; 40]))
            .with_pre_table(vec![7, 7, 7, 7])
            .with_trailing(vec![5, 5])
    }

    #[test]
    fn reads_a_block_as_kv3() {
        let doc = resource(4).read_kv3(BlockKind::DATA).unwrap();
        assert_eq!(doc.root, kv3(4).root);
    }

    #[test]
    fn put_replaces_only_the_named_block() {
        let before = resource(4);
        let mut after = Resource::parse(&before.to_bytes().unwrap()).unwrap();
        let mut doc = after.read_kv3(BlockKind::DATA).unwrap();
        doc.root.as_object_mut().unwrap().insert("count", 500);
        after.put_kv3(BlockKind::DATA, &doc).unwrap();

        let again = Resource::parse(&after.to_bytes().unwrap()).unwrap();
        assert_eq!(again.versions, before.versions);
        assert_eq!(again.pre_table, before.pre_table);
        assert_eq!(again.trailing, before.trailing);
        assert_eq!(again.blocks.len(), before.blocks.len());
        for (new, old) in again.blocks.iter().zip(&before.blocks) {
            assert_eq!(new.kind, old.kind);
            assert_eq!(new.padding, old.padding);
            if new.kind != BlockKind::DATA {
                assert_eq!(new.data, old.data);
            }
        }
        let edited = again.read_kv3(BlockKind::DATA).unwrap();
        assert_eq!(
            edited
                .root
                .get("count")
                .and_then(source2::kv3::Value::as_i64),
            Some(500)
        );
        assert_eq!(
            edited.root.get("name").and_then(|v| v.as_str()),
            Some("demo")
        );
    }

    #[test]
    fn put_without_edit_keeps_the_payload() {
        let mut r = resource(4);
        let original = r.read_kv3(BlockKind::DATA).unwrap();
        r.put_kv3(BlockKind::DATA, &original).unwrap();
        assert_eq!(r, resource(4));
    }

    #[test]
    fn put_appends_a_missing_block() {
        let mut r = resource(1);
        r.put_kv3(BlockKind::new(*b"XTRA"), &kv3(8)).unwrap();
        assert_eq!(r.blocks.len(), 4);
        assert_eq!(r.blocks[3].kind, BlockKind::new(*b"XTRA"));
        assert_eq!(r.blocks[3].padding, Padding::Auto);
        assert_eq!(
            r.read_kv3(BlockKind::new(*b"XTRA")).unwrap().root,
            kv3(8).root
        );
    }

    #[test]
    fn only_the_first_block_of_a_kind_is_touched() {
        let mut r = resource(1).with_block(Block::new(BlockKind::DATA, b"second".to_vec()));
        r.put_kv3(BlockKind::DATA, &kv3(2)).unwrap();
        assert_eq!(r.blocks[3].data, b"second");
        assert_eq!(r.read_kv3(BlockKind::DATA).unwrap().root, kv3(2).root);
    }

    #[test]
    fn a_missing_block_is_an_error() {
        let err = resource(1).read_kv3(BlockKind::MBUF).unwrap_err();
        assert!(
            matches!(err, Error::BlockNotFound(ref k) if k == "MBUF"),
            "{err:?}"
        );
    }

    #[test]
    fn a_block_that_is_not_kv3_is_an_error() {
        let err = resource(1).read_kv3(BlockKind::RERL).unwrap_err();
        assert!(
            matches!(
                err,
                Error::UnrecognisedFormat {
                    expected: Format::Kv3Binary,
                    found: Format::Unknown
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn a_truncated_kv3_block_is_an_error() {
        let mut r = resource(1);
        let full = r.blocks[1].data.clone();
        r.blocks[1].data.truncate(full.len() - 3);
        assert!(matches!(r.read_kv3(BlockKind::DATA), Err(Error::Kv3(_))));
        r.blocks[1].data.truncate(10);
        assert!(matches!(r.read_kv3(BlockKind::DATA), Err(Error::Kv3(_))));
    }
}

#[cfg(all(feature = "vpk", feature = "resource"))]
mod vpk_resource {
    use source2::Error;
    use source2::ext::VpkResource;
    use source2::resource::{Block, BlockKind, Resource};
    use source2::vpk::{Entry, Vpk};

    pub fn pack(entries: &[(&str, &[u8])]) -> Vpk {
        let mut pack = Vpk::new(2);
        for (path, bytes) in entries {
            pack.push(Entry::inline(*path, bytes.to_vec()));
        }
        Vpk::parse(&pack.to_bytes().unwrap()).unwrap()
    }

    fn res(tag: u8) -> Resource {
        Resource::new().with_block(Block::new(BlockKind::DATA, vec![tag; 8]))
    }

    #[test]
    fn reads_an_entry_as_a_resource() {
        let bytes = res(1).to_bytes().unwrap();
        let p = pack(&[("a/x.thing_c", &bytes)]);
        assert_eq!(p.read_resource("a/x.thing_c").unwrap(), res(1));
    }

    #[test]
    fn missing_entries_and_bad_bytes_are_errors() {
        let p = pack(&[
            ("a/x.thing_c", b"not a resource at all, sorry"),
            ("a/s.thing_c", &[1, 2, 3]),
        ]);
        assert!(matches!(
            p.read_resource("nope.thing_c"),
            Err(Error::EntryNotFound(ref path)) if path == "nope.thing_c"
        ));
        assert!(matches!(
            p.read_resource("a/x.thing_c"),
            Err(Error::Resource(_))
        ));
        assert!(matches!(
            p.read_resource("a/s.thing_c"),
            Err(Error::Resource(_))
        ));
    }

    #[test]
    fn put_replaces_in_place_and_leaves_other_entries_alone() {
        let a = res(1).to_bytes().unwrap();
        let mut p = pack(&[
            ("a/x.thing_c", &a),
            ("a/y.thing_c", &a),
            ("docs/readme.txt", b"hello"),
        ]);
        p.put_resource("a/x.thing_c", &res(2)).unwrap();

        assert_eq!(p.len(), 3);
        assert_eq!(p.read_resource("a/x.thing_c").unwrap(), res(2));
        assert_eq!(p.read_path("a/y.thing_c").unwrap(), a);
        assert_eq!(p.read_path("docs/readme.txt").unwrap(), b"hello");
    }

    #[test]
    fn put_adds_a_new_entry_bound_for_numbered_archives() {
        let mut p = pack(&[("docs/readme.txt", b"hello")]);
        p.put_resource("a/new.thing_c", &res(3)).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p.read_resource("a/new.thing_c").unwrap(), res(3));
        assert!(p.to_bytes().is_err());
        let built = p.build().unwrap();
        assert_eq!(built.archives.len(), 1);
        let reopened = Vpk::parse(&built.directory).unwrap();
        assert!(reopened.find("a/new.thing_c").is_some());
    }

    #[test]
    fn put_keeps_an_inline_entry_inline() {
        let a = res(1).to_bytes().unwrap();
        let mut p = pack(&[("a/x.thing_c", &a)]);
        p.put_resource("a/x.thing_c", &res(2)).unwrap();
        let again = Vpk::parse(&p.to_bytes().unwrap()).unwrap();
        assert_eq!(again.read_resource("a/x.thing_c").unwrap(), res(2));
    }

    #[test]
    fn put_collapses_duplicate_paths_into_one_entry() {
        let a = res(1).to_bytes().unwrap();
        let mut p = Vpk::new(2);
        p.push(Entry::inline("a/x.thing_c", a.clone()));
        p.push(Entry::inline("a/x.thing_c", a));
        assert_eq!(p.len(), 2);
        p.put_resource("a/x.thing_c", &res(2)).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p.read_resource("a/x.thing_c").unwrap(), res(2));
    }

    #[test]
    fn put_with_an_invalid_path_keeps_the_pack_unchanged() {
        let a = res(1).to_bytes().unwrap();
        let mut p = pack(&[("a/x.thing_c", &a)]);
        let before = p.entries().to_vec();
        assert!(matches!(p.put_resource("", &res(2)), Err(Error::Vpk(_))));
        assert!(matches!(
            p.put_resource("/lead/x.thing_c", &res(2)),
            Err(Error::Vpk(_))
        ));
        assert_eq!(p.entries(), before.as_slice());
    }

    #[test]
    fn an_unwritable_resource_is_an_error_and_changes_nothing() {
        let a = res(1).to_bytes().unwrap();
        let mut p = pack(&[("a/x.thing_c", &a)]);
        let mut bad = res(2);
        bad.versions.header = 99;
        assert!(matches!(
            p.put_resource("a/x.thing_c", &bad),
            Err(Error::Resource(_))
        ));
        assert_eq!(p.read_resource("a/x.thing_c").unwrap(), res(1));
    }
}

#[cfg(all(feature = "vpk", feature = "kv1"))]
mod vpk_kv1 {
    use source2::Error;
    use source2::ext::VpkKv1;
    use source2::kv1::{Document, Entry as Kv1Entry};
    use source2::vpk::{Entry, Vpk};

    fn doc() -> Document {
        Document::new(vec![Kv1Entry::section(
            "Root",
            vec![Kv1Entry::string("k", "v"), Kv1Entry::int("n", 5)],
        )])
    }

    #[test]
    fn text_round_trips() {
        let text = doc().stringified();
        let mut p = Vpk::new(2);
        p.put_kv1_text("cfg/a.vdf", &text).unwrap();
        assert_eq!(
            p.read_kv1_text("cfg/a.vdf").unwrap().without_layout(),
            text.without_layout()
        );
    }

    #[test]
    fn binary_round_trips() {
        let mut p = Vpk::new(2);
        p.put_kv1_binary("cfg/a.bin", &doc()).unwrap();
        assert_eq!(p.read_kv1_binary("cfg/a.bin").unwrap(), doc());
    }

    #[test]
    fn typed_values_do_not_silently_become_text() {
        let mut p = Vpk::new(2);
        assert!(matches!(
            p.put_kv1_text("cfg/a.vdf", &doc()),
            Err(Error::Kv1(_))
        ));
        assert!(p.is_empty());
    }

    #[test]
    fn errors_are_reported() {
        let mut p = Vpk::new(2);
        p.push(Entry::inline("cfg/bad.bin", vec![0, 1, 2]));
        assert!(matches!(
            p.read_kv1_binary("cfg/bad.bin"),
            Err(Error::Kv1(_))
        ));
        assert!(matches!(
            p.read_kv1_text("nope.vdf"),
            Err(Error::EntryNotFound(_))
        ));
    }
}

#[cfg(all(feature = "vpk", feature = "kv2"))]
mod vpk_kv2 {
    use source2::Error;
    use source2::ext::VpkKv2;
    use source2::kv2::{Document, Element, Encoding};
    use source2::vpk::Vpk;

    #[test]
    fn text_and_binary_round_trip_in_their_own_encoding() {
        for (encoding, version) in [(Encoding::KeyValues2, 1), (Encoding::Binary, 5)] {
            let mut doc = Document::with_encoding(encoding, version, "dmx", 1);
            doc.add_element(Element::new("Root").attr("n", 3));
            let mut p = Vpk::new(2);
            p.put_kv2("scene/a.dmx", &doc).unwrap();
            assert_eq!(
                p.read_kv2("scene/a.dmx").unwrap().to_bytes().unwrap(),
                doc.to_bytes().unwrap()
            );
        }
    }

    #[test]
    fn errors_are_reported() {
        let mut p = Vpk::new(2);
        p.add("scene/bad.dmx", b"junk".to_vec()).unwrap();
        assert!(matches!(p.read_kv2("scene/bad.dmx"), Err(Error::Kv2(_))));
        assert!(matches!(
            p.read_kv2("nope.dmx"),
            Err(Error::EntryNotFound(_))
        ));
    }
}

#[cfg(all(feature = "vpk", feature = "kv3"))]
mod vpk_kv3 {
    use source2::ext::VpkKv3;
    use source2::kv3::{Document, Object};
    use source2::vpk::Vpk;
    use source2::{Error, Format};

    fn doc() -> Document {
        Document::new(Object::from_iter([("k", 1), ("j", 2)]))
    }

    #[test]
    fn binary_round_trips() {
        let mut p = Vpk::new(2);
        p.put_kv3("cfg/a.kv3", &doc()).unwrap();
        assert_eq!(p.read_kv3("cfg/a.kv3").unwrap().root, doc().root);
    }

    #[test]
    fn text_round_trips() {
        let mut p = Vpk::new(2);
        p.put_kv3_text("cfg/a.kv3", &doc()).unwrap();
        assert_eq!(p.read_kv3_text("cfg/a.kv3").unwrap().root, doc().root);
        let stored = p.read_path("cfg/a.kv3").unwrap();
        assert_eq!(source2::detect(&stored), Format::Kv3Text);
    }

    #[test]
    fn the_encoding_is_chosen_by_the_method_not_the_extension() {
        let mut p = Vpk::new(2);
        p.put_kv3_text("cfg/a.thing_c", &doc()).unwrap();
        let err = p.read_kv3("cfg/a.thing_c").unwrap_err();
        assert!(
            matches!(
                err,
                Error::UnrecognisedFormat {
                    expected: Format::Kv3Binary,
                    found: Format::Kv3Text
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn text_that_is_not_utf8_is_an_error() {
        let mut p = Vpk::new(2);
        p.add("cfg/a.kv3", vec![0xff, 0xfe, 0x00]).unwrap();
        assert!(
            matches!(p.read_kv3_text("cfg/a.kv3"), Err(Error::NotUtf8(ref s)) if s == "cfg/a.kv3")
        );
    }

    #[test]
    fn missing_entries_are_errors() {
        let p = Vpk::new(2);
        assert!(matches!(p.read_kv3("x.kv3"), Err(Error::EntryNotFound(_))));
        assert!(matches!(
            p.read_kv3_text("x.kv3"),
            Err(Error::EntryNotFound(_))
        ));
    }
}

#[cfg(all(feature = "vpk", feature = "resource", feature = "kv3"))]
mod vpk_resource_kv3 {
    use super::resource_kv3::{kv3, resource};
    use source2::ext::{ResourceKv3, VpkResource, VpkResourceKv3};
    use source2::resource::{BlockKind, Resource};
    use source2::vpk::{Entry, Vpk};
    use source2::{Error, Format};

    fn pack() -> Vpk {
        let mut p = Vpk::new(2);
        p.push(Entry::inline(
            "m/a.thing_c",
            resource(4).to_bytes().unwrap(),
        ));
        p.push(Entry::inline(
            "m/b.thing_c",
            resource(9).to_bytes().unwrap(),
        ));
        p.push(Entry::inline("docs/readme.txt", b"hello".to_vec()));
        Vpk::parse(&p.to_bytes().unwrap()).unwrap()
    }

    #[test]
    fn reads_the_kv3_of_a_resource_entry() {
        let doc = pack()
            .read_resource_kv3("m/a.thing_c", BlockKind::DATA)
            .unwrap();
        assert_eq!(doc.root, kv3(4).root);
    }

    #[test]
    fn edit_and_put_back_leaves_everything_else_byte_identical() {
        let mut p = pack();
        let before_b = p.read_path("m/b.thing_c").unwrap();
        let before_readme = p.read_path("docs/readme.txt").unwrap();
        let before_a: Resource = p.read_resource("m/a.thing_c").unwrap();

        let mut doc = p.read_resource_kv3("m/a.thing_c", BlockKind::DATA).unwrap();
        doc.root.as_object_mut().unwrap().insert("count", 77);
        p.put_resource_kv3("m/a.thing_c", BlockKind::DATA, &doc)
            .unwrap();

        let reopened = Vpk::parse(&p.to_bytes().unwrap()).unwrap();
        assert_eq!(reopened.len(), 3);
        assert_eq!(reopened.read_path("m/b.thing_c").unwrap(), before_b);
        assert_eq!(
            reopened.read_path("docs/readme.txt").unwrap(),
            before_readme
        );

        let after_a = reopened.read_resource("m/a.thing_c").unwrap();
        assert_eq!(after_a.versions, before_a.versions);
        assert_eq!(after_a.pre_table, before_a.pre_table);
        assert_eq!(after_a.trailing, before_a.trailing);
        assert_eq!(after_a.blocks.len(), before_a.blocks.len());
        for (new, old) in after_a.blocks.iter().zip(&before_a.blocks) {
            assert_eq!((new.kind, &new.padding), (old.kind, &old.padding));
            if new.kind != BlockKind::DATA {
                assert_eq!(new.data, old.data);
            }
        }
        let edited = after_a.read_kv3(BlockKind::DATA).unwrap();
        assert_eq!(
            edited
                .root
                .get("count")
                .and_then(source2::kv3::Value::as_i64),
            Some(77)
        );
    }

    #[test]
    fn put_back_without_edit_is_a_no_op_on_content() {
        let mut p = pack();
        let before = p.read_resource("m/a.thing_c").unwrap();
        let doc = p.read_resource_kv3("m/a.thing_c", BlockKind::DATA).unwrap();
        p.put_resource_kv3("m/a.thing_c", BlockKind::DATA, &doc)
            .unwrap();
        assert_eq!(p.read_resource("m/a.thing_c").unwrap(), before);
    }

    #[test]
    fn error_paths() {
        let mut p = pack();
        p.push(Entry::inline(
            "m/junk.thing_c",
            b"junk junk junk junk junk".to_vec(),
        ));
        let mut short = resource(1).to_bytes().unwrap();
        short.truncate(20);
        p.push(Entry::inline("m/short.thing_c", short));
        let doc = kv3(1);

        assert!(matches!(
            p.read_resource_kv3("nope.thing_c", BlockKind::DATA),
            Err(Error::EntryNotFound(_))
        ));
        assert!(matches!(
            p.put_resource_kv3("nope.thing_c", BlockKind::DATA, &doc),
            Err(Error::EntryNotFound(_))
        ));
        assert!(matches!(
            p.read_resource_kv3("m/junk.thing_c", BlockKind::DATA),
            Err(Error::Resource(_))
        ));
        assert!(matches!(
            p.read_resource_kv3("m/short.thing_c", BlockKind::DATA),
            Err(Error::Resource(_))
        ));
        assert!(matches!(
            p.read_resource_kv3("m/a.thing_c", BlockKind::MBUF),
            Err(Error::BlockNotFound(_))
        ));
        assert!(matches!(
            p.read_resource_kv3("m/a.thing_c", BlockKind::RERL),
            Err(Error::UnrecognisedFormat {
                found: Format::Unknown,
                ..
            })
        ));
        assert!(matches!(
            p.read_resource_kv3("docs/readme.txt", BlockKind::DATA),
            Err(Error::Resource(_))
        ));
        let untouched = p.read_resource("m/a.thing_c").unwrap();
        assert_eq!(untouched, resource(4));
    }
}
