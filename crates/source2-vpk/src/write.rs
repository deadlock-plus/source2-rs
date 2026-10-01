//! Laying a [`Vpk`] out as bytes.

use crate::md5::md5;
use crate::parse::{OTHER_MD5_SECTION, SIGNATURE_HEADER};
use crate::sections::MD5_CHUNK_SIZE;
use crate::vpk::open_file;
use crate::{
    ARCHIVE_INLINE, ArchiveMd5, Data, ENTRY_TERMINATOR, Entry, Error, Result, SIGNATURE,
    SignatureLayout, Vpk,
};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;

/// Highest usable numbered archive; `0x7fff` is reserved for the inline marker.
const MAX_ARCHIVE_INDEX: usize = ARCHIVE_INLINE as usize - 1;

/// A numbered archive built from entries held in memory.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct BuiltArchive {
    /// The archive's number: `pack_dir.vpk` and index 3 give `pack_003.vpk`.
    pub index: u16,
    /// The archive file's contents.
    pub bytes: Vec<u8>,
}

/// A document laid out as bytes, ready to be written out.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct BuiltVpk {
    /// Contents of the directory file.
    pub directory: Vec<u8>,
    /// Numbered archives created for entries held in memory ([`Data::Memory`]). Archives
    /// that parsed entries already live in are not repeated here.
    pub archives: Vec<BuiltArchive>,
}

pub(crate) struct Row<'a> {
    pub entry: &'a Entry,
    pub archive_index: u16,
    pub offset: u32,
    pub length: u32,
}

pub(crate) struct Layout<'a> {
    pub rows: Vec<Row<'a>>,
    /// Inline bytes added after the document's own `inline_data`.
    pub inline_extra: Vec<u8>,
    pub new_archives: Vec<BuiltArchive>,
    /// Numbered archives that parsed entries refer to.
    pub stored_archives: BTreeSet<u16>,
    /// Whether any entry brings bytes in memory.
    pub has_memory_data: bool,
}

pub(crate) fn layout(vpk: &Vpk) -> Result<Layout<'_>> {
    match vpk.version {
        1 => {
            if vpk.other_md5.is_some()
                || vpk.signature.is_some()
                || vpk.archive_md5.as_ref().is_some_and(|r| !r.is_empty())
            {
                return Err(invalid(
                    "version 1 has no MD5 or signature sections".to_owned(),
                ));
            }
        }
        2 => {}
        v => return Err(Error::UnsupportedVersion(v)),
    }

    let first_new = vpk
        .entries()
        .iter()
        .filter_map(|e| match e.data {
            Data::Stored {
                archive: Some(n), ..
            } => Some(usize::from(n) + 1),
            _ => None,
        })
        .max()
        .unwrap_or(0);

    let limit = u64::from(vpk.max_archive_size.unwrap_or(u32::MAX));
    let mut lay = Layout {
        rows: Vec::with_capacity(vpk.len()),
        inline_extra: Vec::new(),
        new_archives: Vec::new(),
        stored_archives: BTreeSet::new(),
        has_memory_data: false,
    };

    for entry in vpk.entries() {
        split_path(&entry.path)?;
        if entry.preload.len() > usize::from(u16::MAX) {
            return Err(invalid(format!(
                "{}: preload of {} bytes exceeds 65535",
                entry.path,
                entry.preload.len()
            )));
        }

        let (archive_index, offset, length) = match &entry.data {
            Data::Stored {
                archive,
                offset,
                length,
            } => {
                if let Some(n) = archive {
                    if *n == ARCHIVE_INLINE {
                        return Err(invalid(format!(
                            "{}: archive {n:#06x} is the inline marker",
                            entry.path
                        )));
                    }
                    lay.stored_archives.insert(*n);
                }
                (archive.unwrap_or(ARCHIVE_INLINE), *offset, *length)
            }
            Data::Memory { bytes, inline } => {
                let length = u32::try_from(bytes.len())
                    .map_err(|_| invalid(format!("{}: file too large", entry.path)))?;
                if bytes.is_empty() {
                    // Valve's tools record wholly empty files at the maximum offset, and
                    // files that are only a preload at zero.
                    let offset = if entry.preload.is_empty() {
                        u32::MAX
                    } else {
                        0
                    };
                    (ARCHIVE_INLINE, offset, 0)
                } else {
                    lay.has_memory_data = true;
                    if *inline {
                        let at = vpk.inline_data.len() + lay.inline_extra.len();
                        let offset = u32::try_from(at).map_err(|_| {
                            invalid(format!("{}: offset exceeds 4 GiB", entry.path))
                        })?;
                        lay.inline_extra.extend_from_slice(bytes);
                        (ARCHIVE_INLINE, offset, length)
                    } else {
                        let start_new = lay.new_archives.last().is_none_or(|a| {
                            !a.bytes.is_empty() && a.bytes.len() as u64 + u64::from(length) > limit
                        });
                        if start_new {
                            let index = first_new + lay.new_archives.len();
                            if index > MAX_ARCHIVE_INDEX {
                                return Err(invalid(
                                    "more archives than the format can index".to_owned(),
                                ));
                            }
                            lay.new_archives.push(BuiltArchive {
                                index: index as u16,
                                bytes: Vec::new(),
                            });
                        }
                        let current = lay.new_archives.last_mut().expect("pushed above");
                        let offset = u32::try_from(current.bytes.len()).map_err(|_| {
                            invalid(format!("{}: offset exceeds 4 GiB", entry.path))
                        })?;
                        current.bytes.extend_from_slice(bytes);
                        (current.index, offset, length)
                    }
                }
            }
        };
        lay.rows.push(Row {
            entry,
            archive_index,
            offset,
            length,
        });
    }
    Ok(lay)
}

pub(crate) struct Directory {
    pub bytes: Vec<u8>,
    /// The whole-file digest that was computed, when the document has an other-MD5
    /// section.
    pub whole: Option<[u8; 16]>,
}

/// Put the directory file together. `digests` overrides the tree and section digests
/// that would otherwise be computed from `tree` and `section`.
pub(crate) fn assemble(
    vpk: &Vpk,
    lay: &Layout<'_>,
    tree: &[u8],
    section: &[u8],
    digests: Option<([u8; 16], [u8; 16])>,
) -> Result<Directory> {
    let len32 = |len: usize, what: &str| {
        u32::try_from(len).map_err(|_| invalid(format!("{what} exceeds 4 GiB")))
    };
    let data_len = vpk.inline_data.len() + lay.inline_extra.len();

    let mut out = Vec::with_capacity(28 + tree.len() + data_len + section.len() + 64);
    out.extend_from_slice(&SIGNATURE.to_le_bytes());
    out.extend_from_slice(&vpk.version.to_le_bytes());
    out.extend_from_slice(&len32(tree.len(), "directory tree")?.to_le_bytes());

    let sig_declared = vpk.signature.as_ref().map_or(0, |s| match s.layout {
        SignatureLayout::Classic => 8 + s.public_key.len() + s.signature.len(),
        SignatureLayout::Headed { .. } => SIGNATURE_HEADER,
    });
    if vpk.version == 2 {
        out.extend_from_slice(&len32(data_len, "inline data")?.to_le_bytes());
        out.extend_from_slice(&len32(section.len(), "archive MD5 section")?.to_le_bytes());
        let other = if vpk.other_md5.is_some() {
            OTHER_MD5_SECTION
        } else {
            0
        };
        out.extend_from_slice(&len32(other, "other MD5 section")?.to_le_bytes());
        out.extend_from_slice(&len32(sig_declared, "signature section")?.to_le_bytes());
    }
    out.extend_from_slice(tree);
    out.extend_from_slice(&vpk.inline_data);
    out.extend_from_slice(&lay.inline_extra);

    let mut whole = None;
    if vpk.version == 2 {
        out.extend_from_slice(section);
        if vpk.other_md5.is_some() {
            let (tree_md5, section_md5) = digests.unwrap_or_else(|| (md5(tree), md5(section)));
            out.extend_from_slice(&tree_md5);
            out.extend_from_slice(&section_md5);
            let digest = md5(&out);
            out.extend_from_slice(&digest);
            whole = Some(digest);
        }
        if let Some(sig) = &vpk.signature {
            match sig.layout {
                SignatureLayout::Classic => {
                    out.extend_from_slice(
                        &len32(sig.public_key.len(), "public key")?.to_le_bytes(),
                    );
                    out.extend_from_slice(&sig.public_key);
                    out.extend_from_slice(&len32(sig.signature.len(), "signature")?.to_le_bytes());
                    out.extend_from_slice(&sig.signature);
                }
                SignatureLayout::Headed { version, reserved } => {
                    out.extend_from_slice(&SIGNATURE.to_le_bytes());
                    out.extend_from_slice(&version.to_le_bytes());
                    out.extend_from_slice(
                        &len32(sig.public_key.len(), "public key")?.to_le_bytes(),
                    );
                    out.extend_from_slice(&len32(sig.signature.len(), "signature")?.to_le_bytes());
                    out.extend_from_slice(&reserved.to_le_bytes());
                    out.extend_from_slice(&sig.public_key);
                    out.extend_from_slice(&sig.signature);
                }
            }
        }
    }
    Ok(Directory { bytes: out, whole })
}

pub(crate) fn tree_bytes(lay: &Layout<'_>) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut current: Option<(String, String)> = None;
    for row in &lay.rows {
        let (ext, dir, stem) = split_path(&row.entry.path)?;
        match &current {
            Some((e, d)) if *e == ext && *d == dir => {}
            Some((e, _)) => {
                out.push(0);
                if *e != ext {
                    out.push(0);
                    push_cstr(&mut out, &ext);
                }
                push_cstr(&mut out, &dir);
            }
            None => {
                push_cstr(&mut out, &ext);
                push_cstr(&mut out, &dir);
            }
        }
        push_cstr(&mut out, &stem);
        out.extend_from_slice(&row.entry.crc.to_le_bytes());
        out.extend_from_slice(&(row.entry.preload.len() as u16).to_le_bytes());
        out.extend_from_slice(&row.archive_index.to_le_bytes());
        out.extend_from_slice(&row.offset.to_le_bytes());
        out.extend_from_slice(&row.length.to_le_bytes());
        out.extend_from_slice(&ENTRY_TERMINATOR.to_le_bytes());
        out.extend_from_slice(&row.entry.preload);
        current = Some((ext, dir));
    }
    if current.is_some() {
        out.push(0);
        out.push(0);
    }
    out.push(0);
    Ok(out)
}

pub(crate) fn records_bytes(records: &[ArchiveMd5]) -> Vec<u8> {
    let mut out = Vec::with_capacity(records.len() * crate::parse::ARCHIVE_MD5_RECORD);
    for r in records {
        out.extend_from_slice(&r.archive_index.to_le_bytes());
        out.extend_from_slice(&r.offset.to_le_bytes());
        out.extend_from_slice(&r.length.to_le_bytes());
        out.extend_from_slice(&r.md5);
    }
    out
}

/// Records for every archive the document uses and for the inline data: [`MD5_CHUNK_SIZE`]
/// pieces of each whole archive file, archives first.
fn generate_records(vpk: &Vpk, lay: &Layout<'_>) -> Result<Vec<ArchiveMd5>> {
    let mut numbers: BTreeSet<u16> = lay.stored_archives.clone();
    numbers.extend(lay.new_archives.iter().map(|a| a.index));

    let mut records = Vec::new();
    for n in numbers {
        if let Some(built) = lay.new_archives.iter().find(|a| a.index == n) {
            for (offset, length, digest) in chunk_digests(&built.bytes) {
                records.push(ArchiveMd5::for_archive(n, offset, length, digest));
            }
        } else {
            let path = vpk.archive_path(n)?;
            let mut f = open_file(&path)?;
            let mut buf = vec![0u8; MD5_CHUNK_SIZE as usize];
            let mut offset = 0u32;
            loop {
                let mut filled = 0;
                while filled < buf.len() {
                    match f.read(&mut buf[filled..]) {
                        Ok(0) => break,
                        Ok(k) => filled += k,
                        Err(source) => return Err(Error::Io { path, source }),
                    }
                }
                if filled == 0 {
                    break;
                }
                records.push(ArchiveMd5::for_archive(
                    n,
                    offset,
                    filled as u32,
                    md5(&buf[..filled]),
                ));
                offset = offset
                    .checked_add(filled as u32)
                    .ok_or_else(|| invalid(format!("{} is larger than 4 GiB", path.display())))?;
                if filled < buf.len() {
                    break;
                }
            }
        }
    }

    if !vpk.inline_data.is_empty() || !lay.inline_extra.is_empty() {
        let mut data = Vec::with_capacity(vpk.inline_data.len() + lay.inline_extra.len());
        data.extend_from_slice(&vpk.inline_data);
        data.extend_from_slice(&lay.inline_extra);
        for (offset, length, digest) in chunk_digests(&data) {
            records.push(ArchiveMd5::for_inline(offset, length, digest));
        }
    }
    Ok(records)
}

/// `(offset, length, md5)` of each chunk of `data`.
fn chunk_digests(data: &[u8]) -> impl Iterator<Item = (u32, u32, [u8; 16])> + '_ {
    data.chunks(MD5_CHUNK_SIZE as usize)
        .enumerate()
        .map(|(i, c)| ((i as u32) * MD5_CHUNK_SIZE, c.len() as u32, md5(c)))
}

impl Vpk {
    /// Lay the document out in memory.
    ///
    /// The directory file always comes back. Numbered archives come back only for entries
    /// held in memory; entries parsed from an existing pack keep pointing at that pack's
    /// archives, which [`Vpk::write`] copies when it writes somewhere else.
    ///
    /// Hand-built entries are placed in the order given, into numbered archives from the
    /// first free number up (or after the document's inline data for [`Entry::inline`]).
    /// Parsed entries keep their recorded archive, offset and length, so a parsed
    /// document writes back to the same directory file bytes: entry order, directory
    /// order, offsets, CRCs, preloads and sections are all reproduced. Only the directory
    /// digests are recomputed (to the same values for an unchanged file).
    ///
    /// # Errors
    ///
    /// [`Error::UnsupportedVersion`] for a version other than 1 or 2, and
    /// [`Error::InvalidInput`] for a path that cannot be split into directory, name and
    /// extension, a preload over 65535 bytes, a version 1 document with version 2
    /// sections, or more data than 32-bit offsets and the archive count can address.
    /// [`Error::Io`] or [`Error::InvalidInput`] when archive MD5 records have to be
    /// regenerated and an archive cannot be read.
    pub fn build(&self) -> Result<BuiltVpk> {
        let lay = layout(self)?;
        let directory = self.directory_for(&lay)?;
        Ok(BuiltVpk {
            directory,
            archives: lay.new_archives,
        })
    }

    fn directory_for(&self, lay: &Layout<'_>) -> Result<Vec<u8>> {
        let tree = tree_bytes(lay)?;
        let records = match (&self.archive_md5, self.version) {
            (_, 1) => Vec::new(),
            (Some(r), _) if !lay.has_memory_data => r.clone(),
            _ => generate_records(self, lay)?,
        };
        let section = records_bytes(&records);
        Ok(assemble(self, lay, &tree, &section, None)?.bytes)
    }

    /// The directory file alone, for a pack that needs no numbered archives (everything
    /// inline or fully preloaded).
    ///
    /// # Errors
    ///
    /// As [`Vpk::build`], plus [`Error::InvalidInput`] if any entry needs a numbered
    /// archive.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let lay = layout(self)?;
        if !lay.new_archives.is_empty() || !lay.stored_archives.is_empty() {
            return Err(invalid(
                "the pack needs numbered archives; use build or write".to_owned(),
            ));
        }
        self.directory_for(&lay)
    }

    /// Write the directory file at `dir_path`, plus every numbered archive beside it:
    /// new ones built from entries in memory, and copies of the archives parsed entries
    /// live in (unless that is where they already are).
    ///
    /// When any entry uses a numbered archive, the file name must end in `_dir.vpk`;
    /// `pack_dir.vpk` gives `pack_000.vpk` and so on. A pack with only inline data can be
    /// named anything.
    ///
    /// # Errors
    ///
    /// As [`Vpk::build`], [`Error::InvalidInput`] for a file name without the `_dir.vpk`
    /// suffix, or for parsed entries in numbered archives when the document has no
    /// source, and [`Error::Io`] when a file cannot be written.
    pub fn write(&self, dir_path: impl AsRef<Path>) -> Result<()> {
        let dir_path = dir_path.as_ref();
        let lay = layout(self)?;

        let uses_archives = !lay.new_archives.is_empty() || !lay.stored_archives.is_empty();
        let dest_archive = |n: u16| -> Result<std::path::PathBuf> {
            let stem = dir_path
                .file_name()
                .and_then(|s| s.to_str())
                .and_then(|s| s.strip_suffix("_dir.vpk"))
                .ok_or_else(|| {
                    invalid(format!(
                        "{} does not end in _dir.vpk, so it cannot have numbered archives",
                        dir_path.display()
                    ))
                })?;
            Ok(dir_path.with_file_name(format!("{stem}_{n:03}.vpk")))
        };
        let mut copies = Vec::new();
        if uses_archives {
            dest_archive(0)?;
            for &n in &lay.stored_archives {
                let from = self.archive_path(n)?;
                let to = dest_archive(n)?;
                if !same_file(&from, &to) {
                    copies.push((from, to));
                }
            }
        }

        let directory = self.directory_for(&lay)?;
        let put = |path: &Path, bytes: &[u8]| {
            std::fs::write(path, bytes).map_err(|source| Error::Io {
                path: path.to_path_buf(),
                source,
            })
        };
        if let Some(parent) = dir_path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        put(dir_path, &directory)?;
        for built in &lay.new_archives {
            put(&dest_archive(built.index)?, &built.bytes)?;
        }
        for (from, to) in copies {
            std::fs::copy(&from, &to).map_err(|source| Error::Io { path: from, source })?;
        }
        Ok(())
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (std::fs::canonicalize(a), std::fs::canonicalize(b)),
            (Ok(x), Ok(y)) if x == y
        )
}

fn invalid(message: String) -> Error {
    Error::InvalidInput(message)
}

/// Split `a/b/c.txt` into extension `txt`, directory `a/b` and stem `c`.
///
/// A space stands for no directory and for no extension, since an empty string ends a
/// level of the tree.
pub(crate) fn split_path(path: &str) -> Result<(String, String, String)> {
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

fn push_cstr(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
    out.push(0);
}
