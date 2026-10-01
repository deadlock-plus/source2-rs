//! Standalone KV3 files whose binary blobs trail the two buffers.

use crate::{Compression, Object, Value, parse};

fn words(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// What sits after the two buffers: the blob area and the tables buffer 2 ends with.
struct Tail {
    sizes: Vec<u32>,
    /// One per LZ4 chunk. Empty for zstd.
    chunk_sizes: Vec<u16>,
    area: Vec<u8>,
}

/// `{ "k": <blob>, "k": <blob>, ... }` laid out the way real standalone files are:
/// buffers 1 and 2, then the blob area, then a closing trailer.
///
/// Buffer 2 ends with each blob's uncompressed length as a u32, the buffer trailer, and on
/// LZ4 files each chunk's compressed length as a u16.
fn blob_file(tail: &Tail, compression: Compression, frame_size: u16) -> Vec<u8> {
    let n = tail.sizes.len();
    let mut buf1 = b"k\0\0\0".to_vec();
    buf1.extend(words(&[1]));

    let mut types = vec![9u8];
    types.extend(std::iter::repeat_n(7u8, n));
    let mut buf2 = words(&[n as u32]);
    buf2.extend(words(&vec![0; n]));
    buf2.extend(&types);
    buf2.extend(words(&tail.sizes));
    buf2.extend(words(&[0xFFEE_DD00]));
    for s in &tail.chunk_sizes {
        buf2.extend(s.to_le_bytes());
    }

    let pack = |raw: &[u8]| match compression {
        Compression::Lz4 => crate::tests::lz4_literal_block(raw),
        Compression::Zstd => {
            ruzstd::encoding::compress_to_vec(raw, ruzstd::encoding::CompressionLevel::Fastest)
        }
        _ => unreachable!(),
    };
    let c1 = pack(&buf1);
    let c2 = pack(&buf2);

    let mut h = vec![0u8; 120];
    let put = |h: &mut Vec<u8>, at: usize, v: u32| h[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut h, 0, crate::MAGIC_V5);
    put(
        &mut h,
        20,
        if compression == Compression::Lz4 {
            1
        } else {
            2
        },
    );
    h[26..28].copy_from_slice(&frame_size.to_le_bytes());
    put(&mut h, 28, 2);
    put(&mut h, 32, 1);
    put(&mut h, 40, types.len() as u32);
    put(&mut h, 48, (buf1.len() + buf2.len()) as u32);
    let main = c1.len() + c2.len();
    let stated = if compression == Compression::Zstd {
        main + tail.area.len()
    } else {
        main
    };
    put(&mut h, 52, stated as u32);
    put(&mut h, 56, n as u32);
    put(&mut h, 60, tail.sizes.iter().sum());
    put(&mut h, 72, buf1.len() as u32);
    put(&mut h, 76, c1.len() as u32);
    put(&mut h, 80, buf2.len() as u32);
    put(&mut h, 84, c2.len() as u32);
    put(&mut h, 96, n as u32);
    put(&mut h, 108, 1);

    let mut out = h;
    out.extend(c1);
    out.extend(c2);
    out.extend(&tail.area);
    out.extend(words(&[0xFFEE_DD00]));
    out
}

/// One independently compressed literal block per blob.
fn lz4_tail(blobs: &[&[u8]]) -> Tail {
    let mut tail = Tail {
        sizes: Vec::new(),
        chunk_sizes: Vec::new(),
        area: Vec::new(),
    };
    for b in blobs {
        let block = crate::tests::lz4_literal_block(b);
        tail.sizes.push(b.len() as u32);
        tail.chunk_sizes.push(block.len() as u16);
        tail.area.extend(block);
    }
    tail
}

fn expected(blobs: &[&[u8]]) -> Value {
    let mut o = Object::default();
    for b in blobs {
        o.push("k".to_string(), Value::blob(b.to_vec()));
    }
    Value::from(o)
}

#[test]
fn lz4_blobs_after_the_buffers_are_read() {
    let blobs: [&[u8]; 2] = [b"abc", b"hello world"];
    let file = blob_file(&lz4_tail(&blobs), Compression::Lz4, 16384);
    assert_eq!(parse(&file).expect("parse").root, expected(&blobs));
}

#[test]
fn zstd_blobs_after_the_buffers_are_read() {
    let blobs: [&[u8]; 2] = [b"abc", b"hello world"];
    let all = blobs.concat();
    let tail = Tail {
        sizes: blobs.iter().map(|b| b.len() as u32).collect(),
        chunk_sizes: Vec::new(),
        area: ruzstd::encoding::compress_to_vec(
            all.as_slice(),
            ruzstd::encoding::CompressionLevel::Fastest,
        ),
    };
    let file = blob_file(&tail, Compression::Zstd, 0);
    assert_eq!(parse(&file).expect("parse").root, expected(&blobs));
}

/// A blob longer than the frame size is stored as several chunks, each with its own entry
/// in the compressed-length table, so that table can be longer than the blob count.
#[test]
fn an_lz4_blob_longer_than_the_frame_size_is_split_into_chunks() {
    let blob: &[u8] = b"abcdefghijkl";
    let first = crate::tests::lz4_literal_block(&blob[..8]);
    let second = crate::tests::lz4_literal_block(&blob[8..]);
    let tail = Tail {
        sizes: vec![blob.len() as u32],
        chunk_sizes: vec![first.len() as u16, second.len() as u16],
        area: [first, second].concat(),
    };
    let file = blob_file(&tail, Compression::Lz4, 8);
    assert_eq!(parse(&file).expect("parse").root, expected(&[blob]));
}

/// A chunk may copy from the chunk before it, so each one is decoded against the output
/// so far rather than on its own.
#[test]
fn an_lz4_chunk_may_copy_from_the_chunk_before() {
    let first = crate::tests::lz4_literal_block(b"abcdefgh");
    // No literals, a four-byte match 8 bytes back, then an empty final literal run.
    let second = vec![0x00, 0x08, 0x00, 0x00];
    let tail = Tail {
        sizes: vec![12],
        chunk_sizes: vec![first.len() as u16, second.len() as u16],
        area: [first, second].concat(),
    };
    let file = blob_file(&tail, Compression::Lz4, 8);
    assert_eq!(
        parse(&file).expect("parse").root,
        expected(&[b"abcdefghabcd"])
    );
}
