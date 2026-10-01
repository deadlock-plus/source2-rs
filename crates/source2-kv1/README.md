# source2-kv1

Reader and writer for Valve's KeyValues 1 (KV1), the format of `gameinfo.gi`, `.vcfg` and
`.vdf` files. Text and binary.

- Ordered tree; duplicate keys and multiple roots are kept.
- Conditional tags (`[$WIN32]`, `[!$X360]`) are kept per entry, never evaluated.
- `#include` / `#base` are reported as directives; no file is opened.
- Writes Valve layout: tab indent, `"key"<TAB><TAB>"value"`.
- Nesting depth is capped (default 128), so hostile input is an error, not a stack overflow.

## Features

None. The crate has no feature flags and no dependencies.

## Usage

```rust,no_run
use source2_kv1::{Document, Entry};

let doc = Document::parse(&std::fs::read_to_string("gameinfo.gi")?)?;
let game = doc.get("GameInfo").and_then(|e| e.get_str("game"));

let out = Document {
    directives: vec![],
    roots: vec![Entry::section("boot", vec![Entry::string("UILanguage", "english")])],
};
std::fs::write("boot.vcfg", out.to_text()?)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Main API: `Document::{parse, to_text, from_binary, to_binary}` (plus `_with` forms taking
`Options`), `Entry::{get, get_all, get_str, get_int, get_float, get_bool}`.

Backslash escapes (`\n \t \ \"`) are on by default; other `\x` is kept as written. Set
`Options::escape_sequences` to `false` for the engine's default of none.

## Status

- Text read and write: complete for the documented syntax. `/* */` comments are not KV1.
- Binary read and write: types 0 to 8. The appinfo variant that stores key names in a string
  table is not supported.

## License

GPL-3.0-or-later. See `LICENSE.md`.
