//! Directory file parser.

use crate::write::split_path;
use crate::{
    ARCHIVE_INLINE, ArchiveMd5, Data, ENTRY_TERMINATOR, Entry, Error, OtherMd5, Result, SIGNATURE,
    Signature, SignatureLayout, Vpk,
};

/// Size of one archive MD5 record.
pub(crate) const ARCHIVE_MD5_RECORD: usize = 28;

/// Size of the "other MD5" section: three digests.
pub(crate) const OTHER_MD5_SECTION: usize = 48;

/// Size of the `Headed` signature section header.
pub(crate) const SIGNATURE_HEADER: usize = 20;

pub(crate) fn parse(mut bytes: Vec<u8>, lossy: bool) -> Result<Vpk> {
    let mut r = Cursor::new(&bytes);
    let signature = r.u32()?;
    if signature != SIGNATURE {
        return Err(Error::NotAVpk { found: signature });
    }
    let version = r.u32()?;
    let tree_size = r.u32()? as usize;

    // Section sizes after the tree: data, archive MD5, other MD5, signature.
    let mut sizes = [0usize; 4];
    match version {
        1 => {}
        2 => {
            for s in &mut sizes {
                *s = r.u32()? as usize;
            }
        }
        v => return Err(Error::UnsupportedVersion(v)),
    }
    let [mut data_size, md5_size, other_size, signature_size] = sizes;

    let tree_start = r.pos();
    let data_start = tree_start
        .checked_add(tree_size)
        .filter(|&end| end <= bytes.len())
        .ok_or_else(|| Error::Malformed(format!("tree of {tree_size} bytes runs past the file")))?;
    if version == 1 {
        data_size = bytes.len() - data_start;
    }

    let md5_start = data_start.checked_add(data_size);
    let other_start = md5_start.and_then(|s| s.checked_add(md5_size));
    let sig_start = other_start.and_then(|s| s.checked_add(other_size));
    let sig_end = sig_start.and_then(|s| s.checked_add(signature_size));
    let (Some(md5_start), Some(other_start), Some(sig_start), Some(sig_end)) =
        (md5_start, other_start, sig_start, sig_end)
    else {
        return Err(Error::Malformed("section sizes overflow".into()));
    };
    if sig_end > bytes.len() {
        return Err(Error::Malformed(format!(
            "sections end at {sig_end}, file is {} bytes",
            bytes.len()
        )));
    }

    let mut tree = Cursor::new(&bytes[tree_start..data_start]);
    let entries = parse_tree(&mut tree, lossy)?;
    if tree.pos() != tree_size {
        return Err(Error::Malformed(format!(
            "tree ends after {} of {tree_size} bytes",
            tree.pos()
        )));
    }

    let (archive_md5, other_md5, signature) = if version == 2 {
        if md5_size % ARCHIVE_MD5_RECORD != 0 {
            return Err(Error::Malformed(format!(
                "archive MD5 section of {md5_size} bytes is not a whole number of records"
            )));
        }
        let mut c = Cursor::new(&bytes[md5_start..other_start]);
        let mut records = Vec::with_capacity(md5_size / ARCHIVE_MD5_RECORD);
        while c.pos() < md5_size {
            records.push(ArchiveMd5 {
                archive_index: c.u32()?,
                offset: c.u32()?,
                length: c.u32()?,
                md5: c.digest()?,
            });
        }

        let other = match other_size {
            0 => None,
            OTHER_MD5_SECTION => {
                let mut c = Cursor::new(&bytes[other_start..sig_start]);
                Some(OtherMd5 {
                    tree: c.digest()?,
                    archive_md5_section: c.digest()?,
                    whole_file: c.digest()?,
                })
            }
            n => {
                return Err(Error::Malformed(format!(
                    "other MD5 section of {n} bytes; expected 0 or {OTHER_MD5_SECTION}"
                )));
            }
        };

        let signature = parse_signature(&bytes[sig_start..sig_end], &bytes[sig_end..])?;
        (Some(records), other, signature)
    } else {
        (None, None, None)
    };

    bytes.truncate(data_start + data_size);
    bytes.drain(..data_start);
    Ok(Vpk::from_parts(
        version,
        entries,
        bytes,
        archive_md5,
        other_md5,
        signature,
    ))
}

/// `section` is the bytes the header sizes account for; `tail` is whatever follows it.
fn parse_signature(section: &[u8], tail: &[u8]) -> Result<Option<Signature>> {
    if section.is_empty() {
        return if tail.is_empty() {
            Ok(None)
        } else {
            Err(Error::Malformed(format!(
                "{} unaccounted bytes at the end of the file",
                tail.len()
            )))
        };
    }

    let mut c = Cursor::new(section);
    if section.len() == SIGNATURE_HEADER && c.u32()? == SIGNATURE {
        let version = c.u32()?;
        let key_len = c.u32()? as usize;
        let sig_len = c.u32()? as usize;
        let reserved = c.u32()?;
        if key_len.checked_add(sig_len) != Some(tail.len()) {
            return Err(Error::Malformed(format!(
                "signature header wants {key_len} + {sig_len} bytes, {} follow",
                tail.len()
            )));
        }
        return Ok(Some(Signature {
            public_key: tail[..key_len].to_vec(),
            signature: tail[key_len..].to_vec(),
            layout: SignatureLayout::Headed { version, reserved },
        }));
    }

    if !tail.is_empty() {
        return Err(Error::Malformed(format!(
            "{} unaccounted bytes at the end of the file",
            tail.len()
        )));
    }
    let mut c = Cursor::new(section);
    let key_len = c.u32()? as usize;
    let public_key = c.take(key_len)?.to_vec();
    let sig_len = c.u32()? as usize;
    let signature = c.take(sig_len)?.to_vec();
    if c.pos() != section.len() {
        return Err(Error::Malformed(
            "signature section has bytes after the signature".into(),
        ));
    }
    Ok(Some(Signature {
        public_key,
        signature,
        layout: SignatureLayout::Classic,
    }))
}

/// Walk the extension / directory / filename nesting the tree is built from.
fn parse_tree(r: &mut Cursor<'_>, lossy: bool) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    loop {
        let extension = r.name(lossy)?;
        if extension.is_empty() {
            break;
        }
        loop {
            let directory = r.name(lossy)?;
            if directory.is_empty() {
                break;
            }
            loop {
                let name = r.name(lossy)?;
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
                let preload = r.take(usize::from(preload_len))?.to_vec();

                // A single space stands for the archive root and for no extension, since
                // an empty string ends a level of the tree.
                let file = match extension.as_str() {
                    " " => name.clone(),
                    e => format!("{name}.{e}"),
                };
                let path = match directory.as_str() {
                    " " => file,
                    d => format!("{d}/{file}"),
                };
                match split_path(&path) {
                    Ok((e, d, n)) if e == extension && d == directory && n == name => {}
                    _ => {
                        return Err(Error::Malformed(format!(
                            "{path:?} does not split back into {extension:?}, {directory:?} \
                             and {name:?}"
                        )));
                    }
                }

                entries.push(Entry {
                    path,
                    crc,
                    preload,
                    data: Data::Stored {
                        archive: (archive_index != ARCHIVE_INLINE).then_some(archive_index),
                        offset,
                        length,
                    },
                });
            }
        }
    }
    Ok(entries)
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

    fn digest(&mut self) -> Result<[u8; 16]> {
        let mut d = [0u8; 16];
        d.copy_from_slice(self.take(16)?);
        Ok(d)
    }

    /// A null-terminated string. Bytes that are not UTF-8 are an error, or replaced when
    /// `lossy`.
    fn name(&mut self, lossy: bool) -> Result<String> {
        let rest = &self.bytes[self.pos.min(self.bytes.len())..];
        let n = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| Error::Malformed(format!("unterminated string at {}", self.pos)))?;
        let raw = &rest[..n];
        self.pos += n + 1;
        match std::str::from_utf8(raw) {
            Ok(s) => Ok(s.to_owned()),
            Err(_) if lossy => Ok(String::from_utf8_lossy(raw).into_owned()),
            Err(_) => Err(Error::InvalidName {
                lossy: String::from_utf8_lossy(raw).into_owned(),
            }),
        }
    }
}
