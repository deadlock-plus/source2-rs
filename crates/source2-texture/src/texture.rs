//! The texture header, mip layout and decoding.

use crate::bc;
use crate::error::{Error, Result};
use crate::format::Format;

const HEADER_LEN: usize = 40;
const EXTRA_ENTRY_LEN: usize = 12;
const EXTRA_TABLE_BASE: u64 = 32;
const EXTRA_COMPRESSED_MIPS: u32 = 3;

const FLAG_CUBE: u16 = 0x0400;
const FLAG_VOLUME: u16 = 0x0800;
const FLAG_ARRAY: u16 = 0x1000;

/// One entry of the header extra-data table. The values are stored as read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Extra {
    /// The entry type. Type 1 is a 1024-byte fallback blob.
    pub kind: u32,
    /// The offset field.
    pub offset: u32,
    /// The size field.
    pub size: u32,
}

/// The 40-byte header at the start of a `vtex_c` `DATA` block, and its extra-data table.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Header {
    /// Header version. Seen as 1.
    pub version: u16,
    /// Flag bits, as stored.
    pub flags: u16,
    /// Average colour, four floats.
    pub reflectivity: [f32; 4],
    /// Width of the largest mip, in pixels.
    pub width: u16,
    /// Height of the largest mip, in pixels.
    pub height: u16,
    /// Depth. 1 for a plain 2D texture.
    pub depth: u16,
    /// Pixel format.
    pub format: Format,
    /// Number of mips.
    pub mip_count: u8,
    /// The picmip-0 resolution field, as stored.
    pub picmip0_res: u32,
    /// The extra-data table, as stored.
    pub extra: Vec<Extra>,
}

fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

impl Header {
    /// Reads the header from a `DATA` block. Any pixel format is accepted here.
    ///
    /// # Errors
    ///
    /// [`Error::Truncated`] if the block or its extra-data table does not fit, and
    /// [`Error::ZeroDimension`] if the width, height, depth or mip count is zero.
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < HEADER_LEN {
            return Err(Error::Truncated {
                what: "header",
                needed: HEADER_LEN as u64,
                available: data.len() as u64,
            });
        }
        let mut reflectivity = [0f32; 4];
        for (i, r) in reflectivity.iter_mut().enumerate() {
            *r = f32::from_bits(le32(data, 4 + 4 * i));
        }
        let header = Header {
            version: le16(data, 0),
            flags: le16(data, 2),
            reflectivity,
            width: le16(data, 20),
            height: le16(data, 22),
            depth: le16(data, 24),
            format: Format::from_u8(data[26]),
            mip_count: data[27],
            picmip0_res: le32(data, 28),
            extra: Self::parse_extra(data)?,
        };
        for (value, what) in [
            (header.width, "width"),
            (header.height, "height"),
            (header.depth, "depth"),
            (u16::from(header.mip_count), "mip count"),
        ] {
            if value == 0 {
                return Err(Error::ZeroDimension { what });
            }
        }
        Ok(header)
    }

    fn parse_extra(data: &[u8]) -> Result<Vec<Extra>> {
        let start = EXTRA_TABLE_BASE + u64::from(le32(data, 32));
        let needed = start + u64::from(le32(data, 36)) * EXTRA_ENTRY_LEN as u64;
        if needed > data.len() as u64 {
            return Err(Error::Truncated {
                what: "extra data table",
                needed,
                available: data.len() as u64,
            });
        }
        // `needed` is at most `data.len()`, so both bounds fit in `usize`.
        let table = &data[start as usize..needed as usize];
        Ok(table
            .as_chunks::<EXTRA_ENTRY_LEN>()
            .0
            .iter()
            .map(|e| Extra {
                kind: le32(e, 0),
                offset: le32(e, 4),
                size: le32(e, 8),
            })
            .collect())
    }
}

/// A decoded image: 8-bit RGBA, rows top to bottom, no padding.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes: red, green, blue, alpha per pixel.
    pub rgba: Vec<u8>,
}

impl Image {
    /// Converts scaled `YCoCg` to RGB in place and sets alpha to 255.
    ///
    /// Some BC3 textures store colour as scaled `YCoCg` instead of RGB. After BC3 decoding the
    /// red channel is Co, green is Cg, blue holds a scale byte and alpha is Y. With
    /// `s = (blue >> 3) + 1`, `Co = (red - 128) / s` and `Cg = (green - 128) / s`, the colour
    /// is `R = Y + Co - Cg`, `G = Y + Cg`, `B = Y - Co - Cg`. Results are rounded to nearest
    /// (halves up) and clamped to 0..=255.
    ///
    /// This never runs by itself: the pixels cannot say whether they are `YCoCg`. The texture's
    /// resource marks it (see the crate README), and the caller decides. Calling it on an
    /// ordinary RGBA image scrambles the colours.
    pub fn ycocg_to_rgb(&mut self) {
        for px in self.rgba.as_chunks_mut::<4>().0.iter_mut() {
            let [co, cg, scale, y] = px.map(i32::from);
            let s = (scale >> 3) + 1;
            let (co, cg) = (co - 128, cg - 128);
            let div = |n: i32| (2 * n + s).div_euclid(2 * s);
            *px = [
                (y + div(co - cg)).clamp(0, 255) as u8,
                (y + div(cg)).clamp(0, 255) as u8,
                (y - div(co + cg)).clamp(0, 255) as u8,
                255,
            ];
        }
    }
}

/// A texture ready to decode: a header plus its pixel bytes.
///
/// The pixel bytes are not in the `DATA` block. In a compiled resource they follow the last
/// block (`Resource::trailing` in `source2-resource`).
#[derive(Debug, Clone)]
pub struct Texture<'a> {
    header: Header,
    pixels: &'a [u8],
    mip_sizes: Vec<usize>,
}

fn mip_dim(base: u16, level: u32) -> u32 {
    u32::from(base).checked_shr(level).unwrap_or(0).max(1)
}

fn mip_size(format: Format, width: u32, height: u32) -> Option<u64> {
    let (w, h) = (u64::from(width), u64::from(height));
    match format {
        Format::Bgra8888 | Format::Rgba8888 => Some(w * h * 4),
        Format::Bc1 => Some(w.div_ceil(4) * h.div_ceil(4) * 8),
        Format::Bc3 => Some(w.div_ceil(4) * h.div_ceil(4) * 16),
        _ => None,
    }
}

fn decode_embedded_png(data: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    let bad = |reason: String| Error::BadEmbeddedImage { reason };
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().map_err(|e| bad(e.to_string()))?;
    let (w, h) = reader.info().size();
    if (w, h) != (width, height) {
        return Err(bad(format!("is {w}x{h}, the header says {width}x{height}")));
    }
    let mut buf = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or_else(|| bad("too large".into()))?
    ];
    let frame = reader
        .next_frame(&mut buf)
        .map_err(|e| bad(e.to_string()))?;
    if frame.bit_depth != png::BitDepth::Eight {
        return Err(bad(format!("bit depth {:?}", frame.bit_depth)));
    }
    buf.truncate(frame.buffer_size());
    match frame.color_type {
        png::ColorType::Rgba => Ok(buf),
        png::ColorType::Rgb => Ok(buf
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect()),
        png::ColorType::Grayscale => Ok(buf.iter().flat_map(|&g| [g, g, g, 255]).collect()),
        png::ColorType::GrayscaleAlpha => Ok(buf
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect()),
        png::ColorType::Indexed => Err(bad("indexed colour left unexpanded".into())),
    }
}

impl<'a> Texture<'a> {
    /// Reads a 2D texture of a supported format.
    ///
    /// `data_block` is the `DATA` block. `pixels` is every mip, smallest first, with nothing
    /// before or after.
    ///
    /// # Errors
    ///
    /// Anything [`Header::parse`] returns, plus [`Error::UnsupportedCompressedMips`],
    /// [`Error::UnsupportedLayout`] for cube maps, volumes and arrays,
    /// [`Error::UnsupportedFormat`], and [`Error::BadPixelLength`] when `pixels` is not exactly
    /// as long as the mips need.
    pub fn parse(data_block: &[u8], pixels: &'a [u8]) -> Result<Self> {
        let header = Header::parse(data_block)?;
        if header.extra.iter().any(|e| e.kind == EXTRA_COMPRESSED_MIPS) {
            return Err(Error::UnsupportedCompressedMips);
        }
        if header.flags & FLAG_CUBE != 0 {
            return Err(Error::UnsupportedLayout { kind: "cube map" });
        }
        if header.flags & FLAG_ARRAY != 0 {
            return Err(Error::UnsupportedLayout { kind: "array" });
        }
        if header.flags & FLAG_VOLUME != 0 || header.depth > 1 {
            return Err(Error::UnsupportedLayout { kind: "volume" });
        }
        if matches!(header.format, Format::PngRgba8888 | Format::PngDxt5) {
            // The mip is a PNG file of whatever length; there is no size to derive.
            if header.mip_count != 1 {
                return Err(Error::UnsupportedFormat {
                    format: header.format,
                });
            }
            return Ok(Texture {
                header,
                pixels,
                mip_sizes: vec![pixels.len()],
            });
        }
        let mut sizes = Vec::with_capacity(usize::from(header.mip_count));
        let mut total = 0u64;
        for level in 0..u32::from(header.mip_count) {
            let width = mip_dim(header.width, level);
            let height = mip_dim(header.height, level);
            let size = mip_size(header.format, width, height).ok_or(Error::UnsupportedFormat {
                format: header.format,
            })?;
            total += size;
            sizes.push(size);
        }
        if total != pixels.len() as u64 {
            return Err(Error::BadPixelLength {
                expected: total,
                actual: pixels.len() as u64,
            });
        }
        // Each size is at most `total`, which equals a slice length, so it fits `usize`.
        let mip_sizes = sizes.into_iter().map(|s| s as usize).collect();
        Ok(Texture {
            header,
            pixels,
            mip_sizes,
        })
    }

    /// The parsed header.
    pub fn header(&self) -> &Header {
        &self.header
    }

    fn index(&self, level: u32) -> Result<usize> {
        let count = u32::from(self.header.mip_count);
        if level >= count {
            return Err(Error::MipOutOfRange {
                index: level,
                count,
            });
        }
        Ok(level as usize)
    }

    /// Width and height of a mip. Mip 0 is the largest.
    ///
    /// # Errors
    ///
    /// [`Error::MipOutOfRange`] if the mip does not exist.
    pub fn mip_dimensions(&self, level: u32) -> Result<(u32, u32)> {
        self.index(level)?;
        Ok((
            mip_dim(self.header.width, level),
            mip_dim(self.header.height, level),
        ))
    }

    /// Length in bytes of a mip's stored data.
    ///
    /// # Errors
    ///
    /// [`Error::MipOutOfRange`] if the mip does not exist.
    pub fn mip_len(&self, level: u32) -> Result<usize> {
        Ok(self.mip_sizes[self.index(level)?])
    }

    /// Offset in bytes of a mip within the pixel bytes. Smaller mips come first.
    ///
    /// # Errors
    ///
    /// [`Error::MipOutOfRange`] if the mip does not exist.
    pub fn mip_offset(&self, level: u32) -> Result<usize> {
        let i = self.index(level)?;
        Ok(self.mip_sizes[i + 1..].iter().sum())
    }

    /// A mip's stored bytes, in the header's pixel format.
    ///
    /// # Errors
    ///
    /// [`Error::MipOutOfRange`] if the mip does not exist.
    pub fn mip_data(&self, level: u32) -> Result<&'a [u8]> {
        let at = self.mip_offset(level)?;
        let len = self.mip_len(level)?;
        Ok(&self.pixels[at..at + len])
    }

    /// Decodes a mip to 8-bit RGBA.
    ///
    /// For the block formats the output is at most 8 times the mip's stored size, so a texture
    /// that passed [`Texture::parse`] cannot make this allocate more than its input justifies.
    /// An embedded PNG must match the header's dimensions, and the `png` decoder's own memory
    /// limit applies.
    ///
    /// # Errors
    ///
    /// [`Error::MipOutOfRange`] if the mip does not exist.
    pub fn decode_rgba8(&self, level: u32) -> Result<Image> {
        let (width, height) = self.mip_dimensions(level)?;
        let data = self.mip_data(level)?;
        let rgba = match self.header.format {
            Format::Rgba8888 => data.to_vec(),
            Format::Bgra8888 => {
                let mut out = data.to_vec();
                for px in out.as_chunks_mut::<4>().0.iter_mut() {
                    px.swap(0, 2);
                }
                out
            }
            Format::Bc1 => bc::decode_bc1(data, width, height),
            Format::Bc3 => bc::decode_bc3(data, width, height),
            Format::PngRgba8888 | Format::PngDxt5 => decode_embedded_png(data, width, height)?,
            format => return Err(Error::UnsupportedFormat { format }),
        };
        Ok(Image {
            width,
            height,
            rgba,
        })
    }
}
