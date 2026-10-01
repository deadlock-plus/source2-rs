//! Element ids.

use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A 128-bit element id.
///
/// Bytes are kept in the order of the hyphenated text form, `8-4-4-4-12` hex digits. The
/// binary encodings store the first three groups little-endian, like a Windows `GUID`;
/// [`Uuid::from_guid_bytes`] and [`Uuid::to_guid_bytes`] convert.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Uuid(pub [u8; 16]);

impl Uuid {
    /// The all-zero id.
    pub const NIL: Uuid = Uuid([0; 16]);

    /// Builds an id from a number, most significant digit first like the text form.
    pub const fn from_u128(n: u128) -> Self {
        Uuid(n.to_be_bytes())
    }

    /// The id as a number, the inverse of [`Uuid::from_u128`].
    pub const fn as_u128(self) -> u128 {
        u128::from_be_bytes(self.0)
    }

    /// A fresh version 4 id, using only the standard library.
    ///
    /// Bits come from the process's randomly keyed hasher, the clock and a counter. Ids
    /// do not collide in practice; do not rely on them being unpredictable.
    pub fn generate() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let mut out = [0u8; 16];
        for (half, chunk) in out.chunks_mut(8).enumerate() {
            let mut h = RandomState::new().build_hasher();
            h.write_u64(n);
            h.write_u128(now);
            h.write_usize(half);
            chunk.copy_from_slice(&h.finish().to_be_bytes());
        }
        out[6] = out[6] & 0x0f | 0x40;
        out[8] = out[8] & 0x3f | 0x80;
        Uuid(out)
    }

    /// Builds an id from the 16 bytes a binary document stores.
    pub fn from_guid_bytes(b: [u8; 16]) -> Self {
        let mut o = b;
        o[0..4].reverse();
        o[4..6].reverse();
        o[6..8].reverse();
        Uuid(o)
    }

    /// The 16 bytes a binary document stores for this id.
    pub fn to_guid_bytes(self) -> [u8; 16] {
        let mut o = self.0;
        o[0..4].reverse();
        o[4..6].reverse();
        o[6..8].reverse();
        o
    }

    /// Parses the hyphenated form, case-insensitively.
    pub fn parse(s: &str) -> Option<Uuid> {
        let b = s.as_bytes();
        if b.len() != 36 {
            return None;
        }
        let mut out = [0u8; 16];
        let mut n = 0;
        let mut i = 0;
        while i < 36 {
            if matches!(i, 8 | 13 | 18 | 23) {
                if b[i] != b'-' {
                    return None;
                }
                i += 1;
                continue;
            }
            let hi = hex(b[i])?;
            let lo = hex(b[i + 1])?;
            out[n] = hi << 4 | lo;
            n += 1;
            i += 2;
        }
        Some(Uuid(out))
    }
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, byte) in self.0.iter().enumerate() {
            if matches!(i, 4 | 6 | 8 | 10) {
                f.write_str("-")?;
            }
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for Uuid {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        Uuid::parse(s).ok_or(())
    }
}
