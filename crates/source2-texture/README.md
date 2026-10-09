# source2-texture

Reader for Source 2 textures (`vtex_c`) and decoder from five pixel formats to 8-bit RGBA.

- A `vtex_c` is a compiled resource. Its `DATA` block holds a 40-byte header. The pixels are
  not in that block: they are the bytes after the last resource block.
- This crate takes those two byte slices. It does not read the resource container. Use
  `source2-resource` for that (`Resource::data()` and `Resource::trailing`).
- It reads only. It does not write textures. It decodes embedded PNG but does not encode PNG.
- No dependencies, no feature flags, no `unsafe`.

## Install

```toml
[dependencies]
source2-texture = "0.1"
```

Minimum supported Rust version: 1.88.

## Decode

```rust
use source2_texture::{Format, Texture};

// A 2x1 BGRA8888 texture with two mips, smallest first.
let mut data = vec![0u8; 40];
data[20] = 2; // width
data[22] = 1; // height
data[24] = 1; // depth
data[26] = 28; // format: BGRA8888
data[27] = 2; // mip count
let pixels = [
    9, 8, 7, 255, // mip 1: 1x1
    10, 20, 30, 255, 40, 50, 60, 255, // mip 0: 2x1
];

let texture = Texture::parse(&data, &pixels)?;
assert_eq!(texture.header().format, Format::Bgra8888);
assert_eq!(texture.mip_dimensions(1)?, (1, 1));

let image = texture.decode_rgba8(0)?;
assert_eq!((image.width, image.height), (2, 1));
assert_eq!(image.rgba, [30, 20, 10, 255, 60, 50, 40, 255]);
# Ok::<(), source2_texture::Error>(())
```

From a file, with `source2-resource`:

```rust,ignore
use source2_resource::Resource;
use source2_texture::Texture;

let bytes = std::fs::read("icon_psd.vtex_c")?;
let resource = Resource::parse(&bytes)?;
let data = resource.data().expect("a vtex_c has a DATA block");
let texture = Texture::parse(data, &resource.trailing)?;
let image = texture.decode_rgba8(0)?;
```

`Header::parse` reads only the header and accepts any pixel format. Use it to learn the size
and format of a texture this crate cannot decode.

## Scaled `YCoCg`

Some BC3 textures do not hold RGB. They hold scaled `YCoCg`: after BC3 decoding, red is Co,
green is Cg, blue is a scale byte and alpha is Y. `Image::ycocg_to_rgb` converts an image in
place to opaque RGB (`s = (blue >> 3) + 1`, `Co = (red - 128) / s`, `Cg = (green - 128) / s`,
then `R = Y + Co - Cg`, `G = Y + Cg`, `B = Y - Co - Cg`, rounded and clamped).

```
use source2_texture::Texture;

// A 1x1 RGBA8888 texture standing in for decoded BC3: Co = 148, Cg = 128, scale = 8, Y = 100.
let mut data = vec![0u8; 40];
data[20] = 1;
data[22] = 1;
data[24] = 1;
data[26] = 4;
data[27] = 1;
let mut image = Texture::parse(&data, &[148, 128, 8, 100])?.decode_rgba8(0)?;
image.ycocg_to_rgb(); // s = 2, Co = 10, Cg = 0
assert_eq!(image.rgba, [110, 100, 90, 255]);
# Ok::<(), source2_texture::Error>(())
```

- The pixels cannot tell you. This crate never converts by itself: call `ycocg_to_rgb` only
  for a texture the resource marks.
- The mark is in the `RED2` block (KV3). Its `m_SpecialDependencies` list has an entry with
  `m_String` equal to `Texture Compiler Version Image YCoCg Conversion`. In Deadlock's
  `pak01`, 161 of 653 BC3 textures carry it, all under `panorama/images/`. Plain BC3 never does.
- `source2::ext::ResourceTexture` (facade crate) reads the mark and applies the conversion.
  This crate reads no KV3 and has no such dependency.

## Layout

- Mips are stored smallest first, largest last. Mip 0 is the largest.
- Mip `n` is `max(1, size >> n)` pixels on each axis.
- BC1 and BC3 round each axis up to whole 4x4 blocks: 8 bytes per block for BC1, 16 for BC3.
- `Texture::parse` requires the pixel slice to be exactly as long as all the mips add up to.
  A texture whose mips do not tile the slice is `Error::BadPixelLength`, not a guess.
- `mip_offset`, `mip_len` and `mip_data` give a mip's place in the slice.

## Decoded formats

| Format | Number | Output |
| --- | --- | --- |
| `Bgra8888` | 28 | Red and blue swapped. |
| `Rgba8888` | 4 | Copied. |
| `Bc1` | 1 | Four-colour and three-colour blocks. In a three-colour block the fourth entry is transparent. |
| `Bc3` | 2 | Colour block always four-colour. Alpha from the 8-value or 6-value alpha block. |
| `PngRgba8888`, `PngDxt5` | 16, 18 | The single mip is a PNG file. 8-bit, any colour type, expanded to RGBA. Its size must match the header. |

Colours expand from 5 and 6 bits with rounding. Interpolated colours use integer division.
Another decoder may differ by 1 per channel.

## Errors

Malformed or unsupported input returns `Error`, a `non_exhaustive` enum. Nothing panics on
bad input and nothing is guessed.

| Variant | Raised when |
| --- | --- |
| `Truncated` | The header or its extra-data table does not fit in the block. |
| `ZeroDimension` | Width, height, depth or mip count is zero. |
| `UnsupportedFormat` | The format is not one of the above (BC7, BC6H, ETC2 and every other number). |
| `UnsupportedLayout` | The flags say cube map, volume or array, or depth is above 1. |
| `BadEmbeddedImage` | A PNG-embedded mip is malformed or its size differs from the header. |
| `UnsupportedCompressedMips` | The extra-data table has an entry of type 3. |
| `BadPixelLength` | The pixel slice length differs from what the mips need. |
| `MipOutOfRange` | The mip index is not below the mip count. |

The mip sizes are checked against the pixel slice length before any buffer is allocated.
A decoded image is at most 8 times the stored mip, so a hostile header cannot make the
decoder allocate more than the input justifies.

## What is checked

- Unit tests build headers and blocks by hand from the format description and compare the
  decoded pixels with values worked out by hand. They also cover truncated and random input,
  zero sizes, mip indexes out of range, and the largest possible dimensions.
- `source2` (the facade crate) has a test that reads Deadlock textures from an install and
  decodes every mip of the hero art and rank badge samples. It also scans up to 600 textures
  and requires each one to decode or return an `Unsupported*` error. It passes without
  checking anything when no install is found.
- The `YCoCg` conversion is checked against hand-computed pixels. In the `source2` tests, a
  converted Deadlock icon differs from the published image by at most 3 per channel.
- A Deadlock hero icon stored as an embedded PNG (format 16) decodes in an ignored test
  that needs an install.
- Not checked: the output of this crate against another decoder, any format 18 file, and any
  texture of a format other than the decoded ones.

## Known limitations

- Extra-data type 3 means compressed mips here. This comes from the format as seen in
  Deadlock files, not from a specification. Other extra types are stored as read and ignored.
- The flag bits for cube maps (`0x400`), volumes (`0x800`) and arrays (`0x1000`) come from
  ValveResourceFormat. No sample file for them was decoded.
- Header version 1 is the only version seen. Other versions are read the same way.
- Whole-slice operation: the input is in memory and decoded mips are owned `Vec<u8>`s.

## License

Licensed under either of Apache License, Version 2.0 (`LICENSE-APACHE`) or the MIT license (`LICENSE-MIT`) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this work, as defined in the Apache-2.0 licence, is dual licensed as above, without any additional terms or conditions.
