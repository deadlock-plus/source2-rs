//! Tests for the LZ4 high-compression encoder.

use crate::lz4_hc::{Level, compress};
use std::time::{Duration, Instant};

const LEVELS: [Level; 2] = [Level::L9, Level::L12];

struct Seq {
    literals: usize,
    match_len: usize,
    offset: usize,
}

fn extension(block: &[u8], pos: &mut usize) -> usize {
    let mut total = 0;
    loop {
        let b = block[*pos];
        *pos += 1;
        total += usize::from(b);
        if b != 255 {
            return total;
        }
    }
}

fn sequences(block: &[u8]) -> Vec<Seq> {
    let mut seqs = Vec::new();
    let mut pos = 0;
    while pos < block.len() {
        let token = block[pos];
        pos += 1;
        let mut literals = usize::from(token >> 4);
        if literals == 15 {
            literals += extension(block, &mut pos);
        }
        pos += literals;
        if pos == block.len() {
            seqs.push(Seq {
                literals,
                match_len: 0,
                offset: 0,
            });
            break;
        }
        let offset = usize::from(u16::from_le_bytes([block[pos], block[pos + 1]]));
        pos += 2;
        let mut match_len = usize::from(token & 15) + 4;
        if token & 15 == 15 {
            match_len += extension(block, &mut pos);
        }
        seqs.push(Seq {
            literals,
            match_len,
            offset,
        });
    }
    seqs
}

fn decode(window: &[u8], input_len: usize, block: &[u8]) -> Vec<u8> {
    if window.is_empty() {
        lz4_flex::block::decompress(block, input_len).unwrap()
    } else {
        lz4_flex::block::decompress_with_dict(block, input_len, window).unwrap()
    }
}

fn check(window: &[u8], input: &[u8], level: Level) -> Vec<u8> {
    let block = compress(window, input, level);
    assert_eq!(decode(window, input.len(), &block), input, "{level:?}");
    block
}

fn lcg(seed: &mut u64) -> u64 {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *seed >> 33
}

fn noise(len: usize, mut seed: u64) -> Vec<u8> {
    (0..len).map(|_| lcg(&mut seed) as u8).collect()
}

/// Text built from a small vocabulary, so there are many near-equal matches to choose from.
fn word_soup(len: usize, mut seed: u64) -> Vec<u8> {
    const VOCAB: [&str; 12] = [
        "module_",
        "inferno_wave",
        "upgrade",
        "station_",
        "mod_category",
        "spirit",
        "weapon",
        "vitality",
        "_tier3",
        "damage",
        "\0\0\0\0",
        "0123456789",
    ];
    let mut out = Vec::new();
    while out.len() < len {
        out.extend_from_slice(VOCAB[lcg(&mut seed) as usize % VOCAB.len()].as_bytes());
        if lcg(&mut seed) % 5 == 1 {
            out.push(lcg(&mut seed) as u8);
        }
    }
    out.truncate(len);
    out
}

#[test]
fn every_short_length_round_trips() {
    for level in LEVELS {
        for len in 0..40 {
            let input: Vec<u8> = (0..len).map(|i| (i % 3) as u8).collect();
            check(&[], &input, level);
            check(b"windowwindow", &input, level);
        }
    }
}

#[test]
fn incompressible_data_round_trips_with_bounded_growth() {
    for level in LEVELS {
        let input = noise(50_000, 7);
        let block = check(&[], &input, level);
        assert!(block.len() <= input.len() + input.len() / 255 + 16);
    }
}

#[test]
fn structured_data_round_trips() {
    for level in LEVELS {
        check(&[], &word_soup(100_000, 1), level);
    }
}

#[test]
fn the_block_obeys_the_end_of_block_rules() {
    for level in LEVELS {
        for len in [13, 17, 31, 64, 1000, 20_000] {
            let input = word_soup(len, len as u64);
            let block = check(&[], &input, level);
            let seqs = sequences(&block);
            let (last, rest) = seqs.split_last().unwrap();
            assert_eq!(last.match_len, 0, "the block ends on a literal run");
            assert!(
                last.literals >= 5,
                "the last 5 bytes are literals: {level:?} {len}"
            );
            let covered: usize = rest.iter().map(|s| s.literals + s.match_len).sum();
            if let Some(final_match) = rest.last() {
                let start = covered - final_match.match_len;
                assert!(
                    input.len() - start >= 12,
                    "the last match starts 12 bytes before the end: {level:?} {len}"
                );
            }
        }
    }
}

#[test]
fn offsets_stay_within_sixteen_bits_and_the_window() {
    let window = word_soup(65_536, 3);
    let input = word_soup(30_000, 4);
    for level in LEVELS {
        let block = check(&window, &input, level);
        assert!(sequences(&block).iter().all(|s| s.offset <= 65_535));
    }
}

#[test]
fn an_input_equal_to_the_window_is_one_long_match() {
    let window = word_soup(10_000, 9);
    for level in LEVELS {
        let block = check(&window, &window, level);
        assert!(block.len() < 64, "{level:?}: {} bytes", block.len());
    }
}

#[test]
fn matches_reach_into_the_window_only_when_one_is_given() {
    let data = word_soup(8_000, 5);
    for level in LEVELS {
        let alone = check(&[], &data, level).len();
        let linked = check(&data, &data, level).len();
        assert!(linked < alone / 10, "{level:?}: {linked} vs {alone}");
    }
}

#[test]
fn it_beats_the_fast_encoder_on_structured_data() {
    let input = word_soup(200_000, 11);
    let fast = lz4_flex::block::compress(&input).len();
    let chain = check(&[], &input, Level::L9).len();
    let optimal = check(&[], &input, Level::L12).len();
    assert!(chain < fast, "chain {chain} vs fast {fast}");
    assert!(optimal <= chain, "optimal {optimal} vs chain {chain}");
}

#[test]
fn long_runs_do_not_blow_up_the_search() {
    let mut input = vec![0u8; 300_000];
    input.extend_from_slice(&word_soup(5_000, 2));
    input.extend(std::iter::repeat_n(0xAB, 100_000));
    for level in LEVELS {
        let start = Instant::now();
        let block = check(&[], &input, level);
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{level:?} is too slow"
        );
        assert!(block.len() < 5_000 + 2_000, "{level:?}: {}", block.len());
    }
}
