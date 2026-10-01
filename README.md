# source2-rs

Rust libraries for reading and writing Valve's Source 2 file formats.

Each crate covers one format end to end, the way a ZIP or RAR crate covers an archive:
parse a file, change it, write it back, and nothing the format stores is lost.

## Crates

| Crate | Format |
| --- | --- |
| `source2-vpk` | Valve Pak (VPK) archives |
| `source2-resource` | Compiled-resource containers (`*_c`) |
| `source2-kv1` | KeyValues 1, text and binary |
| `source2-kv2` | KeyValues 2 / DMX, text and binary |
| `source2-kv3` | KeyValues 3, text and binary |

The crates are independent of each other. Each crate's README lists the versions it supports and
what has and has not been checked against real files.

`source2-kv3` has two features, both on by default:

- `lz4`: LZ4 compression
- `zstd`: Zstandard compression

All crates are pure Rust. The workspace forbids `unsafe` code.

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
