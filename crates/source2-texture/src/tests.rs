//! Tests on textures built in-process.
//!
//! `header` and the pixel builders lay bytes out from the format description alone, so
//! parsing and decoding are checked against them and not against this crate's own code.

use super::*;

const FORMAT_BC1: u8 = 1;
const FORMAT_BC3: u8 = 2;
const FORMAT_RGBA8888: u8 = 4;
const FORMAT_PNG_RGBA8888: u8 = 16;
const FORMAT_PNG_DXT5: u8 = 18;
const FORMAT_BGRA8888: u8 = 28;

fn header(width: u16, height: u16, depth: u16, format: u8, mips: u8) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    for r in [0.25f32, 0.5, 0.75, 1.0] {
        out.extend_from_slice(&r.to_le_bytes());
    }
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&depth.to_le_bytes());
    out.push(format);
    out.push(mips);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(out.len(), 40);
    out
}

fn with_flags(mut block: Vec<u8>, flags: u16) -> Vec<u8> {
    block[2..4].copy_from_slice(&flags.to_le_bytes());
    block
}

fn with_extra(mut block: Vec<u8>, entries: &[(u32, u32, u32)]) -> Vec<u8> {
    block[36..40].copy_from_slice(&(entries.len() as u32).to_le_bytes());
    for (kind, offset, size) in entries {
        block.extend_from_slice(&kind.to_le_bytes());
        block.extend_from_slice(&offset.to_le_bytes());
        block.extend_from_slice(&size.to_le_bytes());
    }
    block
}

fn zeros(len: usize) -> Vec<u8> {
    vec![0; len]
}

#[test]
fn header_fields_are_read() {
    let block = with_flags(header(64, 32, 1, FORMAT_BGRA8888, 3), 0x0004);
    let h = Header::parse(&block).unwrap();
    assert_eq!(h.version, 1);
    assert_eq!(h.flags, 0x0004);
    assert_eq!(h.reflectivity, [0.25, 0.5, 0.75, 1.0]);
    assert_eq!((h.width, h.height, h.depth), (64, 32, 1));
    assert_eq!(h.format, Format::Bgra8888);
    assert_eq!(h.mip_count, 3);
}

#[test]
fn header_reads_picmip_and_extra_entries() {
    let mut block = with_extra(header(4, 4, 1, FORMAT_BC1, 1), &[(1, 24, 1024), (2, 7, 9)]);
    block[28..32].copy_from_slice(&512u32.to_le_bytes());
    let h = Header::parse(&block).unwrap();
    assert_eq!(h.picmip0_res, 512);
    assert_eq!(
        h.extra,
        vec![
            Extra {
                kind: 1,
                offset: 24,
                size: 1024
            },
            Extra {
                kind: 2,
                offset: 7,
                size: 9
            },
        ]
    );
}

#[test]
fn format_numbers_round_trip() {
    for (n, f) in [
        (1, Format::Bc1),
        (2, Format::Bc3),
        (4, Format::Rgba8888),
        (16, Format::PngRgba8888),
        (18, Format::PngDxt5),
        (19, Format::Bc6h),
        (20, Format::Bc7),
        (28, Format::Bgra8888),
        (99, Format::Other(99)),
    ] {
        assert_eq!(Format::from_u8(n), f);
        assert_eq!(f.to_u8(), n);
    }
    assert_eq!(Format::Bc7.to_string(), "BC7");
    assert_eq!(Format::Other(99).to_string(), "unknown (99)");
}

#[test]
fn short_header_is_truncated() {
    let block = header(4, 4, 1, FORMAT_BC1, 1);
    for len in 0..40 {
        assert!(matches!(
            Header::parse(&block[..len]),
            Err(Error::Truncated {
                what: "header",
                needed: 40,
                ..
            })
        ));
    }
}

#[test]
fn extra_table_past_the_block_is_truncated() {
    let mut block = with_extra(header(4, 4, 1, FORMAT_BC1, 1), &[(1, 0, 0)]);
    block.truncate(45);
    assert!(matches!(
        Header::parse(&block),
        Err(Error::Truncated {
            what: "extra data table",
            ..
        })
    ));
}

#[test]
fn huge_extra_count_does_not_allocate() {
    let mut block = header(4, 4, 1, FORMAT_BC1, 1);
    block[36..40].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        Header::parse(&block),
        Err(Error::Truncated {
            what: "extra data table",
            ..
        })
    ));
}

#[test]
fn extra_offset_overflow_is_truncated() {
    let mut block = with_extra(header(4, 4, 1, FORMAT_BC1, 1), &[(1, 0, 0)]);
    block[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        Header::parse(&block),
        Err(Error::Truncated { .. })
    ));
}

#[test]
fn zero_fields_are_rejected() {
    for (w, h, d, m, what) in [
        (0, 4, 1, 1, "width"),
        (4, 0, 1, 1, "height"),
        (4, 4, 0, 1, "depth"),
        (4, 4, 1, 0, "mip count"),
    ] {
        let block = header(w, h, d, FORMAT_BGRA8888, m);
        assert_eq!(Header::parse(&block), Err(Error::ZeroDimension { what }));
    }
}

#[test]
fn mip_dimensions_halve_and_stop_at_one() {
    // 20x3, 10x1, 5x1, 2x1, 1x1, 1x1 = 60 + 10 + 5 + 2 + 1 + 1 pixels.
    let block = header(20, 3, 1, FORMAT_BGRA8888, 6);
    assert_eq!(
        Texture::parse(&block, &[]).err(),
        Some(Error::BadPixelLength {
            expected: 79 * 4,
            actual: 0
        })
    );
    let pixels = zeros(79 * 4);
    let t = Texture::parse(&block, &pixels).unwrap();
    let dims: Vec<_> = (0..6).map(|i| t.mip_dimensions(i).unwrap()).collect();
    assert_eq!(dims, [(20, 3), (10, 1), (5, 1), (2, 1), (1, 1), (1, 1)]);
}

#[test]
fn mip_sizes_per_format() {
    let len = |fmt, w, h, len| {
        let pixels = zeros(len);
        Texture::parse(&header(w, h, 1, fmt, 1), &pixels)
            .unwrap()
            .mip_len(0)
    };
    assert_eq!(len(FORMAT_BC1, 5, 5, 32), Ok(32));
    assert_eq!(len(FORMAT_BC3, 1, 1, 16), Ok(16));
    assert_eq!(len(FORMAT_BGRA8888, 3, 2, 24), Ok(24));
    assert_eq!(len(FORMAT_RGBA8888, 3, 2, 24), Ok(24));
}

#[test]
fn mips_are_stored_smallest_first() {
    // 8x8 BGRA, 4 mips: 256, 64, 16, 4 bytes; mip 3 first.
    let block = header(8, 8, 1, FORMAT_BGRA8888, 4);
    let mut pixels = Vec::new();
    for (fill, len) in [(3u8, 4usize), (2, 16), (1, 64), (0, 256)] {
        pixels.extend(std::iter::repeat_n(fill, len));
    }
    let t = Texture::parse(&block, &pixels).unwrap();
    assert_eq!(t.mip_offset(0), Ok(84));
    assert_eq!(t.mip_offset(1), Ok(20));
    assert_eq!(t.mip_offset(2), Ok(4));
    assert_eq!(t.mip_offset(3), Ok(0));
    for level in 0..4 {
        let data = t.mip_data(level).unwrap();
        assert_eq!(data.len(), 256 >> (2 * level));
        assert!(data.iter().all(|&b| b == level as u8));
    }
}

#[test]
fn block_mips_round_up_to_blocks() {
    // 6x6 BC1: mip 0 is 2x2 blocks (32 bytes), mip 1 is 3x3 -> 1 block, mip 2 is 1x1 -> 1 block.
    let block = header(6, 6, 1, FORMAT_BC1, 3);
    let pixels = zeros(48);
    let t = Texture::parse(&block, &pixels).unwrap();
    assert_eq!(
        [t.mip_len(0), t.mip_len(1), t.mip_len(2)],
        [Ok(32), Ok(8), Ok(8)]
    );
    assert_eq!(t.mip_offset(0), Ok(16));
}

#[test]
fn wrong_pixel_length_is_rejected() {
    let block = header(2, 2, 1, FORMAT_BGRA8888, 1);
    for len in [0, 15, 17, 1000] {
        assert_eq!(
            Texture::parse(&block, &zeros(len)).err(),
            Some(Error::BadPixelLength {
                expected: 16,
                actual: len as u64
            })
        );
    }
}

#[test]
fn mip_out_of_range_is_an_error() {
    let block = header(2, 2, 1, FORMAT_BGRA8888, 2);
    let pixels = zeros(20);
    let t = Texture::parse(&block, &pixels).unwrap();
    for level in [2, 3, u32::MAX] {
        let want = Error::MipOutOfRange {
            index: level,
            count: 2,
        };
        assert_eq!(t.mip_dimensions(level), Err(want.clone()));
        assert_eq!(t.mip_len(level), Err(want.clone()));
        assert_eq!(t.mip_offset(level), Err(want.clone()));
        assert_eq!(t.mip_data(level), Err(want.clone()));
        assert_eq!(t.decode_rgba8(level).err(), Some(want));
    }
}

#[test]
fn unsupported_formats_are_distinct_errors() {
    for (n, f) in [
        (19, Format::Bc6h),
        (20, Format::Bc7),
        (23, Format::Other(23)),
        (0, Format::Other(0)),
    ] {
        let block = header(4, 4, 1, n, 1);
        assert!(Header::parse(&block).is_ok());
        assert_eq!(
            Texture::parse(&block, &zeros(64)).err(),
            Some(Error::UnsupportedFormat { format: f })
        );
    }
}

#[test]
fn cube_volume_and_array_layouts_are_unsupported() {
    for (flags, depth, kind) in [
        (0x0400, 1, "cube map"),
        (0x0800, 1, "volume"),
        (0x1000, 1, "array"),
        (0x0000, 4, "volume"),
    ] {
        let block = with_flags(header(4, 4, depth, FORMAT_BGRA8888, 1), flags);
        assert_eq!(
            Texture::parse(&block, &zeros(64)).err(),
            Some(Error::UnsupportedLayout { kind })
        );
    }
}

#[test]
fn compressed_mips_are_unsupported() {
    let block = with_extra(header(4, 4, 1, FORMAT_BGRA8888, 1), &[(3, 0, 4)]);
    assert_eq!(
        Texture::parse(&block, &zeros(64)).err(),
        Some(Error::UnsupportedCompressedMips)
    );
}

#[test]
fn fallback_blob_does_not_block_parsing() {
    let block = with_extra(header(4, 4, 1, FORMAT_BGRA8888, 1), &[(1, 0, 1024)]);
    assert!(Texture::parse(&block, &zeros(64)).is_ok());
}

#[test]
fn oversize_dimensions_are_rejected_before_any_allocation() {
    for format in [FORMAT_BC1, FORMAT_BC3, FORMAT_BGRA8888, FORMAT_RGBA8888] {
        let block = header(u16::MAX, u16::MAX, 1, format, 255);
        let err = Texture::parse(&block, &zeros(64)).err();
        assert!(matches!(err, Some(Error::BadPixelLength { .. })), "{err:?}");
    }
}

#[test]
fn garbage_never_panics() {
    let mut seed = 0x9e37_79b9_u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for round in 0..4000 {
        let len = (next() % 90) as usize;
        let mut data: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        if round % 2 == 0 && data.len() >= 40 {
            data[24..26].copy_from_slice(&1u16.to_le_bytes());
            data[26] = [1, 2, 4, 28, 20][(next() % 5) as usize];
            data[20..24].copy_from_slice(&[(next() % 9 + 1) as u8, 0, (next() % 9 + 1) as u8, 0]);
            data[27] = (next() % 5 + 1) as u8;
        }
        let pixels: Vec<u8> = (0..next() % 300).map(|_| next() as u8).collect();
        if let Ok(t) = Texture::parse(&data, &pixels) {
            for level in 0..10 {
                let _ = t.decode_rgba8(level);
            }
        }
    }
}

#[test]
fn truncating_a_valid_texture_returns_errors() {
    let block = header(8, 8, 1, FORMAT_BC3, 4);
    let pixels = zeros(64 + 16 + 16 + 16);
    assert!(Texture::parse(&block, &pixels).is_ok());
    for len in 0..block.len() {
        assert!(Texture::parse(&block[..len], &pixels).is_err());
    }
    for len in 0..pixels.len() {
        assert!(Texture::parse(&block, &pixels[..len]).is_err());
    }
}

fn single(format: u8, w: u16, h: u16, pixels: &[u8]) -> Image {
    let t = Texture::parse(&header(w, h, 1, format, 1), pixels).unwrap();
    t.decode_rgba8(0).unwrap()
}

#[test]
fn bgra_decodes_by_swapping_red_and_blue() {
    let pixels = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let img = single(FORMAT_BGRA8888, 2, 2, &pixels);
    assert_eq!((img.width, img.height), (2, 2));
    assert_eq!(
        img.rgba,
        [3, 2, 1, 4, 7, 6, 5, 8, 11, 10, 9, 12, 15, 14, 13, 16]
    );
}

#[test]
fn rgba_decodes_unchanged() {
    let pixels: Vec<u8> = (0..24).collect();
    let img = single(FORMAT_RGBA8888, 3, 2, &pixels);
    assert_eq!((img.width, img.height), (3, 2));
    assert_eq!(img.rgba, pixels);
}

#[test]
fn any_mip_decodes_with_its_own_size() {
    // 4x2 BGRA, 3 mips: 4x2 (32 bytes), 2x1 (8), 1x1 (4). Smallest first.
    let block = header(4, 2, 1, FORMAT_BGRA8888, 3);
    let mut pixels = vec![30, 20, 10, 255];
    pixels.extend([1, 2, 3, 4, 5, 6, 7, 8]);
    pixels.extend((0..32).map(|i| 100 + i as u8));
    let t = Texture::parse(&block, &pixels).unwrap();

    let small = t.decode_rgba8(2).unwrap();
    assert_eq!((small.width, small.height), (1, 1));
    assert_eq!(small.rgba, [10, 20, 30, 255]);

    let mid = t.decode_rgba8(1).unwrap();
    assert_eq!((mid.width, mid.height), (2, 1));
    assert_eq!(mid.rgba, [3, 2, 1, 4, 7, 6, 5, 8]);

    let big = t.decode_rgba8(0).unwrap();
    assert_eq!((big.width, big.height), (4, 2));
    assert_eq!(big.rgba.len(), 32);
    assert_eq!(&big.rgba[..4], &[102, 101, 100, 103]);
}

const RED: u16 = 0xF800;
const BLUE: u16 = 0x001F;

fn bc1_block(c0: u16, c1: u16, indices: [u8; 16]) -> Vec<u8> {
    let mut bits = 0u32;
    for (p, i) in indices.iter().enumerate() {
        bits |= u32::from(*i) << (2 * p);
    }
    let mut out = c0.to_le_bytes().to_vec();
    out.extend(c1.to_le_bytes());
    out.extend(bits.to_le_bytes());
    out
}

fn bc3_block(a0: u8, a1: u8, alpha: [u8; 16], c0: u16, c1: u16, colors: [u8; 16]) -> Vec<u8> {
    let mut bits = 0u64;
    for (p, i) in alpha.iter().enumerate() {
        bits |= u64::from(*i) << (3 * p);
    }
    let mut out = vec![a0, a1];
    out.extend(&bits.to_le_bytes()[..6]);
    out.extend(bc1_block(c0, c1, colors));
    out
}

fn column_indices() -> [u8; 16] {
    std::array::from_fn(|p| (p % 4) as u8)
}

fn px(img: &Image, x: usize, y: usize) -> [u8; 4] {
    let at = (y * img.width as usize + x) * 4;
    img.rgba[at..at + 4].try_into().unwrap()
}

#[test]
fn bc1_four_colour_block() {
    let img = single(FORMAT_BC1, 4, 4, &bc1_block(RED, BLUE, column_indices()));
    assert_eq!((img.width, img.height), (4, 4));
    for y in 0..4 {
        assert_eq!(px(&img, 0, y), [255, 0, 0, 255]);
        assert_eq!(px(&img, 1, y), [0, 0, 255, 255]);
        assert_eq!(px(&img, 2, y), [170, 0, 85, 255]);
        assert_eq!(px(&img, 3, y), [85, 0, 170, 255]);
    }
}

#[test]
fn bc1_three_colour_block_has_transparent_index_three() {
    let img = single(FORMAT_BC1, 4, 4, &bc1_block(BLUE, RED, column_indices()));
    for y in 0..4 {
        assert_eq!(px(&img, 0, y), [0, 0, 255, 255]);
        assert_eq!(px(&img, 1, y), [255, 0, 0, 255]);
        assert_eq!(px(&img, 2, y), [127, 0, 127, 255]);
        assert_eq!(px(&img, 3, y), [0, 0, 0, 0]);
    }
}

#[test]
fn bc1_expands_565_to_8_bits() {
    let white = single(FORMAT_BC1, 4, 4, &bc1_block(0xFFFF, 0, [0; 16]));
    assert_eq!(px(&white, 0, 0), [255, 255, 255, 255]);
    let mid = single(FORMAT_BC1, 4, 4, &bc1_block(0x8410, 0, [0; 16]));
    assert_eq!(px(&mid, 0, 0), [132, 130, 132, 255]);
}

#[test]
fn bc1_reads_indices_row_by_row_from_the_low_bits() {
    let mut idx = [0u8; 16];
    idx[1] = 1;
    idx[4] = 1;
    idx[15] = 1;
    let img = single(FORMAT_BC1, 4, 4, &bc1_block(RED, BLUE, idx));
    for y in 0..4 {
        for x in 0..4 {
            let blue = (x, y) == (1, 0) || (x, y) == (0, 1) || (x, y) == (3, 3);
            let want = if blue {
                [0, 0, 255, 255]
            } else {
                [255, 0, 0, 255]
            };
            assert_eq!(px(&img, x, y), want, "({x}, {y})");
        }
    }
}

#[test]
fn bc1_crops_partial_blocks() {
    let mut pixels = bc1_block(RED, BLUE, [0; 16]);
    pixels.extend(bc1_block(RED, BLUE, [1; 16]));
    let img = single(FORMAT_BC1, 5, 3, &pixels);
    assert_eq!((img.width, img.height), (5, 3));
    assert_eq!(img.rgba.len(), 5 * 3 * 4);
    for y in 0..3 {
        for x in 0..4 {
            assert_eq!(px(&img, x, y), [255, 0, 0, 255]);
        }
        assert_eq!(px(&img, 4, y), [0, 0, 255, 255]);
    }
}

#[test]
fn bc1_one_pixel_texture() {
    let img = single(FORMAT_BC1, 1, 1, &bc1_block(RED, BLUE, [0; 16]));
    assert_eq!(img.rgba, [255, 0, 0, 255]);
}

#[test]
fn bc1_mips_decode_from_the_right_offsets() {
    // 8x4 BC1, 2 mips: mip 1 (4x2, one block) first, then mip 0 (two blocks).
    let block = header(8, 4, 1, FORMAT_BC1, 2);
    let mut pixels = bc1_block(BLUE, BLUE, [0; 16]);
    pixels.extend(bc1_block(RED, RED, [0; 16]));
    pixels.extend(bc1_block(0xFFFF, 0xFFFF, [0; 16]));
    let t = Texture::parse(&block, &pixels).unwrap();
    let small = t.decode_rgba8(1).unwrap();
    assert_eq!((small.width, small.height), (4, 2));
    assert_eq!(px(&small, 3, 1), [0, 0, 255, 255]);
    let big = t.decode_rgba8(0).unwrap();
    assert_eq!(px(&big, 0, 0), [255, 0, 0, 255]);
    assert_eq!(px(&big, 7, 3), [255, 255, 255, 255]);
}

#[test]
fn bc3_eight_value_alpha() {
    let alpha = std::array::from_fn(|p| (p % 8) as u8);
    let img = single(
        FORMAT_BC3,
        4,
        4,
        &bc3_block(255, 0, alpha, RED, BLUE, [0; 16]),
    );
    let want = [255, 0, 218, 182, 145, 109, 72, 36];
    for p in 0..16 {
        let px = px(&img, p % 4, p / 4);
        assert_eq!(px, [255, 0, 0, want[p % 8]], "pixel {p}");
    }
}

#[test]
fn bc3_six_value_alpha_ends_in_zero_and_opaque() {
    let alpha = std::array::from_fn(|p| (p % 8) as u8);
    let img = single(
        FORMAT_BC3,
        4,
        4,
        &bc3_block(10, 100, alpha, RED, BLUE, [0; 16]),
    );
    let want = [10, 100, 28, 46, 64, 82, 0, 255];
    for p in 0..16 {
        assert_eq!(px(&img, p % 4, p / 4)[3], want[p % 8], "pixel {p}");
    }
}

#[test]
fn bc3_colour_is_always_four_colour() {
    let img = single(
        FORMAT_BC3,
        4,
        4,
        &bc3_block(200, 200, [0; 16], BLUE, RED, column_indices()),
    );
    assert_eq!(px(&img, 2, 0), [85, 0, 170, 200]);
    assert_eq!(px(&img, 3, 0), [170, 0, 85, 200]);
}

#[test]
fn bc3_crops_partial_blocks_and_uses_16_byte_blocks() {
    let mut pixels = bc3_block(77, 77, [0; 16], RED, RED, [0; 16]);
    pixels.extend(bc3_block(33, 33, [0; 16], BLUE, BLUE, [0; 16]));
    let img = single(FORMAT_BC3, 6, 2, &pixels);
    assert_eq!((img.width, img.height), (6, 2));
    assert_eq!(px(&img, 3, 1), [255, 0, 0, 77]);
    assert_eq!(px(&img, 4, 0), [0, 0, 255, 33]);
    assert_eq!(px(&img, 5, 1), [0, 0, 255, 33]);
}

#[test]
fn decoding_random_pixels_never_panics() {
    let mut seed = 0x1234_5678_u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for format in [FORMAT_BC1, FORMAT_BC3, FORMAT_BGRA8888, FORMAT_RGBA8888] {
        for (w, h, mips) in [(1, 1, 1), (3, 5, 3), (4, 4, 3), (17, 9, 5), (64, 2, 7)] {
            let block = header(w, h, 1, format, mips);
            let len = Texture::parse(&block, &[])
                .err()
                .map(|e| match e {
                    Error::BadPixelLength { expected, .. } => expected as usize,
                    other => panic!("{other:?}"),
                })
                .unwrap();
            let pixels: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            let t = Texture::parse(&block, &pixels).unwrap();
            for level in 0..u32::from(mips) {
                let img = t.decode_rgba8(level).unwrap();
                assert_eq!(img.rgba.len(), (img.width * img.height * 4) as usize);
            }
        }
    }
}

fn ycocg_image(pixels: &[[u8; 4]]) -> Image {
    Image {
        width: pixels.len() as u32,
        height: 1,
        rgba: pixels.concat(),
    }
}

#[test]
fn ycocg_with_no_chroma_is_grey() {
    // Co = Cg = 0 (stored 128), scale 1: every channel is Y.
    let mut img = ycocg_image(&[[128, 128, 0, 100], [128, 128, 7, 0], [128, 128, 0, 255]]);
    img.ycocg_to_rgb();
    assert_eq!(
        img.rgba,
        [100, 100, 100, 255, 0, 0, 0, 255, 255, 255, 255, 255]
    );
}

#[test]
fn ycocg_applies_chroma_with_the_scale_from_blue() {
    // Scale byte 8 -> s = (8 >> 3) + 1 = 2. Co = (148 - 128) / 2 = 10, Cg = 0, Y = 100:
    // R = 110, G = 100, B = 90.
    let mut img = ycocg_image(&[[148, 128, 8, 100]]);
    img.ycocg_to_rgb();
    assert_eq!(img.rgba, [110, 100, 90, 255]);

    // Scale 1, Co = 10, Cg = -10, Y = 100: R = 120, G = 90, B = 100.
    let mut img = ycocg_image(&[[138, 118, 0, 100]]);
    img.ycocg_to_rgb();
    assert_eq!(img.rgba, [120, 90, 100, 255]);
}

#[test]
fn ycocg_scale_uses_the_top_five_bits_of_blue() {
    // 7 >> 3 = 0 (s = 1) and 8 >> 3 = 1 (s = 2); 255 >> 3 = 31 (s = 32).
    let mut img = ycocg_image(&[[138, 128, 7, 100], [138, 128, 255, 100]]);
    img.ycocg_to_rgb();
    // s = 1: Co = 10 -> R = 110, B = 90. s = 32: Co = 10/32 = 0.31 -> all round to 100.
    assert_eq!(img.rgba, [110, 100, 90, 255, 100, 100, 100, 255]);
}

#[test]
fn ycocg_rounds_to_nearest_and_clamps() {
    // s = 3 (blue 16), Co = 1/3, Cg = 0, Y = 100: R = 100.33, G = 100, B = 99.67.
    let mut img = ycocg_image(&[[129, 128, 16, 100]]);
    img.ycocg_to_rgb();
    assert_eq!(img.rgba, [100, 100, 100, 255]);

    // s = 1, Co = 100, Y = 250: R = 350 -> 255, B = 150. Y = 10: B = -90 -> 0.
    let mut img = ycocg_image(&[[228, 128, 0, 250], [228, 128, 0, 10]]);
    img.ycocg_to_rgb();
    assert_eq!(img.rgba, [255, 250, 150, 255, 110, 10, 0, 255]);
}

fn png_file(width: u32, height: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(color);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(data).unwrap();
    writer.finish().unwrap();
    out
}

#[test]
fn embedded_png_rgba_decodes_to_its_pixels() {
    let pixels: Vec<u8> = (0..24).collect();
    let file = png_file(3, 2, png::ColorType::Rgba, &pixels);
    for format in [FORMAT_PNG_RGBA8888, FORMAT_PNG_DXT5] {
        let img = single(format, 3, 2, &file);
        assert_eq!((img.width, img.height), (3, 2));
        assert_eq!(img.rgba, pixels);
    }
}

#[test]
fn embedded_png_rgb_gets_opaque_alpha() {
    let file = png_file(2, 1, png::ColorType::Rgb, &[1, 2, 3, 4, 5, 6]);
    let img = single(FORMAT_PNG_RGBA8888, 2, 1, &file);
    assert_eq!(img.rgba, [1, 2, 3, 255, 4, 5, 6, 255]);
}

#[test]
fn embedded_png_with_other_dimensions_is_rejected() {
    let file = png_file(2, 2, png::ColorType::Rgba, &[0; 16]);
    let t = Texture::parse(&header(4, 4, 1, FORMAT_PNG_RGBA8888, 1), &file).unwrap();
    assert!(matches!(
        t.decode_rgba8(0),
        Err(Error::BadEmbeddedImage { .. })
    ));
}

#[test]
fn embedded_png_garbage_is_an_error_not_a_panic() {
    let t = Texture::parse(&header(2, 2, 1, FORMAT_PNG_RGBA8888, 1), &[1, 2, 3]).unwrap();
    assert!(matches!(
        t.decode_rgba8(0),
        Err(Error::BadEmbeddedImage { .. })
    ));
}

#[test]
fn embedded_png_with_several_mips_is_unsupported() {
    assert_eq!(
        Texture::parse(&header(4, 4, 1, FORMAT_PNG_RGBA8888, 2), &[0; 8]).err(),
        Some(Error::UnsupportedFormat {
            format: Format::PngRgba8888
        })
    );
}

/// `DEADLOCK_CITADEL_DIR=".../Deadlock/game/citadel" cargo test -p source2-texture -- --ignored`
#[test]
#[ignore = "needs an installed game; set DEADLOCK_CITADEL_DIR"]
fn real_embedded_png_icon_decodes() {
    let dir = std::env::var("DEADLOCK_CITADEL_DIR").expect("set DEADLOCK_CITADEL_DIR");
    let vpk = source2_vpk::Vpk::open(std::path::Path::new(&dir).join("pak01_dir.vpk")).unwrap();
    let entry = vpk
        .find("panorama/images/heroes/hornet_sm_png.vtex_c")
        .unwrap();
    let bytes = vpk.read(entry).unwrap();
    let resource = source2_resource::Resource::parse(&bytes).unwrap();
    let data = resource.block(source2_resource::BlockKind::DATA).unwrap();
    let t = Texture::parse(&data.data, &resource.trailing).unwrap();
    let img = t.decode_rgba8(0).unwrap();
    assert_eq!((img.width, img.height), (128, 128));
    assert!(img.rgba.as_chunks::<4>().0.iter().any(|p| p[3] != 0));
}
