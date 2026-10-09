//! BC1 and BC3 block decoders.

fn expand_565(c: u16) -> [u8; 3] {
    let r = u32::from(c >> 11 & 31);
    let g = u32::from(c >> 5 & 63);
    let b = u32::from(c & 31);
    [
        ((r * 255 + 15) / 31) as u8,
        ((g * 255 + 31) / 63) as u8,
        ((b * 255 + 15) / 31) as u8,
    ]
}

/// The four colours of a BC1 colour block. With `punch_through`, a block whose first colour
/// is not greater than its second has three colours and a transparent fourth entry.
fn palette(block: &[u8], punch_through: bool) -> [[u8; 4]; 4] {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let (a, b) = (expand_565(c0), expand_565(c1));
    let mut out = [[0, 0, 0, 255]; 4];
    out[0][..3].copy_from_slice(&a);
    out[1][..3].copy_from_slice(&b);
    for i in 0..3 {
        let (x, y) = (u32::from(a[i]), u32::from(b[i]));
        if c0 > c1 || !punch_through {
            out[2][i] = ((2 * x + y) / 3) as u8;
            out[3][i] = ((x + 2 * y) / 3) as u8;
        } else {
            out[2][i] = ((x + y) / 2) as u8;
        }
    }
    if c0 <= c1 && punch_through {
        out[3] = [0, 0, 0, 0];
    }
    out
}

/// The eight alpha values of a BC3 alpha block.
fn alpha_palette(a0: u8, a1: u8) -> [u8; 8] {
    let (x, y) = (u32::from(a0), u32::from(a1));
    let mut out = [a0, a1, 0, 0, 0, 0, 0, 255];
    if a0 > a1 {
        for i in 1..7 {
            out[i as usize + 1] = (((7 - i) * x + i * y) / 7) as u8;
        }
    } else {
        for i in 1..5 {
            out[i as usize + 1] = (((5 - i) * x + i * y) / 5) as u8;
        }
        out[6] = 0;
    }
    out
}

/// Decodes `width` x `height` pixels from 4x4 blocks of `block_len` bytes (8 for BC1, 16 for
/// BC3). `data` must hold every block.
fn decode(data: &[u8], width: u32, height: u32, bc3: bool) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let block_len = if bc3 { 16 } else { 8 };
    let blocks_per_row = w.div_ceil(4);
    let mut out = vec![0u8; w * h * 4];
    for (n, block) in data.chunks_exact(block_len).enumerate() {
        let (bx, by) = (n % blocks_per_row, n / blocks_per_row);
        let (colour_block, alpha_block) = if bc3 {
            (&block[8..], Some(&block[..8]))
        } else {
            (block, None)
        };
        let colours = palette(colour_block, !bc3);
        let indices = u32::from_le_bytes([
            colour_block[4],
            colour_block[5],
            colour_block[6],
            colour_block[7],
        ]);
        let mut alphas = [255u8; 16];
        if let Some(a) = alpha_block {
            let table = alpha_palette(a[0], a[1]);
            let mut bits = 0u64;
            for (i, byte) in a[2..8].iter().enumerate() {
                bits |= u64::from(*byte) << (8 * i);
            }
            for (p, alpha) in alphas.iter_mut().enumerate() {
                *alpha = table[(bits >> (3 * p) & 7) as usize];
            }
        }
        for p in 0..16 {
            let (x, y) = (bx * 4 + p % 4, by * 4 + p / 4);
            if x >= w || y >= h {
                continue;
            }
            let mut px = colours[(indices >> (2 * p) & 3) as usize];
            if bc3 {
                px[3] = alphas[p];
            }
            out[(y * w + x) * 4..][..4].copy_from_slice(&px);
        }
    }
    out
}

/// Decodes a BC1 mip.
pub(crate) fn decode_bc1(data: &[u8], width: u32, height: u32) -> Vec<u8> {
    decode(data, width, height, false)
}

/// Decodes a BC3 mip.
pub(crate) fn decode_bc3(data: &[u8], width: u32, height: u32) -> Vec<u8> {
    decode(data, width, height, true)
}
