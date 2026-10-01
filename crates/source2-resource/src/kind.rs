//! Four-character block tags.

/// The four-character tag that names a block, e.g. `DATA`.
///
/// Any four bytes are accepted. The constants cover every tag seen in real compiled
/// resources; a tag without a constant is still read and written unchanged. Constants
/// marked "not decoded" are named only so callers can find them; this crate does not
/// interpret their contents.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockKind(pub [u8; 4]);

impl BlockKind {
    /// Build a tag from its four bytes.
    pub const fn new(tag: [u8; 4]) -> Self {
        BlockKind(tag)
    }

    /// The raw tag bytes.
    pub const fn as_bytes(&self) -> &[u8; 4] {
        &self.0
    }

    /// The main payload; its encoding depends on the resource type. Not decoded.
    pub const DATA: Self = Self(*b"DATA");
    /// External resource references. Not decoded.
    pub const RERL: Self = Self(*b"RERL");
    /// Edit info (compiler inputs and settings), newer layout. Not decoded.
    pub const RED2: Self = Self(*b"RED2");
    /// Edit info, older layout. Not decoded.
    pub const REDI: Self = Self(*b"REDI");
    /// Introspection of the structs the payload uses. Not decoded.
    pub const NTRO: Self = Self(*b"NTRO");
    /// Seen in real files. Not decoded.
    pub const STAT: Self = Self(*b"STAT");
    /// Seen in real files. Not decoded.
    pub const FLCI: Self = Self(*b"FLCI");
    /// Seen in real files. Not decoded.
    pub const CTRL: Self = Self(*b"CTRL");
    /// Seen in real files. Not decoded.
    pub const MBUF: Self = Self(*b"MBUF");
    /// Seen in real files. Not decoded.
    pub const MDAT: Self = Self(*b"MDAT");
    /// Seen in real files. Not decoded.
    pub const MIDX: Self = Self(*b"MIDX");
    /// Seen in real files. Not decoded.
    pub const MVTX: Self = Self(*b"MVTX");
    /// Seen in real files. Not decoded.
    pub const MRPH: Self = Self(*b"MRPH");
    /// Seen in real files. Not decoded.
    pub const PHYS: Self = Self(*b"PHYS");
    /// Seen in real files. Not decoded.
    pub const ANIM: Self = Self(*b"ANIM");
    /// Seen in real files. Not decoded.
    pub const AGRP: Self = Self(*b"AGRP");
    /// Seen in real files. Not decoded.
    pub const ASEQ: Self = Self(*b"ASEQ");
    /// Seen in real files. Not decoded.
    pub const DSTF: Self = Self(*b"DSTF");
    /// Seen in real files. Not decoded.
    pub const INSG: Self = Self(*b"INSG");
    /// Seen in real files (`LaCo`). Not decoded.
    pub const LACO: Self = Self(*b"LaCo");
    /// Seen in real files (`SrMa`). Not decoded.
    pub const SRMA: Self = Self(*b"SrMa");
    /// Seen in real files. Not decoded.
    pub const SNAP: Self = Self(*b"SNAP");
    /// Seen in real files. Not decoded.
    pub const TBUF: Self = Self(*b"TBUF");
    /// Seen in real files. Not decoded.
    pub const VBIB: Self = Self(*b"VBIB");
}

impl From<[u8; 4]> for BlockKind {
    fn from(tag: [u8; 4]) -> Self {
        BlockKind(tag)
    }
}

impl std::fmt::Display for BlockKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&String::from_utf8_lossy(&self.0))
    }
}

impl std::fmt::Debug for BlockKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BlockKind({:?})", String::from_utf8_lossy(&self.0))
    }
}
