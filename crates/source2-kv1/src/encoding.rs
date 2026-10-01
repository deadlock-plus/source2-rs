/// How the strings of a document map to bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Encoding {
    /// UTF-8.
    #[default]
    Utf8,
    /// Windows-1252. Used when the input is not valid UTF-8. Every byte maps to one
    /// character (the five undefined bytes map to the C1 control of the same value), so the
    /// original bytes are recovered exactly.
    Windows1252,
}

const HIGH: [char; 32] = [
    '\u{20ac}', '\u{81}', '\u{201a}', '\u{192}', '\u{201e}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{2c6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '\u{8d}', '\u{17d}', '\u{8f}',
    '\u{90}', '\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{2dc}', '\u{2122}', '\u{161}', '\u{203a}', '\u{153}', '\u{9d}', '\u{17e}', '\u{178}',
];

pub(crate) fn decode_1252(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9f => HIGH[usize::from(b - 0x80)],
            _ => char::from(b),
        })
        .collect()
}

/// Encodes to Windows-1252, or returns the first character that has no byte.
pub(crate) fn encode_1252(s: &str) -> Result<Vec<u8>, char> {
    s.chars()
        .map(|c| {
            let cp = u32::from(c);
            if cp < 0x80 || (0xa0..=0xff).contains(&cp) {
                return u8::try_from(cp).map_err(|_| c);
            }
            HIGH.iter()
                .position(|&h| h == c)
                .and_then(|i| u8::try_from(i).ok())
                .map(|i| 0x80 + i)
                .ok_or(c)
        })
        .collect()
}

impl Encoding {
    pub(crate) fn encode(self, s: &str) -> Result<Vec<u8>, char> {
        match self {
            Self::Utf8 => Ok(s.as_bytes().to_vec()),
            Self::Windows1252 => encode_1252(s),
        }
    }
}
