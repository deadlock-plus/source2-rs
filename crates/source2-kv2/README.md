# source2-kv2

Reader and writer for Valve's `KeyValues2` / DMX (Datamodel eXchange), the format behind
`.dmx`, `.pcf` and Source 2 `.vmap` files.

- Text: `keyvalues2` and `keyvalues2_noids`, any encoding version.
- Binary: encoding versions 1, 2, 3, 4, 5 and 9.
- One model for both: elements in an arena, referenced by index, with stable ids.
- Parse then write gives the same bytes. Everything a file stores beyond its elements
  (string table order, text whitespace, `id` and `name` positions) lives in public fields
  with defaults, so hand-built documents write in Valve's conventions.
- No dependencies, no `unsafe`.

## Install

```toml
[dependencies]
source2-kv2 = "0.1"
```

Minimum supported Rust version: 1.88. No feature flags, no dependencies.

## Read

`Document::parse` picks the text or binary reader from the header line.

```rust,no_run
use source2_kv2::{Document, Value};

let doc = Document::read_file("input.dmx")?;
if let Some(root) = doc.root() {
    println!("{} has {} attributes", root.class, root.attributes.len());
    if let Some(Value::Element(_)) = root.attribute("model") {
        println!("root has a model element");
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

From memory, text or binary:

```rust
use source2_kv2::{Document, ElementRef, Value};

let text = "<!-- dmx encoding keyvalues2 1 format dmx 1 -->
\"DmElement\"
{
	\"id\" \"elementid\" \"8aa7f40b-f824-4431-aea0-0f5e40cfc8b5\"
	\"name\" \"string\" \"scene\"
	\"count\" \"int\" \"3\"
}

";
let doc = Document::parse(text.as_bytes())?;
let root = doc.root().unwrap();
assert_eq!(root.name, "scene");
assert_eq!(root.attribute("count"), Some(&Value::Int(3)));

// Follow an element reference.
if let Some(Value::Element(ElementRef::Element(id))) = root.attribute("child") {
    println!("{}", doc.element(*id).unwrap().class);
}
# Ok::<(), source2_kv2::Error>(())
```

References to an id no element has parse as `ElementRef::External` and write back
unchanged. Call `Document::check_references` to make them an error.

## Write

`to_bytes` and `write_file` use the document's own encoding and version. A parsed document
writes back as it was read.

```rust
use source2_kv2::Document;

let src = b"<!-- dmx encoding keyvalues2 1 format dmx 1 -->
";
let doc = Document::parse(src)?;
assert_eq!(doc.to_bytes()?, src);
# Ok::<(), source2_kv2::Error>(())
```

## Build

```rust
use source2_kv2::{Document, Element, Encoding};

let mut doc = Document::with_encoding(Encoding::Binary, 5, "dmx", 1);
let root = doc.add_element(Element::new("Scene").name("demo").attr("version", 3));
doc.add_child(root, "camera", Element::new("Camera").attr("fov", 90.0f32));
doc.push_child(root, "items", Element::new("Item").attr("label", "a"));
let bytes = doc.to_bytes()?;
assert_eq!(Document::parse(&bytes)?.elements.len(), 3);
# Ok::<(), source2_kv2::Error>(())
```

- `Document::new(format, version)` makes an empty text document; `with_encoding` picks the
  encoding and its version; `default()` is an empty `dmx` 1 text document.
- `Element::new(class)` has a generated id. Chain `name`, `id`, `attr` (set) and `push`
  (append to an array attribute).
- `Document::add_element`, `add_child` (element-valued attribute), `push_child` (element
  array), `link` and `push_link` (point at an existing element).
- `Value` converts from `i32`, `f32`, `bool`, `&str`, `String`, `Vec<u8>`, `u8`, `u64`,
  `[f32; 2|3|4|16]`, `ElementId`, `ElementRef`, `Color`, `Time`. Use `Value::qangle`,
  `Value::quaternion` and `Value::array` for the types a conversion cannot pick.
- `Uuid::from_u128`, `Uuid::parse` and `Uuid::generate` (std only, not for secrets) make ids.

## Edit

Elements live in `Document::elements`; fields are public.

```rust
use source2_kv2::{Document, Element, Value};

let mut doc = Document::new("dmx", 1);
let root = doc.add_element(Element::new("DmElement").attr("count", 1));
doc.element_mut(root).unwrap().set("count", 2);
doc.root_mut().unwrap().name = "renamed".into();
assert_eq!(doc.root().unwrap().attribute("count"), Some(&Value::Int(2)));
let bytes = doc.to_bytes()?;
assert_eq!(Document::parse(&bytes)?, doc);
# Ok::<(), source2_kv2::Error>(())
```

## Errors

Every fallible call returns `source2_kv2::Result`. `Error` is `#[non_exhaustive]`:

- `BadHeader`, `UnsupportedEncoding`, `UnsupportedVersion { encoding, version }`: the
  first line is wrong or names something this crate does not handle.
- `Syntax { line, message }`: text body does not parse.
- `Malformed`: binary body is truncated or has an out-of-range count, index or type.
- `UnresolvedReference`, `DuplicateId`: from `check_references` and writing.
- `TooDeep`: nesting beyond the reader and writer limit.
- `InvalidModel`: the document cannot be written as built (wrong value type for the
  encoding, reserved names, and so on).
- `Io { path, source }`: from `read_file` and `write_file`.

```rust
use source2_kv2::{Document, Error};

let err = Document::parse(b"not a dmx file").unwrap_err();
assert!(matches!(err, Error::BadHeader(_)));
```

## Supported versions

- Text: `keyvalues2` and `keyvalues2_noids`, any encoding version.
- Binary: encoding versions 1, 2, 3, 4, 5 and 9. Versions 6, 7, 8 and above 9 are
  rejected with `UnsupportedVersion`.
- `keyvalues2_flat` is rejected with `UnsupportedEncoding`.

## Layout fields

| Field | Holds |
| --- | --- |
| `Document::string_table` | binary string table order; unused entries stay; new strings are appended |
| `Document::text_style` | line ending, blank lines, array spelling, float spelling |
| `Element::text` | position of `id` and `name` among the attributes; top-level vs nested |
| `Prefix::id` | prefix element id (text only) |

Elements are written where a reader would number them: a nested block is the next element
in the arena. Elements the root does not nest, and `standalone` ones, follow as top-level
blocks, so text and binary both write every element. Hand-built elements that are not in
nesting order are still written, but come back renumbered.

## Status

Verified against real files (read, write, byte compare, second parse equal):

| Kind | Files | Byte-identical |
| --- | --- | --- |
| binary 4 | 1 | 1 |
| binary 5 | 1 | 1 |
| binary 9 | 9 | 9 |
| keyvalues2 1 (two different serializers) | 2 | 2 |

- Text: CRLF and LF, `}` blank-line habits, `[ ]` inline arrays, `, ` separators, `id` and
  `name` anywhere, shared elements as top-level blocks, ten-decimal floats.
- Header: LF or CRLF line end, tabs or spaces between words, text header without a line
  end. Only LF binary and LF/CRLF text headers were seen in files.
- A UTF-8 byte order mark is rejected, since it cannot be written back.
- Binary 4 stores the string table count in 4 bytes and indices in 2; confirmed by a real
  file.
- `uint64` and `uint8` exist only in text and binary 9; `objectid` only in text and binary
  1 and 2; `time` only in text and binary 3 and up.

## Known gaps

- Binary versions 6, 7 and 8 are rejected with `UnsupportedVersion`. No public reader
  documents them and no sample was found, so any layout would be a guess. Versions above 9
  are rejected too.
- Binary 1, 2 and 3 are implemented from the Source 1 serializer and have no real sample;
  the byte-level tests there pin this crate's reading.
- Text float spelling: `Shortest` round-trips every `f32` except NaN payloads; `Fixed10`
  (chosen when a file shows more than 9 significant digits) matches Valve's output but
  rounds values below 5e-11.
- Text whitespace is modelled by the `TextStyle` habits above. A file with other habits
  (extra blank lines, other indents, comments) parses fully but writes back normalized.
- `keyvalues2_noids` has no ids, so parsing invents ids and shared elements are written
  once per use; parse-write is not byte-identical for documents that share elements.
- Binary booleans are any non-zero byte on read and `1` on write.
- The `keyvalues2_flat` encoding is rejected with `UnsupportedEncoding`. Text encoding
  versions other than 1 were not seen in files; the version number is kept and written back.

## Verification

`cargo test -p source2-kv2` runs the unit tests. Real-file checks need
`SOURCE2_KV2_SAMPLES`, one or more directories (separated like `PATH`) searched for DMX
files. They pass without checking anything when it is unset:

```text
SOURCE2_KV2_SAMPLES=/path/to/files cargo test -p source2-kv2 -- --nocapture
```

Each file is parsed, written and compared byte for byte, then parsed again and compared
to the first parse.

## License

GPL-3.0-or-later. See `LICENSE.md`.
