//! Pixel formats.

use std::fmt;

/// The pixel format stored in a texture header.
///
/// Numbers follow ValveResourceFormat. Only the formats seen in Deadlock textures are named.
/// Any other number is [`Format::Other`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Format {
    /// BC1 (DXT1): 4x4 blocks of 8 bytes.
    Bc1,
    /// BC3 (DXT5): 4x4 blocks of 16 bytes.
    Bc3,
    /// 8-bit red, green, blue, alpha.
    Rgba8888,
    /// A PNG file holding RGBA8888.
    PngRgba8888,
    /// A PNG file holding DXT5.
    PngDxt5,
    /// BC6H: 4x4 blocks of 16 bytes, half-float HDR.
    Bc6h,
    /// BC7: 4x4 blocks of 16 bytes.
    Bc7,
    /// 8-bit blue, green, red, alpha.
    Bgra8888,
    /// A format number this crate has no name for.
    Other(u8),
}

impl Format {
    /// The format for a header format byte.
    pub fn from_u8(n: u8) -> Self {
        match n {
            1 => Format::Bc1,
            2 => Format::Bc3,
            4 => Format::Rgba8888,
            16 => Format::PngRgba8888,
            18 => Format::PngDxt5,
            19 => Format::Bc6h,
            20 => Format::Bc7,
            28 => Format::Bgra8888,
            other => Format::Other(other),
        }
    }

    /// The header format byte for this format.
    pub fn to_u8(self) -> u8 {
        match self {
            Format::Bc1 => 1,
            Format::Bc3 => 2,
            Format::Rgba8888 => 4,
            Format::PngRgba8888 => 16,
            Format::PngDxt5 => 18,
            Format::Bc6h => 19,
            Format::Bc7 => 20,
            Format::Bgra8888 => 28,
            Format::Other(n) => n,
        }
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Format::Bc1 => f.write_str("BC1"),
            Format::Bc3 => f.write_str("BC3"),
            Format::Rgba8888 => f.write_str("RGBA8888"),
            Format::PngRgba8888 => f.write_str("PNG_RGBA8888"),
            Format::PngDxt5 => f.write_str("PNG_DXT5"),
            Format::Bc6h => f.write_str("BC6H"),
            Format::Bc7 => f.write_str("BC7"),
            Format::Bgra8888 => f.write_str("BGRA8888"),
            Format::Other(n) => write!(f, "unknown ({n})"),
        }
    }
}
