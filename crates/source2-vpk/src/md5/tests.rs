use super::md5;

fn hex(d: [u8; 16]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// RFC 1321 appendix A.5 vectors, plus a length that forces the padding into a second block.
#[test]
fn matches_the_rfc_1321_vectors() {
    assert_eq!(hex(md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(hex(md5(b"a")), "0cc175b9c0f1b6a831c399e269772661");
    assert_eq!(hex(md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(
        hex(md5(b"message digest")),
        "f96b697d7cb7938d525a2f31aaf161d0"
    );
    assert_eq!(
        hex(md5(
            b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"
        )),
        "57edf4a22be3c955ac49da2e2107b67a"
    );
}
