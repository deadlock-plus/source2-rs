use std::fmt;

const VPK_SIGNATURE: u32 = 0x55aa_1234;
const RESOURCE_HEADER_VERSION: u16 = 12;
const KV3_MAGICS: [u32; 6] = [
    0x0356_4b56,
    0x4b56_3301,
    0x4b56_3302,
    0x4b56_3303,
    0x4b56_3304,
    0x4b56_3305,
];

const RESOURCE_HEADER_LEN: u64 = 16;
const RESOURCE_ENTRY_LEN: u64 = 12;
const DMX_LINE_LIMIT: usize = 512;

/// A file format [`detect`] can tell apart by its leading bytes.
///
/// `non_exhaustive`: match with a wildcard arm.
///
/// KeyValues 1 has no magic in either encoding, so it is never reported: its files come
/// back as [`Format::Unknown`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Format {
    /// A Valve Pak directory file.
    Vpk,
    /// A compiled-resource container.
    Resource,
    /// KeyValues 2 / DMX text (`keyvalues2`, `keyvalues2_noids`).
    Kv2Text,
    /// KeyValues 2 / DMX binary.
    Kv2Binary,
    /// KeyValues 3 text with its `<!-- kv3 ... -->` header.
    Kv3Text,
    /// A KeyValues 3 binary block, any revision.
    Kv3Binary,
    /// Nothing recognised, including every KeyValues 1 file.
    Unknown,
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Format::Vpk => "vpk",
            Format::Resource => "resource",
            Format::Kv2Text => "kv2 text",
            Format::Kv2Binary => "kv2 binary",
            Format::Kv3Text => "kv3 text",
            Format::Kv3Binary => "kv3 binary",
            Format::Unknown => "unknown",
        })
    }
}

/// Identify a file from its leading bytes.
///
/// Only magics and header lines the format crates themselves write are used, and the
/// answer is [`Format::Unknown`] rather than a guess. Nothing here parses a body, so a
/// match means "starts like this format", not "is valid".
///
/// A compiled resource has no magic: its first field is the file size. It is reported
/// only when the whole header and block table are consistent with the input length (header
/// version 12, table and every block inside the input), so a truncated resource is
/// `Unknown`, and a resource whose size happens to equal another format's magic is still
/// a `Resource`.
///
/// ```
/// use source2::{Format, detect};
///
/// assert_eq!(detect(b"<!-- kv3 encoding:text:version{e21c7f3c-8a33-41c5-9977-a76d3a32aa0d} format:generic:version{7412167c-06e9-4698-aff2-e63eb59037e7} -->\n{}"), Format::Kv3Text);
/// assert_eq!(detect(b"<!-- dmx encoding binary 5 format dmx 1 -->\n\0"), Format::Kv2Binary);
/// assert_eq!(detect(b"\"Key\" { \"a\" \"b\" }"), Format::Unknown);
/// ```
#[must_use]
pub fn detect(bytes: &[u8]) -> Format {
    // The resource check comes first: its leading size field can equal any 4-byte magic.
    if looks_like_resource(bytes) {
        return Format::Resource;
    }
    if let Some(magic) = le_u32(bytes, 0) {
        if magic == VPK_SIGNATURE {
            return Format::Vpk;
        }
        if KV3_MAGICS.contains(&magic) {
            return Format::Kv3Binary;
        }
    }
    if let Some(binary) = dmx_header(bytes) {
        return if binary {
            Format::Kv2Binary
        } else {
            Format::Kv2Text
        };
    }
    if has_kv3_text_header(bytes) {
        return Format::Kv3Text;
    }
    Format::Unknown
}

fn le_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let raw = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(raw.try_into().ok()?))
}

fn le_u16(bytes: &[u8], at: usize) -> Option<u16> {
    let raw = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes(raw.try_into().ok()?))
}

fn looks_like_resource(bytes: &[u8]) -> bool {
    let len = bytes.len() as u64;
    if len < RESOURCE_HEADER_LEN || le_u16(bytes, 4) != Some(RESOURCE_HEADER_VERSION) {
        return false;
    }
    let (Some(table_rel), Some(count)) = (le_u32(bytes, 8), le_u32(bytes, 12)) else {
        return false;
    };
    let (table_rel, count) = (u64::from(table_rel), u64::from(count));
    if table_rel < 8 {
        return false;
    }
    let table_start = 8 + table_rel;
    let Some(table_end) = count
        .checked_mul(RESOURCE_ENTRY_LEN)
        .and_then(|t| t.checked_add(table_start))
        .filter(|&end| end <= len)
    else {
        return false;
    };
    let mut cursor = table_end;
    for index in 0..count {
        let entry = table_start + index * RESOURCE_ENTRY_LEN;
        let (Some(rel), Some(length)) = (
            le_u32(bytes, (entry + 4) as usize),
            le_u32(bytes, (entry + 8) as usize),
        ) else {
            return false;
        };
        let offset = entry + 4 + u64::from(rel);
        let end = offset + u64::from(length);
        if offset < cursor || end > len {
            return false;
        }
        cursor = end;
    }
    true
}

/// `Some(true)` for a binary DMX header, `Some(false)` for a text one.
fn dmx_header(bytes: &[u8]) -> Option<bool> {
    let window = &bytes[..bytes.len().min(DMX_LINE_LIMIT)];
    let line = window.split(|&b| b == b'\n').next()?;
    let mut words = line
        .split(u8::is_ascii_whitespace)
        .filter(|w| !w.is_empty());
    if words.next()? != b"<!--" || words.next()? != b"dmx" || words.next()? != b"encoding" {
        return None;
    }
    Some(words.next()? == b"binary")
}

fn has_kv3_text_header(bytes: &[u8]) -> bool {
    let rest = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let rest = rest.trim_ascii_start();
    let Some(rest) = rest.strip_prefix(b"<!--") else {
        return false;
    };
    let Some(rest) = rest.trim_ascii_start().strip_prefix(b"kv3") else {
        return false;
    };
    match rest.first() {
        None => true,
        Some(b'-') => rest.starts_with(b"-->"),
        Some(b) => b.is_ascii_whitespace(),
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "vpk")]
    #[test]
    fn vpk_signature_matches_the_crate() {
        assert_eq!(super::VPK_SIGNATURE, source2_vpk::SIGNATURE);
    }

    #[cfg(feature = "resource")]
    #[test]
    fn resource_header_version_matches_the_crate() {
        assert_eq!(
            super::RESOURCE_HEADER_VERSION,
            source2_resource::HEADER_VERSION
        );
    }

    #[cfg(feature = "kv3")]
    #[test]
    fn kv3_magics_match_the_crate() {
        use source2_kv3::{MAGIC_LEGACY, MAGIC_V1, MAGIC_V2, MAGIC_V3, MAGIC_V4, MAGIC_V5};

        assert_eq!(
            super::KV3_MAGICS,
            [
                MAGIC_LEGACY,
                MAGIC_V1,
                MAGIC_V2,
                MAGIC_V3,
                MAGIC_V4,
                MAGIC_V5
            ]
        );
    }
}
