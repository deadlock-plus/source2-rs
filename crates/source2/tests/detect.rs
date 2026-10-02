#![allow(missing_docs)]

use source2::{Format, detect};

#[test]
fn empty_and_short_inputs_are_unknown() {
    assert_eq!(detect(b""), Format::Unknown);
    assert_eq!(detect(b"\x34\x12\xaa"), Format::Unknown);
    assert_eq!(detect(b"KV3"), Format::Unknown);
    assert_eq!(detect(b"plain words, no magic"), Format::Unknown);
    assert_eq!(detect(b"<!-- something else -->"), Format::Unknown);
}

#[test]
fn format_has_a_readable_name() {
    assert_eq!(Format::Kv3Binary.to_string(), "kv3 binary");
    assert_eq!(Format::Unknown.to_string(), "unknown");
}

#[cfg(feature = "vpk")]
#[test]
fn vpk_directory_files_are_detected() {
    use source2::vpk::{Entry, Vpk};

    for version in [1, 2] {
        let mut pack = Vpk::new(version);
        pack.push(Entry::inline("a/b.txt", b"hi".to_vec()));
        let bytes = pack.to_bytes().unwrap();
        assert_eq!(detect(&bytes), Format::Vpk, "version {version}");
    }
    assert_eq!(detect(&Vpk::new(2).to_bytes().unwrap()), Format::Vpk);
}

#[cfg(feature = "resource")]
mod resource {
    use super::*;
    use source2::resource::{Block, BlockKind, Resource};

    fn sample() -> Resource {
        Resource::new()
            .with_block(Block::new(BlockKind::RERL, vec![1, 2, 3]))
            .with_block(Block::new(BlockKind::DATA, b"payload".to_vec()))
    }

    #[test]
    fn written_resources_are_detected() {
        assert_eq!(detect(&sample().to_bytes().unwrap()), Format::Resource);
        assert_eq!(
            detect(&Resource::new().to_bytes().unwrap()),
            Format::Resource
        );
    }

    #[test]
    fn truncated_resources_are_unknown() {
        let bytes = sample().to_bytes().unwrap();
        for cut in [0, 8, 15, 20, bytes.len() - 1] {
            assert_eq!(detect(&bytes[..cut]), Format::Unknown, "cut at {cut}");
        }
    }

    #[test]
    fn a_stored_size_that_looks_like_another_magic_is_still_a_resource() {
        for size in [0x55aa_1234, 0x0356_4b56, 0x4b56_3305] {
            let bytes = sample().with_declared_size(Some(size)).to_bytes().unwrap();
            assert_eq!(detect(&bytes), Format::Resource, "size {size:#x}");
        }
    }

    #[test]
    fn a_resource_with_another_header_version_is_unknown() {
        let mut bytes = sample().to_bytes().unwrap();
        bytes[4] = 13;
        assert_eq!(detect(&bytes), Format::Unknown);
    }
}

#[cfg(feature = "kv1")]
#[test]
fn kv1_has_no_magic_and_is_never_guessed() {
    use source2::kv1::{Document, Entry};

    let doc = Document::new(vec![Entry::section(
        "Root",
        vec![Entry::string("key", "value")],
    )]);
    assert_eq!(detect(doc.to_text().unwrap().as_bytes()), Format::Unknown);
    assert_eq!(detect(&doc.to_binary().unwrap()), Format::Unknown);
}

#[cfg(feature = "kv2")]
mod kv2 {
    use super::*;
    use source2::kv2::{Document, Element, Encoding};

    fn written(encoding: Encoding, version: u32) -> Vec<u8> {
        let mut doc = Document::with_encoding(encoding, version, "dmx", 1);
        doc.add_element(Element::new("Root"));
        doc.to_bytes().unwrap()
    }

    #[test]
    fn text_encodings_are_detected() {
        assert_eq!(detect(&written(Encoding::KeyValues2, 1)), Format::Kv2Text);
        assert_eq!(
            detect(&written(Encoding::KeyValues2NoIds, 1)),
            Format::Kv2Text
        );
    }

    #[test]
    fn binary_versions_are_detected() {
        for version in [1, 2, 3, 4, 5, 9] {
            assert_eq!(
                detect(&written(Encoding::Binary, version)),
                Format::Kv2Binary,
                "version {version}"
            );
        }
    }

    #[test]
    fn crlf_headers_are_detected() {
        assert_eq!(
            detect(b"<!-- dmx encoding keyvalues2 1 format dmx 1 -->\r\n"),
            Format::Kv2Text
        );
    }

    #[test]
    fn a_byte_order_mark_is_not_accepted() {
        let mut bytes = b"\xef\xbb\xbf".to_vec();
        bytes.extend(written(Encoding::KeyValues2, 1));
        assert_eq!(detect(&bytes), Format::Unknown);
    }
}

#[cfg(feature = "kv3")]
mod kv3 {
    use super::*;
    use source2::kv3::{Compression, Document, Object, Version, WriteOptions};

    fn doc() -> Document {
        Document::new(Object::from_iter([("k", 1)]))
    }

    #[test]
    fn every_binary_revision_is_detected() {
        for version in [
            Version::Legacy,
            Version::V1,
            Version::V2,
            Version::V3,
            Version::V4,
            Version::V5,
        ] {
            let options = WriteOptions {
                version,
                compression: Compression::None,
                ..WriteOptions::default()
            };
            let bytes = doc().to_bytes_with(&options).unwrap();
            assert_eq!(detect(&bytes), Format::Kv3Binary, "{version:?}");
        }
    }

    #[test]
    fn default_binary_output_is_detected() {
        assert_eq!(detect(&doc().to_bytes().unwrap()), Format::Kv3Binary);
    }

    #[test]
    fn text_with_header_is_detected() {
        let text = doc().to_text().unwrap();
        assert!(text.starts_with("<!-- kv3"));
        assert_eq!(detect(text.as_bytes()), Format::Kv3Text);
    }

    #[test]
    fn text_tolerates_a_byte_order_mark_and_leading_whitespace() {
        assert_eq!(
            detect(b"\xef\xbb\xbf\n  <!--kv3 encoding:text:version{e21c7f3c-8a33-41c5-9977-a76d3a32aa0d} format:generic:version{7412167c-06e9-4698-aff2-e63eb59037e7} -->\n{}"),
            Format::Kv3Text
        );
    }

    #[test]
    fn a_kv3_looking_word_without_the_header_is_unknown() {
        assert_eq!(detect(b"<!-- kv3x -->"), Format::Unknown);
        assert_eq!(detect(b"{ a = 1 }"), Format::Unknown);
    }
}

#[cfg(all(feature = "kv2", feature = "kv3"))]
#[test]
fn text_headers_do_not_cross_over() {
    use source2::kv2::{Document as Dmx, Element};
    use source2::kv3::{Document as Kv3, Object};

    let mut dmx = Dmx::new("dmx", 1);
    dmx.add_element(Element::new("Root"));
    let kv3 = Kv3::new(Object::new());
    assert_eq!(detect(&dmx.to_bytes().unwrap()), Format::Kv2Text);
    assert_eq!(detect(kv3.to_text().unwrap().as_bytes()), Format::Kv3Text);
}
