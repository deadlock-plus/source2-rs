# source2-kv3

Reader and writer for Valve's KeyValues 3 (KV3), the typed key-value format Source 2 uses for
data tables, configuration and the structured blocks inside compiled resources. Binary and
text, every revision from the original `VKV\x03` encoding to `KV3\x05`.

- Reads and writes binary KV3: `VKV\x03`, `KV3\x01` to `KV3\x05`.
- Reads and writes text KV3 (`<!-- kv3 ... -->`).
- Payloads stored, LZ4, zstd, or (`VKV\x03` only) Valve's own block scheme.
- One `Document` type: a root `Value` plus the settings that decide how it is encoded.
- Parse, edit and write back. The decompressed payload of a parsed document is reproduced byte
  for byte, and the value tree always is.

## Install

```toml
[dependencies]
source2-kv3 = "0.1"
```

Both codecs are on by default. See [Features](#features-and-dependencies) to build without one.

## Usage

Building a document by hand and writing both forms:

```rust
use source2_kv3::{Document, Object, Value, flag};

let mut config = Object::new();
config.insert("name", "example");
config.insert("count", 3);
config.insert("scale", 0.5);
config.insert("model", Value::from("a.vmdl").with_flags(flag::RESOURCE));
config.insert("tags", vec![Value::from("a"), Value::from("b")]);

let doc = Document::new(config);        // v5, generic format, LZ4
let bytes = doc.to_bytes()?;
let text = doc.to_text()?;

assert_eq!(Document::from_bytes(&bytes)?.root, doc.root);
assert_eq!(Document::from_text(&text)?.root, doc.root);
# Ok::<(), source2_kv3::Error>(())
```

Reading a block, editing it and writing it back in the revision it came in:

```rust
use source2_kv3::{Document, Kind, Object, Value};

# let mut o = Object::new();
# o.insert("count", 3);
# o.insert("old", "x");
# let block = Document::new(o).to_bytes()?;
let mut doc = Document::from_bytes(&block)?;          // any revision

let root = doc.root.as_object_mut().expect("an object");
root.remove("old");
root.insert("added", true);
if let Some(count) = root.get_mut("count") {
    *count.kind_mut() = Kind::Int(300);
}

let rewritten = doc.to_bytes()?;                      // same revision, compression, format
assert_eq!(Document::from_bytes(&rewritten)?.root.get("count").and_then(Value::as_i64), Some(300));
# Ok::<(), source2_kv3::Error>(())
```

Choosing the encoding:

```rust
use source2_kv3::{Compression, Document, Object, Version, WriteOptions};

let doc = Document::new(Object::from_iter([("k", 1)]));
let options = WriteOptions {
    version: Version::V4,
    compression: Compression::None,
    ..WriteOptions::default()
};
let bytes = doc.to_bytes_with(&options)?;
# Ok::<(), source2_kv3::Error>(())
```

Looking inside a block without parsing the value tree:

```rust
use source2_kv3::{Document, Header, Object, decode};

# let block = Document::new(Object::new()).to_bytes()?;
let header = Header::parse(&block)?;       // revision, compression, sizes
let decoded = decode(&block)?;             // payload with compression undone
assert_eq!(header.version, decoded.header.version);
# Ok::<(), source2_kv3::Error>(())
```

Reading text KV3, which keeps the header's GUIDs and `"""` strings:

```rust
use source2_kv3::{Document, Kind};

let doc = Document::from_text(r#"<!-- kv3 encoding:text:version{e21c7f3c-8a33-41c5-9977-a76d3a32aa0d} format:generic:version{7412167c-06e9-4698-aff2-e63eb59037e7} -->
{
	name = "example"
	big = 0xFFFFFFFFFFFFFFFF
	model = resource:"models/a.vmdl"
	note = """
two lines
"""
}
"#)?;

let root = doc.root.as_object().expect("an object");
assert_eq!(root["name"].as_str(), Some("example"));
assert_eq!(root["big"].kind(), &Kind::UInt(u64::MAX));
assert!(root["note"].is_multiline());
# Ok::<(), source2_kv3::Error>(())
```

Handling errors. `Error` is `#[non_exhaustive]`, so keep a wildcard arm:

```rust
use source2_kv3::{Document, Error};

match Document::from_bytes(b"not a kv3 block") {
    Ok(_) => unreachable!(),
    Err(Error::Malformed(why)) => eprintln!("bad binary: {why}"),
    Err(Error::Compression(why)) => eprintln!("codec problem: {why}"),
    Err(other) => eprintln!("{other}"),
}

match Document::from_text("{ a = ") {
    Err(Error::Syntax { line, .. }) => assert_eq!(line, 1),
    other => panic!("expected a syntax error, got {other:?}"),
}
```

## Binary revisions

| Revision | Magic | Read | Write | Checked against |
|---|---|---|---|---|
| `Version::Legacy` | `VKV\x03` | yes | yes | sample files: stored by block flag, LZ4, block-compressed |
| `Version::V1` | `KV3\x01` | yes | yes | sample files (LZ4) |
| `Version::V2` | `KV3\x02` | yes | yes | nothing: no sample exists, layout inferred |
| `Version::V3` | `KV3\x03` | yes | yes | nothing: no sample exists, laid out as v4 |
| `Version::V4` | `KV3\x04` | yes | yes | files from a shipped Source 2 title (LZ4, zstd) |
| `Version::V5` | `KV3\x05` | yes | yes | files from a shipped Source 2 title (LZ4, zstd, blobs) |

`Compression::Block` is Valve's own byte-oriented scheme and exists only for `Legacy`. Pairings
no file is known to use (`Legacy` with zstd, a numbered revision with `Block`) are refused with
`Error::Unsupported`. `Compression::Unknown(n)` is what a header with an unrecognised method
reads as; decoding it is `Error::Unsupported` and it cannot be written.

## Round trips

`parse` followed by `Document::to_bytes` gives back:

- the same value tree, always;
- the same **decompressed payload**, byte for byte, on every sample the real-data tests were
  run against;
- for LZ4 files, usually the same **whole block**: the LZ4 encoder is a port of the reference
  high-compression encoder, and the buffers of the sample files re-encode to identical bytes.
  This is measured on those samples and is not a guarantee for every file.

What makes that possible is stored in the public types. A `Value` remembers how each number,
array and multi-line string was stored (integer width, float against double, typed against
general arrays, `"""` against a quoted string) and a `Document` remembers its revision,
compression, format GUID, dictionary id and frame size. Objects keep member order and repeated
keys. The remembered storage is a preference: the writer follows it only while it still holds
the value, so editing a parsed tree never produces a block that misstates it. See the `Value`
docs for the edit rules, `clear_storage` to ask for the default encoding, and `Value::float`
for 4-byte floats.

Not reproduced:

- zstd output matches Valve's size but not its bytes;
- block-scheme output decompresses to Valve's stream but uses a different match finder, so the
  compressed bytes differ;
- the LZ4 blob chunk-size table can differ in a few files with blobs;
- header fields that describe the compressed form (sizes, counts) are derived, not copied.

Hand-built values are written the way Valve's files usually are.

## Text

`parse_text` and `Document::to_text` read and write the text form. The header's encoding and
format entries (`name:version{guid}`) are kept in the `Document`, so a text file's format
survives a trip through binary and back.

Text KV3 keeps: values, member order, repeated keys, flag prefixes (`resource:"..."` and the
other four), blobs (`#[ 01 FF ]`), unsigned integers (written as `0x...` so they read back
unsigned) and `"""` multi-line strings.

Text KV3 drops or refuses:

- comments are read past and not kept;
- integer widths, float widths and array layouts have no text form;
- a flag bit outside the five with a text spelling is `Error::Invalid` on writing;
- an unknown flag prefix is `Error::Syntax` on reading;
- NaN and infinite doubles have no spelling and are `Error::Invalid`, and `nan` / `inf` are
  refused on reading;
- a `Value::multiline` string that contains `"""` or ends in a carriage return is written
  quoted, since it cannot be written raw unchanged.

## Features and dependencies

| Feature | Default | Effect |
|---|---|---|
| `lz4` | yes | LZ4 reading through `lz4_flex` (safe decoder). LZ4 writing is a built-in port of the reference high-compression encoder (level 12) and needs no dependency. |
| `zstd` | yes | zstd reading through `ruzstd`, writing through `zstd-rs` (level 7 with content checksum and size, as Valve's files use) |

Valve's block scheme is read and written by built-in code, and needs no feature. The crate is
pure Rust with `forbid(unsafe_code)`; MSRV 1.88.

```toml
source2-kv3 = { version = "0.1", default-features = false, features = ["lz4"] }
```

Without a codec's feature, a block that needs it fails with `Error::Compression`.

## What is not verified

- `KV3\x02` and `KV3\x03` have no sample file. They are written from the layout the reader
  infers: v2 as v1 with the dictionary id and frame size in the header, v3 as v4. LZ4 level for
  them follows the other revisions.
- The `VKV\x03` encoding that stores its stream uncompressed names itself with a GUID recalled
  from memory; no sample uses it.
- When the block scheme falls back to a stored stream (a tiny document), the cut-off used is a
  guess that matches the one sample.
- No rewritten file has been loaded by an engine; checks are re-parsing and payload comparison.

## Errors

`Error` is `#[non_exhaustive]`: `Malformed` (bad binary), `Syntax` (bad text, with a line),
`Invalid` (a value the output cannot store), `Unsupported` (a revision or pairing this crate
does not handle) and `Compression` (a codec failed or is not built in).

## Real-file tests

`cargo test -p source2-kv3` runs a rewrite test over real files only when
`SOURCE2_KV3_SAMPLES` names a directory of sample files (VPK archives or bare blocks), or
`SOURCE2_KV3_CORPUS` a directory of extracted `*.bin` blocks. Unset, the test returns at once.
`SOURCE2_KV3_FAST` and `SOURCE2_KV3_VERBOSE` trim and widen its output.

## Main API

`Document`, `Tag`, `TextHeader`, `parse`, `parse_text`, `write`, `write_text`, `decode`,
`Decoded`, `Header`, `Version`, `Compression`, `WriteOptions`, `Value`, `Kind`, `Object`, `flag`,
`Error`, `Result`, `GENERIC_FORMAT`, `TEXT_ENCODING`, and the `MAGIC_*` constants.

## License

GPL-3.0-or-later. See `LICENSE.md`.
