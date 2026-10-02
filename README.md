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

## Limitations

The crates read and write the files described below. Anything outside that returns an error. The crates do not skip it silently. Each crate's README has the full list.

### All crates

- Only one rewritten file has been loaded by a game. It was a KeyValues 3 file (version 5, zstd) that Deadlock accepted and saved again. No output from any other crate or KeyValues 3 version has been loaded by an engine.
- The tests compare against files from a Deadlock and a SteamVR install. Formats and versions with no sample file are written from the format description, and are marked below.
- `source2-kv1`, `source2-kv2`, `source2-kv3` and `source2-resource` parse from a byte slice or string, so the whole input is in memory. None of them reads from `std::io::Read`. Only `source2-resource` writes to `std::io::Write`. The others return a `Vec` or `String`.
- `source2-vpk` reads the directory file whole. It reads each entry from its archive with a file seek, and returns that entry as one `Vec`.

### `source2-vpk`

- Version 2 is tested against real packs. Version 1 and the classic signature layout come from the format description only.
- Other versions return `UnsupportedVersion`.
- MD5 records for numbered archives in shipped packs do not match the archive bytes. The crate keeps them, writes them back unchanged, and reports them as unchecked.
- The RSA signature is kept and written back. The crate cannot create a new one without the publisher's private key.
- Names must be UTF-8.
- Offsets are 32-bit, so one archive file holds at most 4 GiB.

### `source2-resource`

- The crate handles the container only. It stores every block as opaque bytes and decodes none of them, including `DATA`, `RERL`, `RED2`, `REDI` and `NTRO`.
- Only header version 12 is supported. Every file in the test set uses it.
- Tables with overlapping or out-of-order blocks return `BadLayout` and are not written.
- Writing a file over 4 GiB returns `TooLarge`.

### `source2-kv1`

- UTF-16 text returns `UnsupportedEncoding`.
- Binary types 9 and 11, and the string-table form of `appinfo.vdf`, return `UnsupportedType`.
- A bare `=` between a key and a value, as in some Steam client `.res` and `.menu` files, is not KeyValues 1. The reader treats it as part of the token.
- `/* */` comments are not supported.
- The crate evaluates nothing. Conditional tags, `#include` and `#base` stay in the tree as written.

### `source2-kv2`

- Binary encoding versions 1 to 5 and 9 work. Versions 6 to 8, and versions above 9, return `UnsupportedVersion`.
- `keyvalues2_flat` returns `UnsupportedEncoding`.
- Binary versions 1 to 3, and text encoding versions above 1, have no sample file.

### `source2-kv3`

- The crate rewrites compressed data with different bytes than Valve's tools produce. The content matches after parsing, but the file bytes differ.
- Versions 2 and 3 of the binary header, version 3 blobs, and the stored `VKV\x03` encoding have no sample file. The crate writes them from the layout it infers from other versions.
- The cut-off for storing a very small document uncompressed is a guess that matches one sample.
- Version 4 zstd blob handling comes from reverse engineering a few files.

### Formats not covered

These formats have no crate:

- Payloads inside compiled resources: `vsnd_c`, `vtex_c`, `vpcf_c`, `vmat_c`, `vmdl_c`, `vnmclip_c`, and panorama `_c` files
- Shader files (`.vcs`)
- Protobuf files (`.stats`, `.soc`)
- `.fgd` and `.vfont` files

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
