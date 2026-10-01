# source2-vpk

Reader and writer for Valve Pak (VPK) archives, the container format used by Source and
Source 2 games.

- One document type, `Vpk`, for reading, writing and editing.
- Parsing keeps everything the format stores. Writing a parsed document gives back the same
  directory file bytes.
- Reads single files out of large multi-archive packs without touching the rest.
- Builds packs from scratch, including preloads and size-limited archive splitting.
- Version 2 integrity data (archive MD5 records, directory digests, RSA key and signature)
  is exposed as plain types.
- No dependencies, no `unsafe`, no feature flags.

## Install

```toml
[dependencies]
source2-vpk = "0.1"
```

Minimum supported Rust version: 1.88.

## Concepts

- A pack is a directory file, `pack_dir.vpk`, plus numbered archives, `pack_000.vpk` upward.
- The directory file holds the whole file tree. Each file records which archive holds its
  bytes, at what offset, and its CRC32.
- A pack can also keep file bytes inside the directory file itself ("inline" data). Such a
  pack is usually named without `_dir`.
- A file can keep its first bytes in the tree as a "preload". Reading a file returns the
  preload followed by the rest.
- Paths use forward slashes: `docs/readme.txt`.

## Read

`Vpk::open` parses only the directory file. Numbered archives are opened when an entry in
them is read.

```no_run
use source2_vpk::Vpk;

let vpk = Vpk::open("pack_dir.vpk")?;

for entry in vpk.find_by_extension("txt") {
    println!("{} ({} bytes)", entry.path, entry.size());
}

let bytes = vpk.read_path("docs/readme.txt")?;
# let _ = bytes;
# Ok::<(), source2_vpk::Error>(())
```

Lookups: `find` (exact path), `find_by_extension`, `entries_under` (directory prefix) and
`entries` (everything, in directory order).

```
use source2_vpk::{Entry, Vpk};

# let mut pack = Vpk::new(2);
# pack.push(Entry::inline("docs/readme.txt", b"hello".to_vec()));
# pack.push(Entry::inline("docs/guide/intro.md", b"# intro".to_vec()));
# pack.push(Entry::inline("data/table.txt", b"1,2,3".to_vec()));
# let vpk = Vpk::parse(&pack.to_bytes()?)?;
assert_eq!(vpk.find_by_extension("txt").count(), 2);
assert_eq!(vpk.entries_under("docs").count(), 2);
assert_eq!(vpk.read_path("docs/readme.txt")?, b"hello");
# Ok::<(), source2_vpk::Error>(())
```

`read` and `read_path` check the entry's CRC32. `read_with_crc` returns the bytes and the
verdict instead of failing:

```
use source2_vpk::{Entry, Vpk};

# let mut pack = Vpk::new(2);
# pack.push(Entry::inline("a.txt", b"data".to_vec()));
# let vpk = Vpk::parse(&pack.to_bytes()?)?;
let entry = vpk.find("a.txt").unwrap();
let contents = vpk.read_with_crc(entry)?;
assert!(contents.crc_ok());
assert_eq!(contents.bytes, b"data");
# Ok::<(), source2_vpk::Error>(())
```

A document parsed with `Vpk::parse` has no file on disk behind it. Inline entries can be
read. For entries in numbered archives, say where the directory file is with
`Vpk::with_source`.

## Write and round trip

Build a pack from scratch, then write it. `Entry::inline` stores bytes in the directory
file. `Vpk::add` stores them in numbered archives.

```no_run
use source2_vpk::{Entry, Vpk};

let mut out = Vpk::new(2);
out.max_archive_size = Some(64 << 20); // split archives at 64 MiB
out.add("docs/readme.txt", b"hello".to_vec())?;
out.push(Entry::inline("docs/small.txt", b"tiny".to_vec()));
out.write("out/new_dir.vpk")?; // also writes out/new_000.vpk
# Ok::<(), source2_vpk::Error>(())
```

A pack that needs no numbered archives can be built in memory with `Vpk::to_bytes`.
`Vpk::build` returns the directory bytes and the new archives without touching disk.

```
use source2_vpk::{Entry, Vpk};

let mut pack = Vpk::new(2);
pack.push(Entry::inline("docs/readme.txt", b"hello".to_vec()));
let bytes = pack.to_bytes()?;

let again = Vpk::parse(&bytes)?;
assert_eq!(again.to_bytes()?, bytes); // byte-identical
assert!(again.verify_directory_md5()?.all_ok());
# Ok::<(), source2_vpk::Error>(())
```

Writing a parsed document elsewhere copies the numbered archives it refers to:

```no_run
use source2_vpk::Vpk;

Vpk::open("pack_dir.vpk")?.write("copy/pack_dir.vpk")?;
# Ok::<(), source2_vpk::Error>(())
```

## Edit

Entries can be added, removed and replaced. Parsed entries keep pointing at their original
bytes, so editing a large pack does not load it.

```
use source2_vpk::{Entry, Vpk};

# let mut pack = Vpk::new(2);
# pack.push(Entry::inline("docs/readme.txt", b"hello".to_vec()));
# pack.push(Entry::inline("docs/old.txt", b"stale".to_vec()));
# let mut vpk = Vpk::parse(&pack.to_bytes()?)?;
vpk.remove("docs/old.txt");
vpk.remove("docs/readme.txt");
vpk.push(Entry::inline("docs/readme.txt", b"hello, world".to_vec()));
vpk.push(Entry::inline("docs/new.txt", b"new".to_vec()));

let edited = Vpk::parse(&vpk.to_bytes()?)?;
assert_eq!(edited.read_path("docs/readme.txt")?, b"hello, world");
assert!(edited.find("docs/old.txt").is_none());
# Ok::<(), source2_vpk::Error>(())
```

Notes:

- `remove` drops exactly one entry per call, the first with that path. A pack may list a
  path twice (a `README.txt` twice, say), so `find` can still return another copy
  afterwards. Call `remove` until it returns `None` to drop every copy.
- `Vpk::add` rejects duplicate paths and bad paths at once. `Vpk::push` checks nothing until
  the document is written.
- Version 2 archive MD5 records are regenerated when the document holds new in-memory
  bytes. Directory digests are recomputed on every write.
- Changing a signed pack invalidates its signature. See the limitations below.
- Move a file's first bytes into the tree with `Entry::with_preload_len`.

## Error handling

Every fallible call returns `source2_vpk::Result<T>`. `Error` is `#[non_exhaustive]`, so
keep a wildcard arm.

```
use source2_vpk::{Error, Vpk};

match Vpk::parse(b"definitely not a pack") {
    Err(Error::NotAVpk { found }) => eprintln!("bad signature {found:#010x}"),
    Err(Error::UnsupportedVersion(v)) => eprintln!("version {v} not supported"),
    Err(Error::Malformed(why)) => eprintln!("damaged: {why}"),
    Err(other) => eprintln!("{other}"),
    Ok(_) => unreachable!(),
}
```

| Variant | Meaning |
| --- | --- |
| `NotAVpk` | Wrong signature. |
| `UnsupportedVersion` | Version other than 1 or 2. |
| `Malformed` | Tree or section cut short, or sizes that disagree with the file. |
| `InvalidName` | A name is not UTF-8. Use `open_lossy` / `parse_lossy` to replace bad bytes. |
| `ChecksumMismatch` | Entry bytes do not match the recorded CRC32. `read_with_crc` reports it instead. |
| `NotFound` | `read_path` found no such entry. |
| `InvalidInput` | The document cannot be laid out, or the request cannot be answered (for example, an archive is needed and the document has no source). |
| `Io` | A file could not be read or written. Carries the path. |

## Versions and verification

| Version | Status |
| --- | --- |
| 2 | Read and write. Checked against real Source 2 packs: every `_dir` pack and standalone inline pack in the test set parses and writes back to identical directory bytes, and the directory digests verify. |
| 1 | Read and write, from the format description. Covered by hand-assembled bytes only. |
| other | `Error::UnsupportedVersion`. Variants other engines built on the same signature are not supported. |

Integrity checks:

- `Vpk::verify_directory_md5` checks the tree, section and whole-file digests.
- `Vpk::verify_archive_md5` checks archive MD5 records against the archive bytes. It reads
  every byte the records cover, which on a large pack is the whole pack.
- Records are reported as matched, mismatched or unchecked.

## Limitations

- Archive MD5 records for numbered archives in shipped Source 2 packs tile each archive in
  1 MiB chunks, but their digests are not the MD5 of the archive bytes. They are kept,
  written back untouched and reported as unchecked. Inline-data records are verified.
- Records generated for hand-built documents are plain MD5 per 1 MiB chunk. This crate can
  check them. They are not compared against another tool's output.
- The RSA signature is kept and written back, never re-made. That needs the publisher's
  private key.
- The version 1 layout and the "classic" signature layout come from the format description
  only. No real sample was available.
- Names must be UTF-8. The lossy readers replace bad bytes, and the result does not write
  back to the same bytes.
- Offsets are 32-bit: one archive file holds at most 4 GiB, and a pack at most 32766
  numbered archives.
- Reads go through plain file seeks. There is no memory mapping and no async API.

## Tests against real packs

Tests that read real `.vpk` files look for a directory named by `SOURCE2_VPK_SAMPLES`
(searched two levels deep). They skip when it is unset.

```sh
SOURCE2_VPK_SAMPLES=/path/to/vpks cargo test -p source2-vpk --release -- --include-ignored
```

## License

GPL-3.0-or-later. See `LICENSE.md`.
