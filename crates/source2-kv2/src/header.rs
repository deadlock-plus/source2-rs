//! The `<!-- dmx encoding ... -->` line.

use crate::{Document, Encoding, Error, Newline, Result};

/// Binary versions this crate reads and writes.
///
/// Versions 6, 7 and 8 are not here: no public reader documents their layout, and no
/// sample file was available to derive it.
pub(crate) const BINARY_VERSIONS: [u32; 6] = [1, 2, 3, 4, 5, 9];

/// The header line is short; anything longer than this is not one.
const MAX_LINE: usize = 512;

pub(crate) struct Header {
    pub encoding: Encoding,
    pub version: u32,
    pub format: String,
    pub format_version: u32,
    pub newline: Newline,
    /// Offset of the first body byte.
    pub body: usize,
}

/// Parses the header line.
///
/// Accepted: LF or CRLF line end, any run of spaces or tabs between words, and (text only) a header with no line end at all. Verified
/// against shipped files: LF binary, CRLF text. The others are accepted
/// defensively and have no sample. A byte order mark is rejected, since it could not be
/// written back.
pub(crate) fn parse(bytes: &[u8]) -> Result<Header> {
    let window = &bytes[..bytes.len().min(MAX_LINE)];
    let (line_bytes, mut body, newline) = match window.iter().position(|&b| b == b'\n') {
        Some(nl) => {
            let crlf = nl > 0 && window[nl - 1] == b'\r';
            (
                &window[..nl],
                nl + 1,
                if crlf { Newline::CrLf } else { Newline::Lf },
            )
        }
        None if window.len() == bytes.len() => (window, bytes.len(), Newline::Lf),
        None => return Err(Error::BadHeader("no header line".into())),
    };
    let line = std::str::from_utf8(line_bytes)
        .map_err(|_| Error::BadHeader("header is not UTF-8".into()))?;
    let t: Vec<&str> = line.split_ascii_whitespace().collect();
    if t.len() != 9
        || t[0] != "<!--"
        || t[1] != "dmx"
        || t[2] != "encoding"
        || t[5] != "format"
        || t[8] != "-->"
    {
        return Err(Error::BadHeader(format!("`{}`", line.trim_end())));
    }
    let num = |s: &str| {
        s.parse::<u32>()
            .map_err(|_| Error::BadHeader(format!("`{s}` is not a version number")))
    };
    let version = num(t[4])?;
    let format_version = num(t[7])?;
    let encoding =
        Encoding::from_name(t[3]).ok_or_else(|| Error::UnsupportedEncoding(t[3].to_string()))?;
    if encoding == Encoding::Binary {
        if !BINARY_VERSIONS.contains(&version) {
            return Err(Error::UnsupportedVersion {
                encoding: t[3].to_string(),
                version,
            });
        }
        if bytes.get(body) != Some(&0) {
            return Err(Error::BadHeader("binary header lacks its NUL".into()));
        }
        body += 1;
    }
    Ok(Header {
        encoding,
        version,
        format: t[6].to_string(),
        format_version,
        newline,
        body,
    })
}

pub(crate) fn write(doc: &Document, out: &mut Vec<u8>) -> Result<()> {
    if doc.format.is_empty() || doc.format.contains(|c: char| c.is_whitespace()) {
        return Err(Error::InvalidModel(format!(
            "format name `{}` must be non-empty and contain no whitespace",
            doc.format
        )));
    }
    if doc.encoding == Encoding::Binary && !BINARY_VERSIONS.contains(&doc.encoding_version) {
        return Err(Error::UnsupportedVersion {
            encoding: doc.encoding.name().to_string(),
            version: doc.encoding_version,
        });
    }
    let nl = match doc.encoding {
        Encoding::Binary => "\n",
        _ => doc.text_style.newline.as_str(),
    };
    out.extend_from_slice(
        format!(
            "<!-- dmx encoding {} {} format {} {} -->{nl}",
            doc.encoding.name(),
            doc.encoding_version,
            doc.format,
            doc.format_version
        )
        .as_bytes(),
    );
    if doc.encoding == Encoding::Binary {
        out.push(0);
    }
    Ok(())
}
