//! The VPK document.

use crate::md5::md5;
use crate::sections::{ArchiveMd5Report, DirectoryMd5Report, Md5Target};
use crate::write::{assemble, layout, records_bytes, split_path, tree_bytes};
use crate::{ArchiveMd5, Contents, Data, Entry, Error, OtherMd5, Result, Signature, crc32};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// A VPK directory with everything the format stores, for reading and for writing.
///
/// Parsed and hand-built documents are the same type. [`Vpk::open`] and [`Vpk::parse`]
/// fill every field, and [`Vpk::write`] or [`Vpk::build`] turn the document back into
/// the same directory file bytes. [`Vpk::new`] plus [`Vpk::add`] builds one from scratch;
/// the optional fields default so the result follows Valve's conventions.
///
/// Entries parsed from a `*_dir.vpk` keep their numbered-archive bytes in the archive
/// files beside it, read on demand, so a document is cheap to hold however large the
/// pack is.
#[derive(Clone, Debug)]
pub struct Vpk {
    /// Format version, 1 or 2. Anything else fails at write time.
    pub version: u32,
    /// The directory file's data section: bytes of entries stored inline, which
    /// [`Data::Stored`] entries with no archive offset into. Entries added in memory
    /// with [`Entry::inline`] are appended after it when written.
    pub inline_data: Vec<u8>,
    /// The version 2 archive MD5 section. `None` means "work it out when writing":
    /// version 2 documents then get records for every archive and the inline data. Parsing
    /// always fills it (an empty list when the file has none).
    ///
    /// A `Some` list is written back as found, unless the document also holds entries in
    /// memory ([`Data::Memory`]), which change the data the records describe; then the
    /// records are regenerated. Must be `None` or empty for version 1.
    pub archive_md5: Option<Vec<ArchiveMd5>>,
    /// The version 2 "other MD5" section. Present by default for version 2; the digests
    /// are recomputed on every write. Must be `None` for version 1.
    pub other_md5: Option<OtherMd5>,
    /// The version 2 RSA public key and signature section. Written back as found; it
    /// cannot be re-made here, so any change to the document invalidates it. Must be
    /// `None` for version 1.
    pub signature: Option<Signature>,
    /// Start a new numbered archive once adding a file would take the current one past
    /// this many bytes. A file larger than the limit gets an archive to itself. `None`
    /// splits only where 32-bit offsets run out.
    pub max_archive_size: Option<u32>,
    entries: Vec<Entry>,
    source: Option<PathBuf>,
    index: OnceLock<HashMap<String, usize>>,
}

impl Vpk {
    /// An empty document of the given version (1 or 2).
    pub fn new(version: u32) -> Self {
        Vpk::from_parts(
            version,
            Vec::new(),
            Vec::new(),
            None,
            (version == 2).then(OtherMd5::default),
            None,
        )
    }

    pub(crate) fn from_parts(
        version: u32,
        entries: Vec<Entry>,
        inline_data: Vec<u8>,
        archive_md5: Option<Vec<ArchiveMd5>>,
        other_md5: Option<OtherMd5>,
        signature: Option<Signature>,
    ) -> Self {
        Vpk {
            version,
            inline_data,
            archive_md5,
            other_md5,
            signature,
            max_archive_size: None,
            entries,
            source: None,
            index: OnceLock::new(),
        }
    }

    /// Open a directory file (`*_dir.vpk`, or a standalone pack) and parse it.
    ///
    /// The numbered archives beside it are not touched until an entry is read.
    ///
    /// # Errors
    ///
    /// If the file cannot be read, does not carry the VPK signature, is a version this
    /// crate does not handle, has a tree or section that is cut short or inconsistent, or
    /// holds a name that is not UTF-8 ([`Vpk::open_lossy`] reads those).
    pub fn open(dir_path: impl AsRef<Path>) -> Result<Self> {
        let dir_path = dir_path.as_ref();
        let bytes = read_file(dir_path)?;
        Ok(crate::parse::parse(bytes, false)?.with_source(dir_path))
    }

    /// As [`Vpk::open`], replacing bytes in names that are not UTF-8 with U+FFFD.
    ///
    /// The result does not write back to the same bytes, and names that differed only in
    /// the replaced bytes become equal.
    ///
    /// # Errors
    ///
    /// As [`Vpk::open`], except for non-UTF-8 names.
    pub fn open_lossy(dir_path: impl AsRef<Path>) -> Result<Self> {
        let dir_path = dir_path.as_ref();
        let bytes = read_file(dir_path)?;
        Ok(crate::parse::parse(bytes, true)?.with_source(dir_path))
    }

    /// Parse a directory file already held in memory.
    ///
    /// The document has no source, so entries stored in numbered archives cannot be read
    /// until [`Vpk::with_source`] says where the archives are. Inline entries can.
    ///
    /// # Errors
    ///
    /// As [`Vpk::open`], minus the read.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        crate::parse::parse(bytes.to_vec(), false)
    }

    /// As [`Vpk::parse`], replacing bytes in names that are not UTF-8 with U+FFFD. See
    /// [`Vpk::open_lossy`] for what that costs.
    ///
    /// # Errors
    ///
    /// As [`Vpk::parse`], except for non-UTF-8 names.
    pub fn parse_lossy(bytes: &[u8]) -> Result<Self> {
        crate::parse::parse(bytes.to_vec(), true)
    }

    /// Set the directory file the numbered archives sit beside. Archive `n` of
    /// `pack_dir.vpk` is `pack_00n.vpk` in the same directory.
    #[must_use]
    pub fn with_source(mut self, dir_path: impl AsRef<Path>) -> Self {
        self.source = Some(dir_path.as_ref().to_path_buf());
        self
    }

    /// The directory file this document was opened from or pointed at, if any.
    pub fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }

    /// Every entry, in the order the directory lists them.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Mutable access to the entries. Order is the order written.
    pub fn entries_mut(&mut self) -> &mut Vec<Entry> {
        self.index = OnceLock::new();
        &mut self.entries
    }

    /// Add a file whose bytes go to the numbered archives, next to the other entries that
    /// share its extension and directory.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidInput`] if the path cannot be written to a tree (empty segment, no
    /// file name or extension, a NUL) or an entry with that path already exists. The
    /// duplicate check scans every entry; use [`Vpk::push`] to add many.
    pub fn add(
        &mut self,
        path: impl Into<String>,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<&mut Entry> {
        let entry = Entry::new(path, bytes);
        split_path(&entry.path)?;
        if self.entries.iter().any(|e| e.path == entry.path) {
            return Err(Error::InvalidInput(format!(
                "duplicate path {}",
                entry.path
            )));
        }
        let at = self.insert_grouped(entry);
        Ok(&mut self.entries[at])
    }

    /// Add an entry as given, next to the other entries that share its extension and
    /// directory. Nothing is checked until the document is written.
    pub fn push(&mut self, entry: Entry) {
        self.insert_grouped(entry);
    }

    fn insert_grouped(&mut self, entry: Entry) -> usize {
        self.index = OnceLock::new();
        let at = match split_path(&entry.path) {
            Ok((ext, dir, _)) => {
                let same_dir =
                    |e: &Entry| split_path(&e.path).is_ok_and(|(x, d, _)| x == ext && d == dir);
                let same_ext = |e: &Entry| split_path(&e.path).is_ok_and(|(x, _, _)| x == ext);
                match self.entries.last() {
                    Some(last) if same_dir(last) => self.entries.len(),
                    _ => self
                        .entries
                        .iter()
                        .rposition(same_dir)
                        .or_else(|| self.entries.iter().rposition(same_ext))
                        .map_or(self.entries.len(), |i| i + 1),
                }
            }
            Err(_) => self.entries.len(),
        };
        self.entries.insert(at, entry);
        at
    }

    /// Remove and return the first entry with this path.
    ///
    /// Exactly one entry is dropped per call. A pack may list the same path more than
    /// once (a `README.txt` twice, say), so [`Vpk::find`] can still return another copy
    /// afterwards; call `remove` again until it returns `None` to drop them all.
    pub fn remove(&mut self, path: &str) -> Option<Entry> {
        let at = self.entries.iter().position(|e| e.path == path)?;
        self.index = OnceLock::new();
        Some(self.entries.remove(at))
    }

    /// How many files the archive holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the archive holds no files.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Look up an entry by its full path, e.g. `docs/readme.txt`. When a pack lists a
    /// path twice, the first wins; [`Vpk::entries`] still has both.
    pub fn find(&self, path: &str) -> Option<&Entry> {
        let index = self.index.get_or_init(|| {
            let mut map = HashMap::with_capacity(self.entries.len());
            for (i, e) in self.entries.iter().enumerate() {
                map.entry(e.path.clone()).or_insert(i);
            }
            map
        });
        index.get(path).map(|&i| &self.entries[i])
    }

    /// Every entry whose file name has this extension, with or without the leading dot.
    /// An empty extension matches the entries that have none.
    pub fn find_by_extension<'a>(&'a self, extension: &'a str) -> impl Iterator<Item = &'a Entry> {
        let wanted = extension.strip_prefix('.').unwrap_or(extension);
        self.entries
            .iter()
            .filter(move |e| e.extension().unwrap_or("") == wanted)
    }

    /// Every entry below the directory `prefix`, at any depth, in directory order.
    ///
    /// `docs` and `docs/` are the same request; an empty prefix is the whole archive. The
    /// prefix is matched on a path boundary rather than as a bare string, so `docs` never
    /// returns `docs_old/readme.txt`, and a full file path names a file rather than a
    /// directory and so has nothing below it.
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
    /// [`Error::ChecksumMismatch`] if the bytes do not match the CRC (use
    /// [`Vpk::read_with_crc`] to get them anyway), [`Error::Io`] if an archive cannot be
    /// read, [`Error::InvalidInput`] if the entry lives in a numbered archive and the
    /// document has no source to find it, and [`Error::Malformed`] if the entry points
    /// outside the data it names.
    pub fn read(&self, entry: &Entry) -> Result<Vec<u8>> {
        let contents = self.read_with_crc(entry)?;
        if !contents.crc_ok() {
            return Err(Error::ChecksumMismatch {
                path: entry.path.clone(),
                expected: contents.expected_crc,
                found: contents.actual_crc,
            });
        }
        Ok(contents.bytes)
    }

    /// Read one entry's bytes along with its CRC verdict, without failing on a mismatch.
    ///
    /// # Errors
    ///
    /// As [`Vpk::read`], except for a CRC mismatch, which is reported in the result.
    pub fn read_with_crc(&self, entry: &Entry) -> Result<Contents> {
        let mut bytes = Vec::with_capacity(entry.size());
        bytes.extend_from_slice(&entry.preload);
        match &entry.data {
            Data::Memory { bytes: b, .. } => bytes.extend_from_slice(b),
            Data::Stored { length: 0, .. } => {}
            Data::Stored {
                archive: Some(n),
                offset,
                length,
            } => {
                let path = self.archive_path(*n)?;
                let mut f = open_file(&path)?;
                let chunk = read_range(&mut f, &path, u64::from(*offset), *length as usize)?
                    .ok_or_else(|| {
                        Error::Malformed(format!(
                            "{}: {length} bytes at {offset} run past the end of {}",
                            entry.path,
                            path.display()
                        ))
                    })?;
                bytes.extend_from_slice(&chunk);
            }
            Data::Stored {
                archive: None,
                offset,
                length,
            } => {
                let start = *offset as usize;
                let chunk = self
                    .inline_data
                    .get(start..start + *length as usize)
                    .ok_or_else(|| {
                        Error::Malformed(format!(
                            "{}: inline range {offset}+{length} is past the {}-byte data section",
                            entry.path,
                            self.inline_data.len()
                        ))
                    })?;
                bytes.extend_from_slice(chunk);
            }
        }
        let actual_crc = crc32(&bytes);
        Ok(Contents {
            bytes,
            expected_crc: entry.crc,
            actual_crc,
        })
    }

    /// Find an entry by path and read it.
    ///
    /// # Errors
    ///
    /// As [`Vpk::read`], plus [`Error::NotFound`] when no such path exists.
    pub fn read_path(&self, path: &str) -> Result<Vec<u8>> {
        let entry = self
            .find(path)
            .ok_or_else(|| Error::NotFound(path.to_owned()))?;
        self.read(entry)
    }

    /// Path of numbered archive `n`, beside the directory file.
    ///
    /// `pack_dir.vpk` and archive 254 gives `pack_254.vpk`.
    pub(crate) fn archive_path(&self, n: u16) -> Result<PathBuf> {
        let dir_path = self.source.as_deref().ok_or_else(|| {
            Error::InvalidInput(format!(
                "archive {n} is needed but the document has no source; see Vpk::with_source"
            ))
        })?;
        let stem = dir_path
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_suffix("_dir.vpk"))
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "{} does not end in _dir.vpk, so it has no numbered archives",
                    dir_path.display()
                ))
            })?;
        Ok(dir_path.with_file_name(format!("{stem}_{n:03}.vpk")))
    }

    /// Check the version 2 directory digests against the document: the tree, the archive
    /// MD5 section and the whole directory file, as they would be written holding the
    /// digests the document currently stores. Digests the document lacks report `None`.
    ///
    /// For a document parsed from a file this checks the file's own digests.
    ///
    /// # Errors
    ///
    /// If the document cannot be laid out (see [`Vpk::build`]).
    pub fn verify_directory_md5(&self) -> Result<DirectoryMd5Report> {
        let Some(stored) = self.other_md5 else {
            return Ok(DirectoryMd5Report::default());
        };
        let lay = layout(self)?;
        let tree = tree_bytes(&lay)?;
        let section = records_bytes(self.archive_md5.as_deref().unwrap_or(&[]));
        let dir = assemble(
            self,
            &lay,
            &tree,
            &section,
            Some((stored.tree, stored.archive_md5_section)),
        )?;
        Ok(DirectoryMd5Report {
            tree: Some(md5(&tree) == stored.tree),
            archive_md5_section: Some(md5(&section) == stored.archive_md5_section),
            whole_file: dir.whole.map(|w| w == stored.whole_file),
        })
    }

    /// Check every archive MD5 record against the data it describes, reading the numbered
    /// archives it points into. This reads every byte the records cover; on a large pack
    /// that is the whole pack.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if an archive cannot be read, and [`Error::InvalidInput`] if the
    /// document has records for numbered archives and no source.
    pub fn verify_archive_md5(&self) -> Result<ArchiveMd5Report> {
        let mut report = ArchiveMd5Report::default();
        let Some(records) = &self.archive_md5 else {
            return Ok(report);
        };
        let mut open: Option<(u16, std::fs::File, PathBuf)> = None;
        for (i, r) in records.iter().enumerate() {
            let data = match r.target() {
                Md5Target::Unknown => {
                    report.unchecked.push(i);
                    continue;
                }
                Md5Target::Inline => self
                    .inline_data
                    .get(r.offset as usize..r.offset as usize + r.length as usize)
                    .map(<[u8]>::to_vec),
                Md5Target::Archive(n) => {
                    if open.as_ref().is_none_or(|(m, _, _)| *m != n) {
                        let path = self.archive_path(n)?;
                        open = Some((n, open_file(&path)?, path));
                    }
                    let Some((_, file, path)) = open.as_mut() else {
                        continue;
                    };
                    read_range(file, path, u64::from(r.offset), r.length as usize)?
                }
            };
            match data {
                Some(d) if md5(&d) == r.md5 => report.matched += 1,
                _ => report.mismatched.push(i),
            }
        }
        Ok(report)
    }
}

fn read_file(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn open_file(path: &Path) -> Result<std::fs::File> {
    std::fs::File::open(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Read a range without pulling the whole file into memory; `None` when it runs past the
/// end of the file.
pub(crate) fn read_range(
    f: &mut std::fs::File,
    path: &Path,
    offset: u64,
    len: usize,
) -> Result<Option<Vec<u8>>> {
    use std::io::{ErrorKind, Read, Seek, SeekFrom};

    let io = |source| Error::Io {
        path: path.to_path_buf(),
        source,
    };
    f.seek(SeekFrom::Start(offset)).map_err(io)?;
    let mut buf = vec![0u8; len];
    match f.read_exact(&mut buf) {
        Ok(()) => Ok(Some(buf)),
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => Ok(None),
        Err(e) => Err(io(e)),
    }
}
