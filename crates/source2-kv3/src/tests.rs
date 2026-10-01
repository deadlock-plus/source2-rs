//! Tests against KV3 blocks built in-process.

use super::*;

/// An uncompressed KV3 v5 block wrapping `payload`.
fn build_kv3(payload: &[u8], compression: u32) -> Vec<u8> {
    let mut out = vec![0u8; 120];
    out[0..4].copy_from_slice(&crate::MAGIC_V5.to_le_bytes());
    out[20..24].copy_from_slice(&compression.to_le_bytes());
    out[28..32].copy_from_slice(&7u32.to_le_bytes()); // binary bytes
    out[32..36].copy_from_slice(&3u32.to_le_bytes()); // integers
    out[36..40].copy_from_slice(&1u32.to_le_bytes()); // eight-byte values
    // Totals across all frames...
    out[48..52].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    out[52..56].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    // ...and the first frame, which for a single-frame fixture is the same.
    out[72..76].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    out[76..80].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

#[test]
fn kv3_header_reads_the_fields_the_decoder_needs() {
    let block = build_kv3(b"uncompressed body", 0);
    let h = crate::Header::parse(&block).expect("header");
    assert_eq!(h.version, 5);
    assert_eq!(h.compression, crate::Compression::None);
    assert_eq!(h.binary_byte_count, 7);
    assert_eq!(h.integer_count, 3);
    assert_eq!(h.eight_byte_count, 1);
    assert_eq!(h.payload_offset, 120);
}

#[test]
fn kv3_decodes_an_uncompressed_payload() {
    let block = build_kv3(b"uncompressed body", 0);
    let d = crate::decode(&block).expect("decode");
    assert_eq!(d.payload, b"uncompressed body");
}

#[test]
fn kv3_rejects_a_non_kv3_block() {
    let mut block = build_kv3(b"x", 0);
    block[0..4].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
    assert!(matches!(crate::decode(&block), Err(Error::Malformed(_))));
}

/// An LZ4 block holding `data` as one literal run.
///
/// Hand-built rather than produced by the decoder's own library, so the test pins the
/// wire format instead of a round trip. A literal-only sequence is a complete, valid LZ4
/// block: the format requires the *last* sequence to be literals with no match, and a
/// block may consist of nothing else.
///
/// The token's high nibble is the literal count, or 15 with the remainder trailing as
/// 255-valued continuation bytes. The low nibble is the match length, zero here.
pub(crate) fn lz4_literal_block(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let n = data.len();
    if n < 15 {
        out.push((n as u8) << 4);
    } else {
        out.push(0xF0);
        let mut rest = n - 15;
        while rest >= 255 {
            out.push(255);
            rest -= 255;
        }
        out.push(rest as u8);
    }
    out.extend_from_slice(data);
    out
}

/// A KV3 v5 block whose payload is compressed, so the two sizes genuinely differ.
///
/// [`build_kv3`] stores its payload as-is and so reports one length for both, which
/// cannot catch a decoder that confuses them.
fn build_kv3_compressed(compressed: &[u8], compression: u32, uncompressed_len: u32) -> Vec<u8> {
    let mut out = build_kv3(compressed, compression);
    out[48..52].copy_from_slice(&uncompressed_len.to_le_bytes());
    out[72..76].copy_from_slice(&uncompressed_len.to_le_bytes());
    out
}

/// LZ4 is not a legacy method Deadlock has left behind: eight of the ten
/// `scripts/*.vdata_c` files it ships use it, and only `heroes` and `abilities` are zstd.
#[cfg(feature = "lz4")]
#[test]
fn kv3_decodes_an_lz4_payload() {
    let body = b"a literal run long enough to need an extended length token";
    let block = build_kv3_compressed(&lz4_literal_block(body), 1, body.len() as u32);
    let d = crate::decode(&block).expect("decode");
    assert_eq!(d.payload, body);
}

/// The length check is what separates a real decode from a plausible-looking partial one,
/// so it has to apply to LZ4 exactly as it does to zstd.
#[test]
fn kv3_rejects_an_lz4_payload_that_decodes_to_the_wrong_length() {
    let body = b"eight!!!";
    let block = build_kv3_compressed(&lz4_literal_block(body), 1, body.len() as u32 + 1);
    assert!(matches!(crate::decode(&block), Err(Error::Malformed(_))));
}

/// A block claiming more literals than it carries must fail rather than read past its end.
#[test]
fn kv3_rejects_a_truncated_lz4_payload() {
    let body = b"a literal run that will be cut short";
    let mut lz4 = lz4_literal_block(body);
    lz4.truncate(lz4.len() - 10);
    let block = build_kv3_compressed(&lz4, 1, body.len() as u32);
    assert!(matches!(crate::decode(&block), Err(Error::Malformed(_))));
}

/// Knowing LZ4 must not turn the unknown-method refusal into a guess.
#[test]
fn kv3_still_refuses_a_compression_method_it_does_not_know() {
    let block = build_kv3(b"whatever", 3);
    let err = crate::decode(&block).unwrap_err();
    assert!(format!("{err}").contains('3'), "{err}");
}

#[test]
fn kv3_rejects_a_compressed_size_larger_than_the_block() {
    let mut block = build_kv3(b"body", 0);
    block[52..56].copy_from_slice(&99_999u32.to_le_bytes());
    assert!(matches!(
        crate::Header::parse(&block),
        Err(Error::Malformed(_))
    ));
}

/// Claiming zstd without a zstd frame must fail loudly, not decode nonsense.
#[cfg(feature = "zstd")]
#[test]
fn kv3_rejects_a_zstd_claim_with_no_zstd_frame() {
    let block = build_kv3(b"not actually a zstd frame", 2);
    let err = crate::decode(&block).unwrap_err();
    assert!(format!("{err}").contains("zstd"), "{err}");
}

/// A hand-built KV3 v5 document, so the value model is covered without a game install.
///
/// Encodes `{ "a": 1i32, "b": "xy" }`, and with `with_double` a third member `"c": 2.5`
/// that gives buffer 2 a non-empty 8-byte pool.
///
/// The two shapes exercise opposite sides of the alignment rule: a pool is preceded by
/// padding to its own width only when it has entries. Deadlock ships both -
/// `scripts/ranked_seasons.vdata_c` has an empty 8-byte pool and no padding before its
/// type stream, while every larger file has entries and is padded.
#[cfg(feature = "lz4")]
fn build_kv3_v5_doc() -> Vec<u8> {
    build_kv3_v5_doc_with(false)
}

#[cfg(feature = "lz4")]
fn build_kv3_v5_doc_with(with_double: bool) -> Vec<u8> {
    use crate::value::node;

    // Buffer 1: string blob, then the 4-byte pool whose first slot is the string count.
    let strings: &[&str] = if with_double {
        &["a", "b", "xy", "c"]
    } else {
        &["a", "b", "xy"]
    };
    let mut blob = Vec::new();
    for s in strings {
        blob.extend_from_slice(s.as_bytes());
        blob.push(0);
    }
    let count_bytes1 = blob.len();
    let mut buf1 = blob.clone();
    while buf1.len() % 4 != 0 {
        buf1.push(0);
    }
    buf1.extend_from_slice(&(strings.len() as u32).to_le_bytes()); // string count
    let string_count = strings.len();
    let count_bytes4_b1 = 1u32; // just the count slot
    while buf1.len() % 8 != 0 {
        buf1.push(0);
    }

    // Buffer 2: object lengths, 4-byte operands, the type stream, the trailer.
    let mut buf2 = Vec::new();
    let members: u32 = if with_double { 3 } else { 2 };
    buf2.extend_from_slice(&members.to_le_bytes());
    let object_count = 1u32;

    let mut b4 = Vec::new();
    b4.extend_from_slice(&0u32.to_le_bytes()); // member name -> strings[0] = "a"
    b4.extend_from_slice(&1u32.to_le_bytes()); // its i32 value
    b4.extend_from_slice(&1u32.to_le_bytes()); // member name -> strings[1] = "b"
    b4.extend_from_slice(&2u32.to_le_bytes()); // string value -> strings[2] = "xy"
    if with_double {
        b4.extend_from_slice(&3u32.to_le_bytes()); // member name -> strings[3] = "c"
    }
    let count_bytes4_b2 = (b4.len() / 4) as u32;
    buf2.extend_from_slice(&b4);

    // Padding to the 8-byte pool's width, and the pool itself, only when it has entries:
    // an empty pool is not preceded by padding.
    let eight_count: u32 = u32::from(with_double);
    if with_double {
        while buf2.len() % 8 != 0 {
            buf2.push(0);
        }
        buf2.extend_from_slice(&2.5f64.to_le_bytes());
    }

    let types: Vec<u8> = if with_double {
        vec![node::OBJECT, node::INT32, node::STRING, node::DOUBLE]
    } else {
        vec![node::OBJECT, node::INT32, node::STRING]
    };
    buf2.extend_from_slice(&types);
    buf2.extend_from_slice(&0xFFEE_DD00u32.to_le_bytes());

    let mut header = vec![0u8; 120];
    header[0..4].copy_from_slice(&crate::MAGIC_V5.to_le_bytes());
    header[20..24].copy_from_slice(&0u32.to_le_bytes()); // uncompressed
    header[28..32].copy_from_slice(&(count_bytes1 as u32).to_le_bytes());
    header[32..36].copy_from_slice(&count_bytes4_b1.to_le_bytes());
    header[36..40].copy_from_slice(&0u32.to_le_bytes()); // buffer 1 has no 8-byte values
    header[40..44].copy_from_slice(&(types.len() as u32).to_le_bytes());
    let total = buf1.len() + buf2.len();
    header[48..52].copy_from_slice(&(total as u32).to_le_bytes());
    header[52..56].copy_from_slice(&(total as u32).to_le_bytes());
    header[72..76].copy_from_slice(&(buf1.len() as u32).to_le_bytes());
    header[80..84].copy_from_slice(&(buf2.len() as u32).to_le_bytes());
    header[96..100].copy_from_slice(&count_bytes4_b2.to_le_bytes());
    header[100..104].copy_from_slice(&eight_count.to_le_bytes());
    header[108..112].copy_from_slice(&object_count.to_le_bytes());
    let _ = string_count;

    let mut out = header;
    out.extend_from_slice(&buf1);
    out.extend_from_slice(&buf2);
    out
}

#[cfg(feature = "lz4")]
#[test]
fn value_model_reads_a_hand_built_document() {
    use crate::Value;

    let doc = crate::parse(&build_kv3_v5_doc()).expect("parse");
    let root = doc.root.as_object().expect("object");
    assert_eq!(root.len(), 2);
    assert_eq!(root.get("a").and_then(Value::as_i64), Some(1));
    assert_eq!(root.get("b").and_then(Value::as_str), Some("xy"));
    assert_eq!(root.iter().map(|(k, _)| k).collect::<Vec<_>>(), ["a", "b"]);
}

#[cfg(feature = "lz4")]
#[test]
fn value_model_rejects_a_bad_trailer() {
    let mut doc = build_kv3_v5_doc();
    let n = doc.len();
    doc[n - 4..].copy_from_slice(&0u32.to_le_bytes());
    let err = crate::parse(&doc).unwrap_err();
    assert!(format!("{err}").contains("trailer"), "{err}");
}

#[cfg(feature = "lz4")]
#[test]
fn value_model_rejects_an_out_of_range_string_index() {
    let mut doc = build_kv3_v5_doc();
    let buf1_len = u32::from_le_bytes(doc[72..76].try_into().unwrap()) as usize;
    let at = 120 + buf1_len + 4;
    doc[at..at + 4].copy_from_slice(&99u32.to_le_bytes());
    let err = crate::parse(&doc).unwrap_err();
    assert!(format!("{err}").contains("string index"), "{err}");
}

/// A pool with no entries is not preceded by padding.
///
/// `scripts/ranked_seasons.vdata_c` is the file that proves it: its buffer 2 accounts for
/// exactly its own length with no padding before the type stream, and padding
/// unconditionally leaves four bytes too few for the trailer. Every other shipped file
/// either has 8-byte entries or is already aligned, so this is the one shape that can
/// tell the two rules apart.
#[cfg(feature = "lz4")]
#[test]
fn value_model_does_not_pad_before_an_empty_pool() {
    use crate::Value;

    let doc = build_kv3_v5_doc_with(false);
    let buf1_len = u32::from_le_bytes(doc[72..76].try_into().unwrap()) as usize;
    let buf2_len = u32::from_le_bytes(doc[80..84].try_into().unwrap()) as usize;
    assert_eq!(
        (buf2_len - 3 - 4) % 8,
        4,
        "fixture must land off alignment for this to prove anything"
    );
    let _ = buf1_len;

    let doc = crate::parse(&doc).expect("parse");
    let root = doc.root.as_object().expect("object");
    assert_eq!(root.get("a").and_then(Value::as_i64), Some(1));
}

/// A pool with entries still is padded, so the fix does not simply drop alignment.
#[cfg(feature = "lz4")]
#[test]
fn value_model_pads_before_a_pool_that_has_entries() {
    use crate::Value;

    let doc = crate::parse(&build_kv3_v5_doc_with(true)).expect("parse");
    let root = doc.root.as_object().expect("object");
    assert_eq!(root.len(), 3);
    assert_eq!(root.get("a").and_then(Value::as_i64), Some(1));
    assert_eq!(root.get("b").and_then(Value::as_str), Some("xy"));
    assert_eq!(root.get("c").and_then(Value::as_f64), Some(2.5));
}

/// Assemble a KV3 v4 block out of its pools, string table and type stream.
///
/// v4 is one buffer, laid out as: the 1-byte pool, the 4-byte pool, the 8-byte pool, then
/// a single region holding the strings and the type stream, then the trailer. The header
/// sizes that last region as a whole - offset 40 - and never states where the strings end
/// and the types begin, which is why the reader has to count the strings out.
///
/// The 2-byte pool is absent rather than empty: v4 states no count for one.
#[cfg(feature = "lz4")]
fn build_kv3_v4(
    bytes: &[u8],
    ints: &[u32],
    eights: &[f64],
    strings: &[&str],
    types: &[u8],
) -> Vec<u8> {
    let pad_to = |out: &mut Vec<u8>, width: usize| out.resize(out.len().next_multiple_of(width), 0);

    let mut payload = bytes.to_vec();
    pad_to(&mut payload, 4);
    for i in ints {
        payload.extend_from_slice(&i.to_le_bytes());
    }
    if !eights.is_empty() {
        pad_to(&mut payload, 8);
        for e in eights {
            payload.extend_from_slice(&e.to_le_bytes());
        }
    }

    let mut blob = Vec::new();
    for s in strings {
        blob.extend_from_slice(s.as_bytes());
        blob.push(0);
    }
    blob.extend_from_slice(types);
    payload.extend_from_slice(&blob);
    payload.extend_from_slice(&0xFFEE_DD00u32.to_le_bytes());

    let mut out = vec![0u8; 72];
    out[0..4].copy_from_slice(&crate::MAGIC_V4.to_le_bytes());
    out[28..32].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    out[32..36].copy_from_slice(&(ints.len() as u32).to_le_bytes());
    out[36..40].copy_from_slice(&(eights.len() as u32).to_le_bytes());
    // Strings and types together, which is the field v5 spends on the type stream alone.
    out[40..44].copy_from_slice(&(blob.len() as u32).to_le_bytes());
    out[48..52].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    out[52..56].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    out
}

/// A hand-built KV3 v4 document, so the older revision is covered without a game install.
///
/// Encodes `{ "a": 1i32, "b": "xy" }` - deliberately the shape [`build_kv3_v5_doc`]
/// builds, so the two revisions can be asserted against identically.
#[cfg(feature = "lz4")]
fn build_kv3_v4_doc() -> Vec<u8> {
    use crate::value::node;

    let strings: &[&str] = &["a", "b", "xy"];
    let types = [node::OBJECT, node::INT32, node::STRING];
    // Operands in the order the type stream asks for them, behind the string count.
    let ints = [
        strings.len() as u32, // the string count opens the pool
        2,                    // the root object's member count, which v5 would table
        0,                    // member name -> "a"
        1,                    // its i32 value
        1,                    // member name -> "b"
        2,                    // string value -> "xy"
    ];
    build_kv3_v4(&[], &ints, &[], strings, &types)
}

/// A v4 document that puts every pool to work: `{ "a": 2.5, "list": ["x", "y"] }`.
///
/// The array is `ARRAY_TYPE_BYTE_LENGTH`, so its length is drawn from the 1-byte pool -
/// which also pushes the 4-byte pool off zero and makes its alignment observable. The
/// double gives the 8-byte pool an entry. `scripts/ping_wheel_message_types.vdata_c` and
/// `scripts/propdata.vdata_c` are the shipped files with those shapes.
#[cfg(feature = "lz4")]
fn build_kv3_v4_doc_with_pools() -> Vec<u8> {
    use crate::value::node;

    let strings: &[&str] = &["a", "list", "x", "y"];
    let types = [
        node::OBJECT,
        node::DOUBLE,
        node::ARRAY_TYPE_BYTE_LENGTH,
        node::STRING,
    ];
    let ints = [
        strings.len() as u32, // string count
        2,                    // root member count
        0,                    // member name -> "a"
        1,                    // member name -> "list"
        2,                    // element -> "x"
        3,                    // element -> "y"
    ];
    build_kv3_v4(&[2], &ints, &[2.5], strings, &types)
}

/// The same block with its payload as one LZ4 literal run.
///
/// v4 compresses the whole payload as a single block sized by offset 48, because it has
/// no second buffer to cut at and a raw LZ4 block states no length of its own.
#[cfg(feature = "lz4")]
fn as_lz4(block: &[u8]) -> Vec<u8> {
    let compressed = lz4_literal_block(&block[72..]);
    let mut out = block[..72].to_vec();
    out[20..24].copy_from_slice(&1u32.to_le_bytes());
    out[52..56].copy_from_slice(&(compressed.len() as u32).to_le_bytes());
    out.extend_from_slice(&compressed);
    out
}

#[cfg(feature = "lz4")]
#[test]
fn value_model_reads_a_hand_built_v4_document() {
    use crate::Value;

    let doc = crate::parse(&build_kv3_v4_doc()).expect("parse");
    assert_eq!(doc.header.version, 4);
    assert_eq!(doc.header.payload_offset, 72);
    let root = doc.root.as_object().expect("object");
    assert_eq!(root.len(), 2);
    assert_eq!(root.get("a").and_then(Value::as_i64), Some(1));
    assert_eq!(root.get("b").and_then(Value::as_str), Some("xy"));
    assert_eq!(root.iter().map(|(k, _)| k).collect::<Vec<_>>(), ["a", "b"]);
}

/// A v4 object's member count is an operand in the 4-byte pool, not an entry in a table.
///
/// This is the difference that fails quietly rather than loudly: looking for a table v4
/// does not have leaves the count and the first member's name index one slot out of step,
/// so the document still parses and is simply wrong.
#[cfg(feature = "lz4")]
#[test]
fn a_v4_object_counts_its_members_from_the_same_pool_as_every_other_operand() {
    use crate::Value;

    let mut doc = build_kv3_v4_doc();
    doc[72 + 4..72 + 8].copy_from_slice(&1u32.to_le_bytes());
    let doc = crate::parse(&doc).expect("parse");
    let root = doc.root.as_object().expect("object");
    assert_eq!(root.len(), 1, "the pool slot is what sizes the object");
    assert_eq!(root.get("a").and_then(Value::as_i64), Some(1));
}

/// v4 draws array lengths from the 1-byte pool and doubles from the 8-byte one the same
/// way v5 does, but out of the single buffer it has.
#[cfg(feature = "lz4")]
#[test]
fn value_model_reads_every_v4_pool_in_one_document() {
    use crate::Value;

    let doc = crate::parse(&build_kv3_v4_doc_with_pools()).expect("parse");
    let root = doc.root.as_object().expect("object");
    assert_eq!(root.get("a").and_then(Value::as_f64), Some(2.5));
    let list = root.get("list").and_then(Value::as_array).expect("list");
    let list: Vec<&str> = list.iter().filter_map(Value::as_str).collect();
    assert_eq!(list, ["x", "y"]);
}

/// LZ4 over v4 is one block over the whole payload, so v5's buffer split must not be
/// applied to it - a v4 header states no per-buffer sizes to split on.
#[cfg(feature = "lz4")]
#[test]
fn a_v4_document_stored_with_lz4_decodes_as_a_single_block() {
    use crate::Value;

    let block = as_lz4(&build_kv3_v4_doc_with_pools());
    let header = crate::Header::parse(&block).expect("header");
    assert_eq!(header.compression, crate::Compression::Lz4);
    assert_eq!(
        header.buffer1_compressed_size, 0,
        "v4 has no per-buffer size"
    );
    let doc = crate::parse(&block).expect("parse");
    let root = doc.root.as_object().expect("object");
    assert_eq!(root.get("a").and_then(Value::as_f64), Some(2.5));
}

/// The string-and-type region is sized by the header, so a size larger than the payload
/// has to be refused rather than indexed with.
#[cfg(feature = "lz4")]
#[test]
fn value_model_rejects_a_v4_string_and_type_region_bigger_than_its_payload() {
    let mut doc = build_kv3_v4_doc();
    doc[40..44].copy_from_slice(&9_999u32.to_le_bytes());
    let err = crate::parse(&doc).unwrap_err();
    assert!(format!("{err}").contains("strings and types"), "{err}");
}

/// A string count larger than the region holding the strings must not size an allocation.
#[cfg(feature = "lz4")]
#[test]
fn value_model_rejects_a_v4_string_count_larger_than_its_region() {
    let mut doc = build_kv3_v4_doc();
    doc[72..76].copy_from_slice(&u32::MAX.to_le_bytes());
    let err = crate::parse(&doc).unwrap_err();
    assert!(format!("{err}").contains("strings in a"), "{err}");
}

/// Reading v4 must not turn the refusal of older revisions into a guess: v3 lays its
/// payload out differently again, and nothing Deadlock ships uses it.
#[cfg(feature = "lz4")]
#[test]
fn value_model_still_refuses_a_revision_older_than_v4() {
    let mut doc = build_kv3_v4_doc();
    doc[0..4].copy_from_slice(&crate::MAGIC_V3.to_le_bytes());
    let err = crate::parse(&doc).unwrap_err();
    assert!(format!("{err}").contains("this is v3"), "{err}");
}
