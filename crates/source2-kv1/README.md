# source2-kv1

Reader and writer for Valve's KeyValues 1 (KV1), the format of `.vdf`, `.acf`, `.vcfg` and
similar files. Text and binary. No dependencies, no feature flags, no `unsafe`.

- Ordered tree. Duplicate keys and multiple roots are kept.
- Lossless text round trip: parse then write gives back the same bytes. Comments, blank
  lines, spacing, line endings, quote style, escape spelling, directive position, condition
  position and the text encoding are kept in optional `layout` fields.
- Hand-built documents use Valve's layout: tab indent, `"key"<TAB><TAB>"value"`, brace on
  its own line.
- Conditional tags (`[$WIN32]`) are kept per entry, never evaluated.
- `#include` / `#base` are reported as directives; no file is opened.
- Nesting depth is capped (default 128), so hostile input is an error, not a stack overflow.

## Install

```toml
[dependencies]
source2-kv1 = "0.1"
```

Minimum supported Rust version: 1.88.

## Parse text

`parse_bytes` takes file contents and handles UTF-8 and Windows-1252. `parse` takes a `&str`.

```rust
use source2_kv1::Document;

let doc = Document::parse(r#"
"Settings"
{
    "name"   "demo"
    "volume" "7"
    "on"     "1"
    "Window" { "width" "1280" }
}
"#)?;

let settings = doc.get("settings").expect("root exists"); // keys match ASCII case-insensitively
assert_eq!(settings.get_str("name"), Some("demo"));
assert_eq!(settings.get_int("volume"), Some(7));
assert_eq!(settings.get_bool("on"), Some(true));
assert_eq!(settings.get("Window").and_then(|w| w.get_int("width")), Some(1280));
# Ok::<(), source2_kv1::Error>(())
```

From a file:

```rust,no_run
let bytes = std::fs::read("settings.vdf")?;
let doc = source2_kv1::Document::parse_bytes(&bytes)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Write text

```rust
use source2_kv1::{Document, Entry};

let doc = Document::new(vec![
    Entry::section("Settings", vec![Entry::string("name", "demo"), Entry::bool("on", true)])
        .with_comment(" generated"),
]);
assert_eq!(
    doc.to_text()?,
    "// generated\n\"Settings\"\n{\n\t\"name\"\t\t\"demo\"\n\t\"on\"\t\t\"1\"\n}\n"
);
// Bytes in the document's encoding, for writing files:
let _bytes: Vec<u8> = doc.to_text_bytes()?;
# Ok::<(), source2_kv1::Error>(())
```

## Edit in place

Reading then writing keeps everything you did not touch.

```rust
use source2_kv1::{Document, Entry};

let mut doc = Document::parse("// header\n\"Cfg\"\n{\n\t\"a\"\t\t\"1\"\n}\n")?;
let cfg = doc.get_mut("Cfg").expect("root exists");
cfg.get_mut("a").expect("key exists").value = "2".into();
cfg.push(Entry::string("b", "new"));
assert_eq!(
    doc.to_text()?,
    "// header\n\"Cfg\"\n{\n\t\"a\"\t\t\"2\"\n\t\"b\"\t\t\"new\"\n}\n"
);
# Ok::<(), source2_kv1::Error>(())
```

## Binary

Binary KV1 has types. Text does not.

```rust
use source2_kv1::{Document, Entry, Error};

let doc = Document::new(vec![Entry::section(
    "Root",
    vec![Entry::int("count", 5), Entry::color("tint", [255, 0, 0, 255])],
)]);
let bytes = doc.to_binary()?;
assert_eq!(Document::from_binary(&bytes)?, doc);

// Writing typed values as text is an error unless you convert on purpose.
assert!(matches!(doc.to_text(), Err(Error::TypedValueInText { .. })));
assert!(doc.stringified().to_text()?.contains("\"count\"\t\t\"5\""));
# Ok::<(), Error>(())
```

The binary writer rejects directives and conditions, which the format cannot hold.

## Error handling

`Error` is `#[non_exhaustive]`. Text errors carry a line and column, from 1.

```rust
use source2_kv1::{Document, Error};

match Document::parse("\"Root\"\n{\n\t\"a\" \"unterminated\n") {
    Err(Error::UnterminatedString { line, column }) => assert_eq!((line, column), (3, 6)),
    other => panic!("unexpected: {other:?}"),
}
```

## Options

```rust
use source2_kv1::{Document, Options};

let options = Options::new()
    .with_escape_sequences(false) // backslash is an ordinary character
    .with_max_depth(16);
let doc = Document::parse_with(r#""Path" { "dir" "C:\temp" }"#, &options)?;
assert_eq!(doc.get("Path").unwrap().get_str("dir"), Some(r"C:\temp"));
# Ok::<(), source2_kv1::Error>(())
```

`Options`, `Document`, `Entry`, `Directive`, `Layout`, `DocumentLayout` and `Spelling` are
`#[non_exhaustive]`: build them with `new` / `default` and the `with_*` methods, or assign to
their public fields after construction. Struct literals do not compile outside the crate, so
new fields are not a breaking change.

```rust
use source2_kv1::{Document, DocumentLayout, Entry, Layout, Options, Quote, Spelling};

let doc = Document::new(vec![
    Entry::string("a", "b").with_layout(Layout::new().with_key(Spelling::new().with_quote(Quote::Bare))),
])
.with_layout(DocumentLayout::new().with_bom(true));
let text = doc.to_text_with(&Options::new().with_max_depth(8))?;
assert!(text.starts_with("\u{feff}a"));
# Ok::<(), source2_kv1::Error>(())
```

## Supported syntax

- Quoted and bare tokens, `//` comments, nested sections, repeated keys, several roots.
- Escapes `\n \t \v \b \r \f \a \ \? \' \"`. Any other `\x` is kept as written. Escapes are
  on by default (Valve's engine default is off); the setting is stored in
  `Document::escapes` and the writer follows it. The exact spelling is kept, so `"a\?b"`
  stays `"a\?b"`.
- Conditional tags `[$WIN32]`, `[!$X360]` after the key or after the value or closing brace.
- `#include` / `#base` at the top level of a file. Inside a section they are ordinary keys.
- Encodings: valid UTF-8 is UTF-8, anything else is Windows-1252 (one character per byte,
  written back as the same bytes). A UTF-8 byte order mark is kept.
- Binary type bytes: `0` section, `1` string, `2` int, `3` float, `4` ptr, `5` wide string,
  `6` color, `7` uint64, `8` end, `10` int64. Whether the final end marker was present is
  kept, so files without it are rewritten without it.

## Known limitations

- `/* */` comments are not KV1 and are not supported.
- UTF-16 text is rejected (`Error::UnsupportedEncoding`).
- Binary types `9` and `11` and the string-table variant of `appinfo.vdf` are rejected
  (`Error::UnsupportedType`).
- Wide strings use a 16-bit count and 16-bit units. Lone surrogates are an error.
- Binary data after the final end marker is an error, not preserved.
- Text may end with one NUL byte, as Valve's `.res` files do. It is kept in
  `DocumentLayout::trailing_nul`. A NUL anywhere else is an error.
- A bare `=` between a key and value (`style="x"` in some `.res` and `.menu` files) is not KV1.
  It reads as part of the token and is not given special treatment.
- Nothing is evaluated: no condition evaluation, no include merging, no type guessing. An
  entry whose condition is false for you is still in the tree; filter it yourself.

## Verification

Real files, read-only, via tests in `src/real_tests.rs`. Set
`SOURCE2_KV1_SAMPLES` to a directory of sample files (searched recursively). Files with a
text extension (`vdf`, `vcfg`, `acf`, `txt`, `lst`, `gi`, `res`, `menu`, `cfg`) must parse
and write back byte-identical; files that are not KV1 text, or use a bare `=` separator, are
counted and skipped. Files with extension `bin` are checked as binary the same way. Without the variable the tests pass without checking anything.

```sh
SOURCE2_KV1_SAMPLES=/path/to/samples cargo test -p source2-kv1 real_ -- --nocapture
```

Checked this way against a Steam install: 1407 text files parsed and byte-identical (847
skipped as not KV1 text) and 191 binary files parsed and byte-identical (207 skipped as not
KV1 binary). Valve `.res` files with a trailing NUL are among them.

Not verified:

- No real binary file with typed values, wide strings, colors or int64. The binary layout
  and the meaning of type byte `10` are from memory.
- The rarer escapes (`\v \b \r \f \a \? \'`) are from memory of Valve's conversion table.
- Valve's handling of conditions beside directives and of nested includes.
- No file written by this crate has been loaded by the engine or Steam.

## License

GPL-3.0-or-later. See `LICENSE.md`.
