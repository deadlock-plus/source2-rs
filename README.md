# source2-rs

Rust readers for Valve's Source 2 file formats.

Read-only for now: the crates parse existing files and do not write them.

## Crates

| Crate | Purpose |
| --- | --- |
| `source2-vpk` | Reader for Valve Pak (VPK) archives |
| `source2-resource` | Reader for Source 2 compiled-resource containers |
| `source2-kv3` | Reader for Valve's binary KeyValues 3 (KV3) |

`source2-kv3` has two features, both on by default:

- `lz4`: LZ4 block decompression
- `zstd`: Zstandard decompression

Both are pure Rust. The workspace forbids `unsafe` code.

## Build and test

Minimum supported Rust version: 1.88.

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo fmt --all --check
```

Build `source2-kv3` with a single compression feature:

```sh
cargo test -p source2-kv3 --no-default-features --features lz4
cargo test -p source2-kv3 --no-default-features --features zstd
```

`rust-toolchain.toml` pins the development toolchain so formatting output is stable.

## Licence

GPL-3.0-or-later. See [LICENSE.md](LICENSE.md).
