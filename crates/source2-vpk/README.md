# source2-vpk

Reader for Valve Pak (VPK) archives, as used by Source 2 games.

- Parses only the `*_dir.vpk` directory to locate files.
- Pulls single entries out of large multi-archive paks without reading the rest.
- Lookup by exact path, extension or directory prefix.

## Features

None. The crate has no feature flags and no dependencies.

## Usage

```rust,no_run
use source2_vpk::Vpk;

let vpk = Vpk::open("pak01_dir.vpk")?;

for entry in vpk.find_by_extension("vdata_c") {
    println!("{} ({} bytes)", entry.path, entry.size());
}

let bytes = vpk.read_path("scripts/heroes.vdata_c")?;
# Ok::<(), source2_vpk::Error>(())
```

Main API: `Vpk::open`, `Vpk::parse`, `entries`, `find`, `find_by_extension`,
`entries_under`, `read`, `read_path`.

Compiled resources inside an archive are handled by `source2-resource` and `source2-kv3`.

## Status

- Read-only today.
- A writer is planned.

## License

GPL-3.0-or-later. See `LICENSE.md`.
