# source2

One dependency for the Source 2 file formats. It re-exports the format crates behind
features and adds a few helpers that compose them: format detection, one error type, and
extension traits for moving documents in and out of packs and resources.

| Module | Crate | Format |
|---|---|---|
| `source2::vpk` | `source2-vpk` | Valve Pak archives, versions 1 and 2 |
| `source2::resource` | `source2-resource` | Compiled-resource container, header version 12 |
| `source2::kv1` | `source2-kv1` | KeyValues 1, text and binary |
| `source2::kv2` | `source2-kv2` | KeyValues 2 / DMX, text and binary |
| `source2::kv3` | `source2-kv3` | KeyValues 3, text and binary v1 to v5 |

## Install

```toml
[dependencies]
source2 = "0.1"
```

All formats and both KV3 codecs are on by default. Pick a subset:

```toml
[dependencies]
source2 = { version = "0.1", default-features = false, features = ["vpk", "kv3", "lz4"] }
```

| Feature | Effect |
|---|---|
| `vpk`, `resource`, `kv1`, `kv2`, `kv3` | Enables that module. |
| `lz4` | Enables `kv3` and its LZ4 codec. |
| `zstd` | Enables `kv3` and its zstd codec. |

Without a KV3 codec, a block that needs it fails with a compression error. Everything is
pure Rust with no `unsafe`.

## VPK

```
use source2::vpk::{Entry, Vpk};

let mut pack = Vpk::new(2);
pack.push(Entry::inline("docs/readme.txt", b"hello".to_vec()));
let bytes = pack.to_bytes()?;

let again = Vpk::parse(&bytes)?;
assert_eq!(again.read_path("docs/readme.txt")?, b"hello");
assert_eq!(again.to_bytes()?, bytes);
# Ok::<(), source2::vpk::Error>(())
```

From disk, `Vpk::open("pack_dir.vpk")` reads the directory file and opens numbered
archives on demand.

## Resource

```
use source2::resource::{Block, BlockKind, HEADER_VERSION, Resource, Versions};

let resource = Resource::new()
    .with_versions(Versions::new(HEADER_VERSION, 1))
    .with_block(Block::new(BlockKind::DATA, b"payload".to_vec()));

let bytes = resource.to_bytes()?;
assert_eq!(Resource::parse(&bytes)?, resource);
# Ok::<(), source2::resource::Error>(())
```

## KeyValues 1

```
use source2::kv1::Document;

let doc = Document::parse(r#""Settings" { "name" "demo" "volume" "7" }"#)?;
let settings = doc.get("settings").expect("root exists");
assert_eq!(settings.get_str("name"), Some("demo"));
assert_eq!(settings.get_int("volume"), Some(7));

let text = doc.to_text()?;
assert_eq!(Document::parse(&text)?.to_text()?, text);
# Ok::<(), source2::kv1::Error>(())
```

## KeyValues 2

```
use source2::kv2::Document;

let src = b"<!-- dmx encoding keyvalues2 1 format dmx 1 -->
";
let doc = Document::parse(src)?;
assert_eq!(doc.to_bytes()?, src);
# Ok::<(), source2::kv2::Error>(())
```

## KeyValues 3

```
use source2::kv3::{Document, Object};

let doc = Document::new(Object::from_iter([("count", 3)]));

let bytes = doc.to_bytes()?;
assert_eq!(Document::from_bytes(&bytes)?.root, doc.root);

let text = doc.to_text()?;
assert_eq!(Document::from_text(&text)?.root, doc.root);
# Ok::<(), source2::kv3::Error>(())
```

## Detecting a format

`source2::detect` looks at the leading bytes and names the format, or says `Unknown`. It
needs no feature and parses no body.

```
use source2::{Format, detect};

assert_eq!(detect(b"<!-- dmx encoding keyvalues2 1 format dmx 1 -->
"), Format::Kv2Text);
assert_eq!(detect(b"no magic here"), Format::Unknown);
```

It tells apart VPK directory files, compiled resources, KeyValues 2 text and binary, and
KeyValues 3 text and binary. KeyValues 1 has no magic in either encoding, so it is never
guessed: its files are `Unknown`.

## Composing the crates

The traits in `source2::ext` add `read_*` and `put_*` methods to the foreign `Vpk` and
`Resource` types. Each trait exists only when the features it needs are on:

| Trait | On | Needs |
|---|---|---|
| `ResourceKv3` | `Resource` | `resource`, `kv3` |
| `VpkResource` | `Vpk` | `vpk`, `resource` |
| `VpkKv1`, `VpkKv2`, `VpkKv3` | `Vpk` | `vpk` and that format |
| `VpkResourceKv3` | `Vpk` | `vpk`, `resource`, `kv3` |

Edit the KeyValues 3 data of a resource inside a pack. Only that block changes; the other
blocks, the versions, the padding and every other entry stay as they were.

```
use source2::ext::{VpkResource, VpkResourceKv3};
use source2::kv3::{Document, Object};
use source2::resource::{Block, BlockKind, Resource};
use source2::vpk::Vpk;

let doc = Document::new(Object::from_iter([("count", 1)]));
let resource = Resource::new().with_block(Block::new(BlockKind::DATA, doc.to_bytes()?));
let mut pack = Vpk::new(2);
pack.put_resource("things/a.thing_c", &resource)?;

let mut data = pack.read_resource_kv3("things/a.thing_c", BlockKind::DATA)?;
data.root.as_object_mut().expect("an object").insert("count", 2);
pack.put_resource_kv3("things/a.thing_c", BlockKind::DATA, &data)?;

let again = pack.read_resource_kv3("things/a.thing_c", BlockKind::DATA)?;
assert_eq!(again.root.get("count").and_then(|v| v.as_i64()), Some(2));
# Ok::<(), source2::Error>(())
```

A `put_*` on a pack adds the entry or replaces the one at that path: every entry with the
path is removed and one is added, in the same place (inline or numbered archive) as the
first old one. A new path is bound for the numbered archives like `Vpk::add`, so write the
pack with `Vpk::write` or `Vpk::build`; `Vpk::to_bytes` fails until nothing needs an
archive. Where a format has two encodings (KeyValues 1 and 3), there are two named methods
and nothing is chosen from the file extension.

## Errors

`source2::Error` wraps the error of whichever crate failed (`Error::Vpk`,
`Error::Resource`, `Error::Kv1`, `Error::Kv2`, `Error::Kv3`, present with their features),
implements `source()`, and has `From` for each. Its own variants are `EntryNotFound`,
`BlockNotFound`, `UnrecognisedFormat` and `NotUtf8`. It is `non_exhaustive`.

## Limitations

Each member crate's README lists what it reads and writes, which versions have sample files
behind them, and what is not verified. Read those before relying on a rarer revision.

## Minimum supported Rust version

1.88.

## License

Licensed under either of Apache License, Version 2.0 (`LICENSE-APACHE`) or the MIT license (`LICENSE-MIT`) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this work, as defined in the Apache-2.0 licence, is dual licensed as above, without any additional terms or conditions.
