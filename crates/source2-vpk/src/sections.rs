//! The integrity sections that follow the file data in a version 2 directory file.

/// Size of one chunk in the archive MD5 section: 1 MiB, the last chunk of a range
/// shorter. Shipped files chunk the inline data section this way (verified) and tile each
/// numbered archive with records of this size.
pub const MD5_CHUNK_SIZE: u32 = 1 << 20;

/// Bit set in [`ArchiveMd5::archive_index`] for chunks of the directory file's data
/// section.
pub const MD5_INLINE_FLAG: u32 = 0x8000_0000;

/// What an [`ArchiveMd5`] record hashes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Md5Target {
    /// A range of numbered archive `n`.
    Archive(u16),
    /// A range of the directory file's data section.
    Inline,
    /// An index this crate cannot check. The record is kept and written back as found.
    Unknown,
}

/// One record of the archive MD5 section: the MD5 of a range of data.
///
/// Records come one per [`MD5_CHUNK_SIZE`] chunk of each numbered archive and of the
/// inline data section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveMd5 {
    /// The raw index field. See [`ArchiveMd5::target`].
    pub archive_index: u32,
    /// Offset of the hashed range.
    pub offset: u32,
    /// Length of the hashed range.
    pub length: u32,
    /// MD5 of the range.
    pub md5: [u8; 16],
}

impl ArchiveMd5 {
    /// The record for the range `offset..offset+length` of numbered archive `archive`,
    /// with the plain archive number as its index.
    pub fn for_archive(archive: u16, offset: u32, length: u32, md5: [u8; 16]) -> Self {
        ArchiveMd5 {
            archive_index: u32::from(archive),
            offset,
            length,
            md5,
        }
    }

    /// The record for a range of the inline data section.
    pub fn for_inline(offset: u32, length: u32, md5: [u8; 16]) -> Self {
        ArchiveMd5 {
            archive_index: MD5_INLINE_FLAG,
            offset,
            length,
            md5,
        }
    }

    /// What the record hashes.
    ///
    /// - `0x80000000` is the inline data section. Verified against shipped files: the
    ///   digests are plain MD5 of the section's 1 MiB chunks.
    /// - A plain archive number `n` is numbered archive `n`, as the format description
    ///   has it. Not verified against a shipped file.
    /// - Anything else is [`Md5Target::Unknown`]. That includes `0x10000 | n`, the
    ///   spelling a Source 2 title ships for every numbered archive: its records tile
    ///   each archive file exactly, but their digests are not the MD5 of those bytes (no
    ///   plain-MD5 reading of the data reproduces them), so this crate does not claim to
    ///   check them. They are read and written back untouched.
    pub fn target(&self) -> Md5Target {
        if self.archive_index == MD5_INLINE_FLAG {
            return Md5Target::Inline;
        }
        match u16::try_from(self.archive_index) {
            Ok(n) if n != 0x7fff => Md5Target::Archive(n),
            _ => Md5Target::Unknown,
        }
    }
}

/// The "other MD5" section: digests of the directory file's own parts.
///
/// Writing recomputes all three from the bytes being written, so they always describe
/// the file they sit in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OtherMd5 {
    /// MD5 of the directory tree.
    pub tree: [u8; 16],
    /// MD5 of the archive MD5 section.
    pub archive_md5_section: [u8; 16],
    /// MD5 of everything in the directory file before this digest: header, tree, data
    /// section, archive MD5 section and the two digests above.
    pub whole_file: [u8; 16],
}

/// How a [`Signature`] is laid out in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SignatureLayout {
    /// `u32 key length, key, u32 signature length, signature`, all inside the section.
    ///
    /// This is the layout Valve's published format notes describe. No sample file was
    /// available to check it against.
    Classic,
    /// A 20-byte header inside the section, `u32` [`SIGNATURE`](crate::SIGNATURE), version, key length,
    /// signature length and a reserved word, with the key and signature following the
    /// section at the end of the file. This is what Source 2 titles ship; the section
    /// size in the file header counts only the 20 bytes.
    Headed {
        /// The header's version word (1 in shipped files).
        version: u32,
        /// The header's reserved word (0 in shipped files).
        reserved: u32,
    },
}

/// The RSA public key and signature section.
///
/// Both are kept as the opaque bytes in the file (the key is DER `SubjectPublicKeyInfo`
/// in shipped files). A signed document written back unchanged stays valid; changing any
/// entry, or anything the signature covers, invalidates it. This crate cannot sign: that
/// needs the publisher's private key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    /// The public key bytes. Empty in an unsigned `Headed` section.
    pub public_key: Vec<u8>,
    /// The signature bytes. Empty in an unsigned `Headed` section.
    pub signature: Vec<u8>,
    /// How the section is laid out.
    pub layout: SignatureLayout,
}

impl Signature {
    /// An unsigned section in the shipped `Headed` layout: present, but with no key.
    pub fn unsigned() -> Self {
        Signature {
            public_key: Vec::new(),
            signature: Vec::new(),
            layout: SignatureLayout::Headed {
                version: 1,
                reserved: 0,
            },
        }
    }
}

/// Result of [`Vpk::verify_directory_md5`](crate::Vpk::verify_directory_md5). `None`
/// means the document has no such digest to check.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct DirectoryMd5Report {
    /// The tree digest matches the tree.
    pub tree: Option<bool>,
    /// The section digest matches the archive MD5 section.
    pub archive_md5_section: Option<bool>,
    /// The whole-file digest matches the directory file.
    pub whole_file: Option<bool>,
}

impl DirectoryMd5Report {
    /// Whether every digest that exists matched.
    pub fn all_ok(&self) -> bool {
        [self.tree, self.archive_md5_section, self.whole_file]
            .into_iter()
            .flatten()
            .all(|ok| ok)
    }
}

/// Result of [`Vpk::verify_archive_md5`](crate::Vpk::verify_archive_md5).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ArchiveMd5Report {
    /// Records whose digest matched.
    pub matched: usize,
    /// Indices into the record list whose digest did not match the data.
    pub mismatched: Vec<usize>,
    /// Indices of records that could not be checked ([`Md5Target::Unknown`]).
    pub unchecked: Vec<usize>,
}

impl ArchiveMd5Report {
    /// Whether no record mismatched.
    pub fn all_ok(&self) -> bool {
        self.mismatched.is_empty()
    }
}
