//! Reader for Valve Pak (VPK) archives.
//!
//! A VPK is a directory file, `*_dir.vpk`, plus numbered data archives, `*_000.vpk` upward.
//! The directory carries the whole file tree plus, for each entry, which archive holds its
//! bytes and where. Nothing but the directory has to be parsed to find a file, which is
//! what makes pulling one small entry out of tens of gigabytes cheap.
//!
//! ```no_run
//! use source2_vpk::Vpk;
//!
//! let vpk = Vpk::open("pak01_dir.vpk")?;
//! let bytes = vpk.read_path("scripts/heroes.vdata_c")?;
//! # Ok::<(), source2_vpk::Error>(())
//! ```
//!
//! Compiled resources inside an archive are the business of `source2-resource` and
//! `source2-kv3`.

#![forbid(unsafe_code)]

pub mod error;
mod md5;
mod writer;

pub use error::{Error, Result};
pub use writer::{BuiltVpk, VpkWriter};

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Magic number every VPK starts with.
pub const SIGNATURE: u32 = 0x55aa_1234;

/// `archive_index` value meaning "the bytes are in the directory file itself".
const ARCHIVE_INLINE: u16 = 0x7fff;

/// Terminator written after each directory entry.
const ENTRY_TERMINATOR: u16 = 0xffff;

/// One file inside an archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Full path inside the archive, e.g. `scripts/heroes.vdata_c`.
    pub path: String,
    /// CRC32 of the file's bytes, as the directory records it.
    pub crc: u32,
    /// Which numbered archive holds the bytes, or `None` when they are inline in the
    /// directory file.
    pub archive: Option<u16>,
    /// Offset of the bytes within that archive.
    pub offset: u32,
    /// How many bytes live in the archive, excluding [`Entry::preload`].
    pub length: u32,
    /// Bytes stored directly in the directory file, ahead of the rest.
    ///
    /// Small files are often stored entirely here, with `length` zero.
    pub preload: Vec<u8>,
}

impl Entry {
    /// Total size of the file: preload plus archived bytes.
    pub fn size(&self) -> usize {
        self.preload.len() + self.length as usize
    }
}

/// An opened VPK directory file.
///
/// Holds the parsed tree and the directory file's bytes; the numbered archives are opened
/// on demand, one read per [`Vpk::read`].
#[derive(Debug)]
pub struct Vpk {
    dir_path: PathBuf,
    /// Where the directory file's own data section starts, for inline entries.
    data_start: u64,
    version: u32,
    entries: Vec<Entry>,
    by_path: HashMap<String, usize>,
}

impl Vpk {
    /// Open a `*_dir.vpk` and parse its directory tree.
    ///
    /// # Errors
    ///
    /// If the file cannot be read, does not carry the VPK signature, is a version this
    /// reader does not handle, or has a tree that ends unexpectedly.
    pub fn open(dir_path: impl AsRef<Path>) -> Result<Self> {
        let dir_path = dir_path.as_ref().to_path_buf();
        let bytes = std::fs::read(&dir_path).map_err(|e| Error::Io {
            path: dir_path.clone(),
            source: e.to_string(),
        })?;
        Self::parse(&bytes, dir_path)
    }

    /// Parse a directory file already held in memory.
    ///
    /// `dir_path` is still needed: it is how the numbered archives beside it are found.
    ///
    /// # Errors
    ///
    /// As [`Vpk::open`], minus the read.
    pub fn parse(bytes: &[u8], dir_path: impl AsRef<Path>) -> Result<Self> {
        let mut r = Cursor::new(bytes);
        let signature = r.u32()?;
        if signature != SIGNATURE {
            return Err(Error::NotAVpk { found: signature });
        }
        let version = r.u32()?;
        let tree_size = r.u32()?;
        match version {
            1 => {}
            // v2 adds four section sizes after the tree size. Only the tree matters for
            // reading; the md5 and signature sections sit past the file data.
            2 => {
                for _ in 0..4 {
                    r.u32()?;
                }
            }
            v => return Err(Error::UnsupportedVersion(v)),
        }

        let tree_start = r.pos();
        let entries = parse_tree(&mut r, tree_start + tree_size as usize)?;
        let mut by_path = HashMap::with_capacity(entries.len());
        for (i, e) in entries.iter().enumerate() {
            by_path.insert(e.path.clone(), i);
        }

        Ok(Vpk {
            dir_path: dir_path.as_ref().to_path_buf(),
            data_start: (tree_start + tree_size as usize) as u64,
            version,
            entries,
            by_path,
        })
    }

    /// VPK format version of the directory file.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Every entry, in the order the directory lists them.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// How many files the archive holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the archive holds no files.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Look up an entry by its full path, e.g. `scripts/heroes.vdata_c`.
    pub fn find(&self, path: &str) -> Option<&Entry> {
        self.by_path.get(path).map(|&i| &self.entries[i])
    }

    /// Every entry whose path ends with `suffix`, e.g. `.vdata_c`.
    pub fn find_by_extension<'a>(&'a self, extension: &'a str) -> impl Iterator<Item = &'a Entry> {
        let dotted = extension.strip_prefix('.').unwrap_or(extension);
        self.entries
            .iter()
            .filter(move |e| e.path.rsplit('.').next() == Some(dotted))
    }

    /// Every entry below the directory `prefix`, at any depth, in directory order.
    ///
    /// `scripts` and `scripts/` are the same request; an empty prefix is the whole
    /// archive. The prefix is matched on a path boundary rather than as a bare string, so
    /// `scripts` never returns `scripts_old/heroes.vdata_c`, and a full file path names a
    /// file rather than a directory and so has nothing below it.
    pub fn entries_under<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = &'a Entry> {
        let dir = prefix.trim_end_matches('/');
        self.entries.iter().filter(move |e| {
            dir.is_empty()
                || (e.path.len() > dir.len()
                    && e.path.as_bytes()[dir.len()] == b'/'
                    && e.path.starts_with(dir))
        })
    }

    /// Read one entry's bytes, verifying the CRC the directory recorded.
    ///
    /// # Errors
    ///
    /// If the archive holding the bytes cannot be read, the offset runs past its end, or
    /// the CRC does not match.
    pub fn read(&self, entry: &Entry) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(entry.size());
        out.extend_from_slice(&entry.preload);

        if entry.length > 0 {
            let (path, offset) = match entry.archive {
                Some(n) => (self.archive_path(n), u64::from(entry.offset)),
                // Inline entries are offset from the end of the tree, not the file start.
                None => (
                    self.dir_path.clone(),
                    self.data_start + u64::from(entry.offset),
                ),
            };
            let chunk = read_at(&path, offset, entry.length as usize)?;
            out.extend_from_slice(&chunk);
        }

        let found = crc32(&out);
        if found != entry.crc {
            return Err(Error::ChecksumMismatch {
                path: entry.path.clone(),
                expected: entry.crc,
                found,
            });
        }
        Ok(out)
    }

    /// Find an entry by path and read it.
    ///
    /// # Errors
    ///
    /// As [`Vpk::read`], plus [`Error::Malformed`] when no such path exists.
    pub fn read_path(&self, path: &str) -> Result<Vec<u8>> {
        let entry = self
            .find(path)
            .ok_or_else(|| Error::Malformed(format!("no entry {path}")))?;
        self.read(entry)
    }

    /// Path of the numbered archive `n`, beside the directory file.
    ///
    /// `pak01_dir.vpk` and archive 254 gives `pak01_254.vpk`.
    fn archive_path(&self, n: u16) -> PathBuf {
        let stem = self
            .dir_path
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_suffix("_dir.vpk"))
            .unwrap_or("pak01");
        self.dir_path.with_file_name(format!("{stem}_{n:03}.vpk"))
    }
}

/// Walk the extension / directory / filename nesting the tree is built from.
fn parse_tree(r: &mut Cursor<'_>, tree_end: usize) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    loop {
        if r.pos() >= tree_end {
            break;
        }
        let extension = r.cstr()?;
        if extension.is_empty() {
            break;
        }
        loop {
            let directory = r.cstr()?;
            if directory.is_empty() {
                break;
            }
            loop {
                let name = r.cstr()?;
                if name.is_empty() {
                    break;
                }
                let crc = r.u32()?;
                let preload_len = r.u16()?;
                let archive_index = r.u16()?;
                let offset = r.u32()?;
                let length = r.u32()?;
                let terminator = r.u16()?;
                if terminator != ENTRY_TERMINATOR {
                    return Err(Error::Malformed(format!(
                        "entry {name}.{extension}: terminator {terminator:#06x}"
                    )));
                }
                let preload = r.take(preload_len as usize)?.to_vec();

                // Valve writes a single space for the archive root and for no extension.
                let file = match extension.as_str() {
                    " " => name,
                    e => format!("{name}.{e}"),
                };
                let path = match directory.as_str() {
                    "" | " " => file,
                    d => format!("{d}/{file}"),
                };
                entries.push(Entry {
                    path,
                    crc,
                    archive: (archive_index != ARCHIVE_INLINE).then_some(archive_index),
                    offset,
                    length,
                    preload,
                });
            }
        }
    }
    Ok(entries)
}

/// Read `len` bytes at `offset` without pulling the whole archive into memory.
fn read_at(path: &Path, offset: u64, len: usize) -> Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};

    let io = |e: std::io::Error| Error::Io {
        path: path.to_path_buf(),
        source: e.to_string(),
    };
    let mut f = std::fs::File::open(path).map_err(io)?;
    f.seek(SeekFrom::Start(offset)).map_err(io)?;
    let mut buf = vec![0u8; len];
    f.read_exact(&mut buf).map_err(io)?;
    Ok(buf)
}

/// CRC32, the IEEE polynomial VPK records for each entry.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            // Branchless: `mask` is all ones when the low bit is set.
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Little-endian reader that reports how far it got rather than panicking.
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Cursor { bytes, pos: 0 }
    }

    fn pos(&self) -> usize {
        self.pos
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| Error::Malformed(format!("length {n} at {} overflows", self.pos)))?;
        if end > self.bytes.len() {
            return Err(Error::Malformed(format!(
                "wanted {n} bytes at {}, only {} left",
                self.pos,
                self.bytes.len().saturating_sub(self.pos)
            )));
        }
        let out = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A null-terminated string. Non-UTF-8 is replaced rather than rejected: a single odd
    /// filename should not make the whole archive unreadable.
    fn cstr(&mut self) -> Result<String> {
        let rest = &self.bytes[self.pos.min(self.bytes.len())..];
        let n = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| Error::Malformed(format!("unterminated string at {}", self.pos)))?;
        let s = String::from_utf8_lossy(&rest[..n]).into_owned();
        self.pos += n + 1;
        Ok(s)
    }
}

#[cfg(test)]
mod tests;
