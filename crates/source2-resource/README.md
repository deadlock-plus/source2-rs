# source2-resource

Reader for Source 2 compiled-resource containers (`.vdata_c`, `.vtex_c`, `.vmdl_c`).

- Every compiled resource shares one envelope: a header plus a table of four-character blocks.
- This crate parses that envelope and hands back the raw bytes of each block.
- It borrows the input, so nothing is copied.
- Decoding block contents is left to other crates, such as `source2-kv3` for `vdata_c` data.

## Features

None. The crate has no feature flags and no dependencies.

## Usage

```rust,no_run
use source2_resource::{Block, Resource};

let bytes = std::fs::read("heroes.vdata_c")?;
let resource = Resource::parse(&bytes)?;

for block in resource.blocks() {
    println!("{} at {} ({} bytes)", block.name(), block.offset, block.length);
}

let data: &[u8] = resource.data()?;
let edit_info = resource.block(Block::RED2);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Status

- Read-only today.
- A writer is planned.

## License

GPL-3.0-or-later. See `LICENSE.md`.
