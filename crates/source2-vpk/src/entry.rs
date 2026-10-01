//! One file inside an archive.

use crate::{Error, Result, crc32};

/// Where an entry's bytes (past the preload) live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Data {
    /// Bytes held in memory, not yet placed. Writing the document lays them out: in the
    /// directory file's own data section when `inline` is set, otherwise in numbered
    /// archives, split at [`Vpk::max_archive_size`](crate::Vpk::max_archive_size).
    ///
    /// This is what hand-built entries use.
    Memory {
        /// The file's bytes after the preload.
        bytes: Vec<u8>,
        /// Store in the directory file's data section instead of a numbered archive.
        inline: bool,
    },
    /// Bytes already placed, exactly as the directory records them. This is what parsing
    /// produces; the bytes stay in the archive file (or the document's inline data) until
    /// read.
    Stored {
        /// The numbered archive holding the bytes, or `None` when they are in the
        /// directory file's own data section.
        archive: Option<u16>,
        /// Offset within that archive, or within the data section when inline. Empty
        /// files conventionally record `u32::MAX` here.
        offset: u32,
        /// How many bytes live there, excluding the entry's preload.
        length: u32,
    },
}

/// One file inside an archive.
///
/// Parsing fills every field; [`Entry::new`] builds one by hand with sensible defaults
/// (no preload, numbered archives, CRC computed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Full path inside the archive, e.g. `docs/readme.txt`. Forward slashes, no leading
    /// or trailing slash.
    pub path: String,
    /// CRC32 of the whole file (preload and the rest), as the directory records it.
    /// Written verbatim; [`Entry::new`] computes it, and [`Entry::refresh_crc`]
    /// recomputes it after the bytes change.
    pub crc: u32,
    /// Bytes stored directly in the directory tree, ahead of the rest. At most 65535.
    pub preload: Vec<u8>,
    /// Where the rest of the bytes live.
    pub data: Data,
}

impl Entry {
    /// A file whose bytes go to the numbered archives when the document is written.
    pub fn new(path: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        let bytes = bytes.into();
        Entry {
            path: path.into(),
            crc: crc32(&bytes),
            preload: Vec::new(),
            data: Data::Memory {
                bytes,
                inline: false,
            },
        }
    }

    /// A file stored in the directory file's own data section, so a pack made only of
    /// these needs no numbered archives.
    pub fn inline(path: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        let mut e = Entry::new(path, bytes);
        if let Data::Memory { inline, .. } = &mut e.data {
            *inline = true;
        }
        e
    }

    /// Move the first `len` bytes of the file into the directory tree as its preload.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidInput`] if the entry is already placed, `len` is more than the
    /// file holds, or the preload would exceed 65535 bytes.
    pub fn with_preload_len(mut self, len: usize) -> Result<Self> {
        let Data::Memory { bytes, .. } = &mut self.data else {
            return Err(Error::InvalidInput(format!(
                "{}: cannot re-split an entry that is already placed",
                self.path
            )));
        };
        let total = self.preload.len() + bytes.len();
        if len > total {
            return Err(Error::InvalidInput(format!(
                "{}: preload of {len} bytes is larger than the {total}-byte file",
                self.path
            )));
        }
        if len > usize::from(u16::MAX) {
            return Err(Error::InvalidInput(format!(
                "{}: preload of {len} bytes exceeds 65535",
                self.path
            )));
        }
        let mut whole = std::mem::take(&mut self.preload);
        whole.extend_from_slice(bytes);
        *bytes = whole.split_off(len);
        self.preload = whole;
        Ok(self)
    }

    /// Total size of the file: preload plus the rest.
    pub fn size(&self) -> usize {
        self.preload.len() + self.data_len()
    }

    /// Bytes held outside the preload.
    pub fn data_len(&self) -> usize {
        match &self.data {
            Data::Memory { bytes, .. } => bytes.len(),
            Data::Stored { length, .. } => *length as usize,
        }
    }

    /// Recompute [`Entry::crc`] from the bytes held in memory.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidInput`] if the bytes are still in an archive; read them with
    /// [`Vpk::read`](crate::Vpk::read) instead.
    pub fn refresh_crc(&mut self) -> Result<()> {
        let Data::Memory { bytes, .. } = &self.data else {
            return Err(Error::InvalidInput(format!(
                "{}: bytes are not in memory",
                self.path
            )));
        };
        let mut whole = Vec::with_capacity(self.size());
        whole.extend_from_slice(&self.preload);
        whole.extend_from_slice(bytes);
        self.crc = crc32(&whole);
        Ok(())
    }

    /// The extension, without the dot, or `None` for a name without one.
    pub fn extension(&self) -> Option<&str> {
        let file = self.path.rsplit('/').next().unwrap_or(&self.path);
        file.rsplit_once('.').map(|(_, ext)| ext)
    }
}

/// A file's bytes together with how its CRC fared.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Contents {
    /// The file's bytes, preload first.
    pub bytes: Vec<u8>,
    /// CRC the directory recorded.
    pub expected_crc: u32,
    /// CRC of `bytes`.
    pub actual_crc: u32,
}

impl Contents {
    /// Whether the bytes match the recorded CRC.
    pub fn crc_ok(&self) -> bool {
        self.expected_crc == self.actual_crc
    }
}
