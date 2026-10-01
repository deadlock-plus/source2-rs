#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod entry;
pub mod error;
mod md5;
mod parse;
mod sections;
mod vpk;
mod write;

pub use entry::{Contents, Data, Entry};
pub use error::{Error, Result};
pub use sections::{
    ArchiveMd5, ArchiveMd5Report, DirectoryMd5Report, MD5_CHUNK_SIZE, MD5_INLINE_FLAG, Md5Target,
    OtherMd5, Signature, SignatureLayout,
};
pub use vpk::Vpk;
pub use write::{BuiltArchive, BuiltVpk};

/// Magic number every VPK starts with.
pub const SIGNATURE: u32 = 0x55aa_1234;

/// `archive_index` value meaning "the bytes are in the directory file itself".
pub(crate) const ARCHIVE_INLINE: u16 = 0x7fff;

/// Terminator written after each directory entry.
pub(crate) const ENTRY_TERMINATOR: u16 = 0xffff;

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

static CRC_TABLE: [u32; 256] = crc_table();

/// CRC32, the IEEE polynomial VPK records for each entry.
pub(crate) fn crc32(data: &[u8]) -> u32 {
    crc32_update(0, data)
}

/// Continue a CRC32 over more data; start from 0.
pub(crate) fn crc32_update(crc: u32, data: &[u8]) -> u32 {
    let mut crc = !crc;
    for &b in data {
        crc = (crc >> 8) ^ CRC_TABLE[usize::from((crc as u8) ^ b)];
    }
    !crc
}

#[cfg(test)]
mod real_tests;
#[cfg(test)]
mod round_trip_tests;
#[cfg(test)]
mod section_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod write_tests;
