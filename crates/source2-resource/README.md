# source2-resource

Reader and writer for the Source 2 compiled-resource container (`*_c` files).

- Every compiled resource shares one envelope: a header, a table of four-character blocks,
  the block bytes, and sometimes extra data after the last block.
- This crate reads and writes that envelope. Parsing then writing returns the input bytes.
- The model is owned and public. Build one by hand, or parse one, edit it, and write it back.
- No dependencies, no feature flags, no `unsafe`.

## Install

```toml
[dependencies]
source2-resource = "0.1"
```

Minimum supported Rust version: 1.88.

## Read

```rust,no_run
use source2_resource::Resource;

let bytes = std::fs::read("input_c")?;
let resource = Resource::parse(&bytes)?;

println!("resource version {}", resource.versions.resource);
for block in &resource.blocks {
    println!("{} ({} bytes)", block.kind, block.data.len());
}
if let Some(data) = resource.data() {
    println!("DATA block: {} bytes", data.len());
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

`Resource::read` takes any `std::io::Read` instead of a slice.

## Write and round trip

```rust
use source2_resource::{Block, BlockKind, HEADER_VERSION, Resource, Versions};

let resource = Resource::new()
    .with_versions(Versions::new(HEADER_VERSION, 1))
    .with_block(Block::new(BlockKind::RERL, b"refs".to_vec()))
    .with_block(Block::new(BlockKind::DATA, b"payload".to_vec()));

let bytes = resource.to_bytes()?;
assert_eq!(Resource::parse(&bytes)?, resource);
# Ok::<(), source2_resource::Error>(())
```

`Resource::write` takes any `std::io::Write`. Parsing a file and writing the model returns
the original bytes:

```rust
use source2_resource::Resource;

# let original = Resource::new().to_bytes()?;
let resource = Resource::parse(&original)?;
assert_eq!(resource.to_bytes()?, original);
# Ok::<(), source2_resource::Error>(())
```

## Edit

Fields are public and can be assigned. `Resource`, `Block`, `Versions` and `Padding` are
`non_exhaustive`, so build them with `Resource::new`, `Block::new`, `Versions::new` or
`Default` and the `with_*` builders rather than struct literals, and give `match` on
`Padding` a wildcard arm. Change a block's bytes, add or remove blocks, then write. Offsets and
lengths are derived on write, and padding defaults to zeros up to a 16-byte boundary.

```rust
use source2_resource::{BlockKind, Resource};

# let mut seed = Resource::new();
# seed.push_block(BlockKind::DATA, b"old".to_vec());
# seed.push_block(BlockKind::RED2, b"edit info".to_vec());
# let bytes = seed.to_bytes()?;
let mut resource = Resource::parse(&bytes)?;

if let Some(block) = resource.blocks.iter_mut().find(|b| b.kind == BlockKind::DATA) {
    block.data = b"new payload".to_vec();
}
resource.blocks.retain(|b| b.kind != BlockKind::RED2);
resource.push_block(BlockKind::new(*b"XTRA"), vec![1, 2, 3]);

let edited = resource.to_bytes()?;
assert_eq!(Resource::parse(&edited)?.data(), Some(&b"new payload"[..]));
# Ok::<(), source2_resource::Error>(())
```

## Errors

Malformed input returns `Error`, a `non_exhaustive` enum. It implements
`std::error::Error` and `From<std::io::Error>`.

```rust
use source2_resource::{Error, Resource};

match Resource::parse(b"too short") {
    Err(Error::Truncated { what, needed, available }) => {
        println!("{what}: need {needed} bytes, have {available}");
    }
    Err(other) => println!("{other}"),
    Ok(_) => unreachable!(),
}
```

| Variant | Raised when |
| --- | --- |
| `Truncated` | The header or block table does not fit in the input. |
| `BadOffset` | A block runs past the end of the input. |
| `BadLayout` | Blocks overlap each other or the table, or are out of offset order. |
| `UnsupportedVersion` | The header version is not 12 (reading or writing). |
| `TooLarge` | An offset, length or count does not fit the format's 32-bit field. |
| `Io` | The underlying reader or writer failed. |

The block table is checked against the input length before anything is allocated, and all
offset arithmetic is checked, so hostile input cannot trigger a large allocation or panic.

## What the model keeps

- Versions, block order, and each block's tag and bytes.
- Padding before each block (`Padding::Auto` is zeros to a 16-byte boundary).
- Bytes between the header and the table (`pre_table`), and bytes after the last block
  (`trailing`).
- The stored size field when it differs from the end of the last block (`declared_size`).
- Offsets and lengths are derived when writing.

## Supported versions

| Version | Status |
| --- | --- |
| Header version 12 | Read and written. Verified byte-for-byte on real files. |
| Any other header version | `Error::UnsupportedVersion`. Not verified: no such file was found. |
| Resource version | Stored as given, never interpreted. 0 to 5 and 18 were seen in real files. |

A check over 168,050 compiled resources from two unrelated installs found all of them at
header version 12 and all rewritten to identical bytes.

## Known limitations

- Container only. Block contents are opaque bytes. No block type is decoded: not `DATA`,
  `RERL`, `RED2`, `REDI`, `NTRO`, or any other. Decode a block with a crate for its
  encoding, for example `source2-kv3` when a `DATA` block holds KeyValues 3.
- Only header version 12 is known.
- A table the model cannot represent (overlapping blocks, or out of offset order) is
  `Error::BadLayout` and is not rewritten. None appeared in real files.
- Whole-file operation: input is read fully into memory and blocks are copied into owned
  `Vec<u8>`s.
- Files larger than 4 GiB are rejected on write (`Error::TooLarge`).

## Tests

- Unit tests build resources with an independent encoder and cover both directions
  (bytes to model to bytes, model to bytes to model), hand-built documents, and hostile
  input.
- One test rewrites real files. Set `SOURCE2_RESOURCE_SAMPLES` to a directory; every file
  under it whose name ends in `_c` is parsed, written back, and compared byte for byte.
  The test does nothing when the variable is unset.

## License

GPL-3.0-or-later. See `LICENSE.md`.
