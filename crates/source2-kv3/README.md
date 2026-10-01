# source2-kv3

Reader for Valve's binary KeyValues 3 (KV3), the payload format inside `vdata_c` files.

- Reads binary KV3 versions 4 and 5.
- Handles uncompressed, LZ4 and zstd payloads.
- Produces a `Value` tree with typed accessors.

## Features

| Feature | Default | Effect |
|---|---|---|
| `lz4` | yes | LZ4 read via `lz4_flex`, and write (high-compression, built in) |
| `zstd` | yes | zstd read via `ruzstd`, and write via `zstd-rs` |

Disable defaults to drop a codec you do not need:

```toml
source2-kv3 = { version = "*", default-features = false, features = ["lz4"] }
```

## Usage

The input is the `DATA` block of a compiled resource (see `source2-resource`).

```rust,no_run
use source2_kv3::parse;

let block: &[u8] = /* DATA block bytes */ &[];
let doc = parse(block)?;

if let Some(name) = doc.root.get("name").and_then(|v| v.as_str()) {
    println!("{name}");
}
# Ok::<(), source2_kv3::error::Error>(())
```

Main API: `parse`, `decode`, `Document`, `Value`, `Object`, `Header`, `Compression`.

## Status

- Read-only today.
- A writer is planned.

## License

GPL-3.0-or-later. See `LICENSE.md`.
