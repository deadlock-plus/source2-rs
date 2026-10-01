//! Builds every model type through the public API only, as a downstream crate would.

use source2_resource::{Block, BlockKind, HEADER_VERSION, Padding, Resource, Versions};

#[test]
fn versions_are_constructible() {
    let v = Versions::new(HEADER_VERSION, 5);
    assert_eq!(v.header, HEADER_VERSION);
    assert_eq!(v.resource, 5);
    assert_eq!(Versions::default().header, HEADER_VERSION);
    assert_eq!(Versions::default().resource, 0);
}

#[test]
fn block_builders_set_fields() {
    let block = Block::new(BlockKind::DATA, vec![1, 2]).with_padding(Padding::Exact(vec![9; 3]));
    assert_eq!(block.kind, BlockKind::DATA);
    assert_eq!(block.data, [1, 2]);
    assert_eq!(block.padding, Padding::Exact(vec![9; 3]));
    assert_eq!(Block::new(BlockKind::DATA, vec![]).padding, Padding::Auto);
}

#[test]
fn resource_builders_round_trip() {
    let resource = Resource::new()
        .with_versions(Versions::new(HEADER_VERSION, 2))
        .with_block(Block::new(BlockKind::RERL, b"refs".to_vec()))
        .with_block(
            Block::new(BlockKind::DATA, b"payload".to_vec()).with_padding(Padding::Exact(vec![7])),
        )
        .with_pre_table(vec![0xAA; 4])
        .with_trailing(vec![0xBB; 2]);

    assert_eq!(resource.versions.resource, 2);
    assert_eq!(resource.blocks.len(), 2);
    assert_eq!(resource.pre_table, [0xAA; 4]);
    assert_eq!(resource.trailing, [0xBB; 2]);
    assert_eq!(resource.declared_size, None);

    let bytes = resource.to_bytes().unwrap();
    assert_eq!(Resource::parse(&bytes).unwrap(), resource);
}

#[test]
fn declared_size_builder_is_written() {
    let resource = Resource::new().with_declared_size(Some(1234));
    assert_eq!(resource.declared_size, Some(1234));
    let bytes = resource.to_bytes().unwrap();
    assert_eq!(&bytes[0..4], &1234u32.to_le_bytes());
    assert_eq!(Resource::parse(&bytes).unwrap().declared_size, Some(1234));
}

#[test]
fn fields_stay_editable() {
    let mut resource = Resource::new();
    resource.versions.resource = 3;
    resource.push_block(BlockKind::DATA, vec![1]);
    resource.blocks[0].data = vec![4, 5];
    resource.blocks[0].padding = Padding::Auto;
    resource.trailing.push(0);
    let parsed = Resource::parse(&resource.to_bytes().unwrap()).unwrap();
    assert_eq!(parsed.data(), Some(&[4u8, 5][..]));
    assert_eq!(parsed.versions.resource, 3);
}

#[test]
fn padding_match_needs_wildcard() {
    let padding = Padding::default();
    let label = match padding {
        Padding::Auto => "auto",
        Padding::Exact(_) => "exact",
        _ => "other",
    };
    assert_eq!(label, "auto");
}
