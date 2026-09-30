//! Writer for Valve Pak (VPK) archives.
//!
//! ```no_run
//! use source2_vpk::VpkWriter;
//!
//! VpkWriter::new(2)
//!     .max_archive_size(200 * 1024 * 1024)
//!     .add("scripts/heroes.vdata_c", b"payload")
//!     .write("out/pak01_dir.vpk")?;
//! # Ok::<(), source2_vpk::Error>(())
//! ```

use crate::md5::md5;
use crate::{ARCHIVE_INLINE, ENTRY_TERMINATOR, Error, Result, SIGNATURE, crc32};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// Size of one record in the v2 archive MD5 section: archive index, offset, length, digest.
const ARCHIVE_MD5_RECORD: usize = 28;

/// Size of the v2 "other" MD5 section: tree, archive MD5 section and whole-file digests.
const OTHER_MD5_SECTION: u32 = 48;

/// Highest usable numbered archive; `0x7fff` is reserved for the inline marker.
const MAX_ARCHIVE_INDEX: usize = ARCHIVE_INLINE as usize - 1;

/// The bytes of a finished archive, ready to be written out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltVpk {
    /// Contents of the `*_dir.vpk` file.
    pub directory: Vec<u8>,
    /// Contents of each numbered archive, index 0 first. Empty when every file is inline
    /// or fully preloaded.
    pub archives: Vec<Vec<u8>>,
}

/// Collects files and lays them out as a VPK.
///
/// The v2 footer gets the archive MD5 section and the tree, section and whole-file
/// digests. The optional RSA signature section is never written; its size is recorded as
/// zero.
#[derive(Clone, Debug)]
pub struct VpkWriter {
    version: u32,
    max_archive_size: Option<u32>,
    files: Vec<Pending>,
}

#[derive(Clone, Debug)]
struct Pending {
    path: String,
    data: Vec<u8>,
    preload_len: usize,
    inline: bool,
}

/// Where one file's bytes ended up, as the tree records it.
struct Placed<'a> {
    crc: u32,
    preload: &'a [u8],
    archive_index: u16,
    offset: u32,
    length: u32,
}

/// `extension -> directory -> file stem`, each level sorted so output is deterministic.
type Tree<'a> = BTreeMap<String, BTreeMap<String, BTreeMap<String, Placed<'a>>>>;

impl VpkWriter {
    /// A writer for VPK `version`, 1 or 2. Any other version fails at [`VpkWriter::build`].
    pub fn new(version: u32) -> Self {
        VpkWriter {
            version,
            max_archive_size: None,
            files: Vec::new(),
        }
    }

    /// Start a new numbered archive once adding a file would take the current one past
    /// `bytes`. A file larger than `bytes` gets an archive to itself.
    ///
    /// Without a limit, archives only split where 32-bit offsets run out.
    #[must_use]
    pub fn max_archive_size(mut self, bytes: u32) -> Self {
        self.max_archive_size = Some(bytes);
        self
    }

    /// Add a file whose bytes go to the numbered archives.
    #[must_use]
    pub fn add(self, path: &str, data: &[u8]) -> Self {
        self.push(path, data, 0, false)
    }

    /// Add a file whose first `preload_len` bytes live in the directory tree and the rest
    /// in the numbered archives. `preload_len` is clamped to the file's size.
    #[must_use]
    pub fn add_with_preload(self, path: &str, data: &[u8], preload_len: usize) -> Self {
        self.push(path, data, preload_len, false)
    }

    /// Add a file stored in the directory file's own data section, so a pack made only of
    /// these needs no numbered archives.
    #[must_use]
    pub fn add_inline(self, path: &str, data: &[u8]) -> Self {
        self.push(path, data, 0, true)
    }

    fn push(mut self, path: &str, data: &[u8], preload_len: usize, inline: bool) -> Self {
        self.files.push(Pending {
            path: path.to_owned(),
            data: data.to_vec(),
            preload_len,
            inline,
        });
        self
    }

    /// Lay the files out in memory.
    ///
    /// # Errors
    ///
    /// [`Error::UnsupportedVersion`] for a version other than 1 or 2, and
    /// [`Error::InvalidInput`] for a path that cannot be split into directory, name and
    /// extension, a duplicate path, a preload over 65535 bytes, or more data than 32-bit
    /// offsets and the archive count can address.
    pub fn build(&self) -> Result<BuiltVpk> {
        if !matches!(self.version, 1 | 2) {
            return Err(Error::UnsupportedVersion(self.version));
        }

        let mut seen = HashSet::new();
        let mut names = Vec::with_capacity(self.files.len());
        for f in &self.files {
            if !seen.insert(f.path.as_str()) {
                return Err(invalid(format!("duplicate path {}", f.path)));
            }
            names.push(split_path(&f.path)?);
        }

        let limit = u64::from(self.max_archive_size.unwrap_or(u32::MAX));
        let mut archives: Vec<Vec<u8>> = Vec::new();
        let mut inline: Vec<u8> = Vec::new();
        let mut chunks: Vec<(u16, u32, u32)> = Vec::new();
        let mut tree = Tree::new();

        for (f, (ext, dir, stem)) in self.files.iter().zip(names) {
            let preload_len = f.preload_len.min(f.data.len());
            if preload_len > usize::from(u16::MAX) {
                return Err(invalid(format!(
                    "{}: preload of {preload_len} bytes exceeds 65535",
                    f.path
                )));
            }
            let (preload, rest) = f.data.split_at(preload_len);
            let length = u32::try_from(rest.len())
                .map_err(|_| invalid(format!("{}: file too large", f.path)))?;

            let (archive_index, offset) = if f.inline {
                let offset = offset_of(&inline, &f.path)?;
                inline.extend_from_slice(rest);
                (ARCHIVE_INLINE, offset)
            } else if rest.is_empty() {
                (ARCHIVE_INLINE, 0)
            } else {
                let start_new = archives
                    .last()
                    .is_none_or(|a| !a.is_empty() && a.len() as u64 + u64::from(length) > limit);
                if start_new {
                    if archives.len() > MAX_ARCHIVE_INDEX {
                        return Err(invalid("more archives than the format can index".into()));
                    }
                    archives.push(Vec::new());
                }
                let index = archives.len() - 1;
                let current = &mut archives[index];
                let offset = offset_of(current, &f.path)?;
                current.extend_from_slice(rest);
                let index = index as u16;
                chunks.push((index, offset, length));
                (index, offset)
            };

            tree.entry(ext).or_default().entry(dir).or_default().insert(
                stem,
                Placed {
                    crc: crc32(&f.data),
                    preload,
                    archive_index,
                    offset,
                    length,
                },
            );
        }

        let tree_bytes = serialize_tree(&tree);
        let mut directory = Vec::new();
        directory.extend_from_slice(&SIGNATURE.to_le_bytes());
        directory.extend_from_slice(&self.version.to_le_bytes());
        directory.extend_from_slice(&len32(tree_bytes.len(), "directory tree")?.to_le_bytes());

        if self.version == 1 {
            directory.extend_from_slice(&tree_bytes);
            directory.extend_from_slice(&inline);
            return Ok(BuiltVpk {
                directory,
                archives,
            });
        }

        let mut section = Vec::with_capacity(chunks.len() * ARCHIVE_MD5_RECORD);
        for &(index, offset, length) in &chunks {
            let bytes = &archives[usize::from(index)][offset as usize..][..length as usize];
            section.extend_from_slice(&u32::from(index).to_le_bytes());
            section.extend_from_slice(&offset.to_le_bytes());
            section.extend_from_slice(&length.to_le_bytes());
            section.extend_from_slice(&md5(bytes));
        }

        directory.extend_from_slice(&len32(inline.len(), "inline data")?.to_le_bytes());
        directory.extend_from_slice(&len32(section.len(), "archive MD5 section")?.to_le_bytes());
        directory.extend_from_slice(&OTHER_MD5_SECTION.to_le_bytes());
        directory.extend_from_slice(&0u32.to_le_bytes());
        directory.extend_from_slice(&tree_bytes);
        directory.extend_from_slice(&inline);
        directory.extend_from_slice(&section);
        directory.extend_from_slice(&md5(&tree_bytes));
        directory.extend_from_slice(&md5(&section));
        let whole = md5(&directory);
        directory.extend_from_slice(&whole);

        Ok(BuiltVpk {
            directory,
            archives,
        })
    }

    /// Build and write the directory file at `dir_path`, plus each numbered archive beside
    /// it. `dir_path` must end in `_dir.vpk`; `pak01_dir.vpk` gives `pak01_000.vpk` and so
    /// on.
    ///
    /// # Errors
    ///
    /// As [`VpkWriter::build`], [`Error::InvalidInput`] for a file name without the
    /// `_dir.vpk` suffix, and [`Error::Io`] when a file cannot be written.
    pub fn write(&self, dir_path: impl AsRef<Path>) -> Result<()> {
        let dir_path = dir_path.as_ref();
        let stem = dir_path
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_suffix("_dir.vpk"))
            .ok_or_else(|| invalid(format!("{} does not end in _dir.vpk", dir_path.display())))?;

        let built = self.build()?;
        let put = |path: &Path, bytes: &[u8]| {
            std::fs::write(path, bytes).map_err(|e| Error::Io {
                path: path.to_path_buf(),
                source: e.to_string(),
            })
        };
        put(dir_path, &built.directory)?;
        for (n, archive) in built.archives.iter().enumerate() {
            put(
                &dir_path.with_file_name(format!("{stem}_{n:03}.vpk")),
                archive,
            )?;
        }
        Ok(())
    }
}

fn invalid(message: String) -> Error {
    Error::InvalidInput(message)
}

fn len32(len: usize, what: &str) -> Result<u32> {
    u32::try_from(len).map_err(|_| invalid(format!("{what} exceeds 4 GiB")))
}

fn offset_of(buf: &[u8], path: &str) -> Result<u32> {
    u32::try_from(buf.len()).map_err(|_| invalid(format!("{path}: offset exceeds 4 GiB")))
}

/// Split `a/b/c.txt` into extension `txt`, directory `a/b` and stem `c`.
///
/// A space stands for no directory and for no extension, since an empty string ends a
/// level of the tree.
fn split_path(path: &str) -> Result<(String, String, String)> {
    if path.is_empty() || path.contains('\0') || path.split('/').any(str::is_empty) {
        return Err(invalid(format!("unusable path {path:?}")));
    }
    let (dir, file) = match path.rfind('/') {
        Some(n) => (&path[..n], &path[n + 1..]),
        None => (" ", path),
    };
    let (stem, ext) = match file.rfind('.') {
        Some(n) => (&file[..n], &file[n + 1..]),
        None => (file, " "),
    };
    if stem.is_empty() || ext.is_empty() {
        return Err(invalid(format!(
            "{path:?} needs a non-empty name and extension"
        )));
    }
    Ok((ext.to_owned(), dir.to_owned(), stem.to_owned()))
}

fn serialize_tree(tree: &Tree<'_>) -> Vec<u8> {
    let mut out = Vec::new();
    for (ext, dirs) in tree {
        push_cstr(&mut out, ext);
        for (dir, files) in dirs {
            push_cstr(&mut out, dir);
            for (stem, p) in files {
                push_cstr(&mut out, stem);
                out.extend_from_slice(&p.crc.to_le_bytes());
                out.extend_from_slice(&(p.preload.len() as u16).to_le_bytes());
                out.extend_from_slice(&p.archive_index.to_le_bytes());
                out.extend_from_slice(&p.offset.to_le_bytes());
                out.extend_from_slice(&p.length.to_le_bytes());
                out.extend_from_slice(&ENTRY_TERMINATOR.to_le_bytes());
                out.extend_from_slice(p.preload);
            }
            out.push(0);
        }
        out.push(0);
    }
    out.push(0);
    out
}

fn push_cstr(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
    out.push(0);
}

#[cfg(test)]
mod tests;
