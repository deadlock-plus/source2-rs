# source2-kv2

Reader and writer for Valve's `KeyValues2` / DMX (Datamodel eXchange), the format behind
Source 1 `.pcf` and `.dmx` files and Source 2 `.vmap` sources.

- Text: `keyvalues2` and `keyvalues2_noids`, any encoding version.
- Binary: encoding versions 1, 2, 3, 4, 5 and 9.
- One model for both: elements in an arena, referenced by index, with stable ids.
- Element references are resolved by id; dangling ids are an error unless asked for.
- No dependencies, no `unsafe`.

## Features

None. The crate has no feature flags and no dependencies.

## Usage

```rust,no_run
use source2_kv2::{Document, Encoding, Value};

let mut doc = Document::read_file("map.vmap")?;
let root = doc.root().expect("a document has a root");
if let Some(Value::Int(build)) = root.attribute("editorbuild") {
    println!("{} built by editor {build}", root.class);
}

doc.encoding = Encoding::KeyValues2;
doc.encoding_version = 1;
std::fs::write("map.txt", doc.to_bytes()?)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Main API: `Document::parse`, `parse_with`, `read_file`, `to_bytes`, `write_file`; the model
types `Document`, `Element`, `Attribute`, `Value`, `ElementRef`.

## Status

- Text read and write: tested against hand-written documents and a shipped Source 1
  `keyvalues2` file.
- Binary version 9: read and re-write checked against a shipped Source 2 `.vmap`. The
  prefix section is byte-identical; the string table is rebuilt, so the file is smaller but
  the model is equal.
- Binary versions 1 to 5: implemented from the Source 1 serializer. No sample file was
  available, so these layouts are unverified against Valve's output.
- Binary versions 6 to 8 and above 9 are rejected with `UnsupportedVersion`.
- Unreachable elements are dropped when writing text; binary writes every element.
- `uint64` and `uint8` exist only in text and binary 9; `objectid` only in text and binary
  1 and 2; `time` only in text and binary 3 and up.

## License

GPL-3.0-or-later. See `LICENSE.md`.
