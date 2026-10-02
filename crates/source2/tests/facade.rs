#![allow(missing_docs)]

#[cfg(feature = "vpk")]
#[test]
fn vpk_round_trips_through_the_facade() {
    use source2::vpk::{Entry, Vpk};

    let mut pack = Vpk::new(2);
    pack.push(Entry::inline("a/b.txt", b"hi".to_vec()));
    let bytes = pack.to_bytes().unwrap();
    assert_eq!(
        Vpk::parse(&bytes).unwrap().read_path("a/b.txt").unwrap(),
        b"hi"
    );
}

#[cfg(feature = "resource")]
#[test]
fn resource_round_trips_through_the_facade() {
    use source2::resource::{Block, BlockKind, Resource};

    let resource = Resource::new().with_block(Block::new(BlockKind::DATA, b"x".to_vec()));
    let bytes = resource.to_bytes().unwrap();
    assert_eq!(Resource::parse(&bytes).unwrap(), resource);
}

#[cfg(feature = "kv1")]
#[test]
fn kv1_round_trips_through_the_facade() {
    use source2::kv1::Document;

    let doc = Document::parse(r#""A" { "k" "v" }"#).unwrap();
    let text = doc.to_text().unwrap();
    assert_eq!(Document::parse(&text).unwrap().to_text().unwrap(), text);
}

#[cfg(feature = "kv2")]
#[test]
fn kv2_round_trips_through_the_facade() {
    use source2::kv2::Document;

    let src = b"<!-- dmx encoding keyvalues2 1 format dmx 1 -->\n";
    assert_eq!(Document::parse(src).unwrap().to_bytes().unwrap(), src);
}

#[cfg(feature = "kv3")]
#[test]
fn kv3_round_trips_through_the_facade() {
    use source2::kv3::{Document, Object};

    let doc = Document::new(Object::from_iter([("k", 1)]));
    let bytes = doc.to_bytes().unwrap();
    assert_eq!(Document::from_bytes(&bytes).unwrap().root, doc.root);
}

#[cfg(all(feature = "kv3", feature = "lz4"))]
#[test]
fn kv3_lz4_feature_is_forwarded() {
    use source2::kv3::{Compression, Document, Object, WriteOptions};

    let doc = Document::new(Object::from_iter([("k", 1)]));
    let options = WriteOptions {
        compression: Compression::Lz4,
        ..WriteOptions::default()
    };
    let bytes = doc.to_bytes_with(&options).unwrap();
    assert_eq!(Document::from_bytes(&bytes).unwrap().root, doc.root);
}

#[cfg(all(feature = "kv3", feature = "zstd"))]
#[test]
fn kv3_zstd_feature_is_forwarded() {
    use source2::kv3::{Compression, Document, Object, WriteOptions};

    let doc = Document::new(Object::from_iter([("k", 1)]));
    let options = WriteOptions {
        compression: Compression::Zstd,
        ..WriteOptions::default()
    };
    let bytes = doc.to_bytes_with(&options).unwrap();
    assert_eq!(Document::from_bytes(&bytes).unwrap().root, doc.root);
}
