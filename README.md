# source2-rs

Rust libraries for reading and writing Valve's Source 2 file formats.

Each crate covers one format end to end, the way a ZIP or RAR crate covers an archive:
parse a file, change it, write it back, and nothing the format stores is lost.

## Crates

| Crate | Format |
| --- | --- |
| `source2` | Facade: re-exports all five behind features |
| `source2-vpk` | Valve Pak (VPK) archives |
| `source2-resource` | Compiled-resource containers (`*_c`) |
| `source2-kv1` | KeyValues 1, text and binary |
| `source2-kv2` | KeyValues 2 / DMX, text and binary |
| `source2-kv3` | KeyValues 3, text and binary |

The format crates do not depend on each other. `source2` re-exports them behind features.

Each crate's README lists the versions it supports and what has been checked against real files.

`source2-kv3` has two features, both on by default:

- `lz4`: LZ4 compression
- `zstd`: Zstandard compression

All crates are pure Rust. The workspace forbids `unsafe` code.

## Use as a dependency

The crates are not on crates.io. Add them as git dependencies and pin a commit:

```toml
[dependencies]
source2 = { git = "https://github.com/deadlock-plus/source2-rs", rev = "<commit>" }
```

Pick one format crate instead of the facade to skip the others:

```toml
source2-vpk = { git = "https://github.com/deadlock-plus/source2-rs", rev = "<commit>" }
```

Turn off default features to choose formats and compression:

```toml
source2 = { git = "https://github.com/deadlock-plus/source2-rs", rev = "<commit>", default-features = false, features = ["vpk", "kv3", "zstd"] }
```

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

`rust-toolchain.toml` pins the development toolchain so `rustfmt` output does not change between machines.

## Licence

Licensed under either of the Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE)) or the MIT license ([`LICENSE-MIT`](LICENSE-MIT)), at your option.

The kv3 binary reader is ported from ValveResourceFormat (MIT). See [`THIRD-PARTY.md`](THIRD-PARTY.md).
