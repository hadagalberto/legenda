//! In-house byte-to-text decoding for subtitle files.
//!
//! Subtitle files in the wild arrive as UTF-8 (with or without a BOM), UTF-16
//! (LE or BE, signalled by a BOM) or a legacy single-byte Windows code page.
//! Legacy Brazilian-Portuguese SRT files are frequently encoded as
//! windows-1252, so that page is used as the final fallback. No external
//! encoding library is involved.

use std::fmt;

/// The encoding that was detected for a byte stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Windows1252,
}

impl fmt::Display for Encoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8Bom => "UTF-8 (BOM)",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
            Self::Windows1252 => "windows-1252",
        };
        f.write_str(name)
    }
}

/// Detects the encoding of `bytes` without consuming or copying them.
pub fn detect(bytes: &[u8]) -> Encoding {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        Encoding::Utf8Bom
    } else if bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) {
        Encoding::Utf16Le
    } else if bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        Encoding::Utf16Be
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        Encoding::Utf16Le
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        Encoding::Utf16Be
    } else if std::str::from_utf8(bytes).is_ok() {
        Encoding::Utf8
    } else {
        Encoding::Windows1252
    }
}

/// Decodes `bytes` into a Rust string.
///
/// Decoding is total: malformed UTF-16 sequences become U+FFFD and undecodable
/// single bytes are mapped through the windows-1252 table. The detected
/// encoding is returned alongside the text.
pub fn decode(bytes: &[u8]) -> (String, Encoding) {
    let encoding = detect(bytes);
    let text = match encoding {
        Encoding::Utf8Bom => String::from_utf8_lossy(&bytes[3..]).into_owned(),
        Encoding::Utf16Le => decode_utf16(&bytes[2..], true),
        Encoding::Utf16Be => decode_utf16(&bytes[2..], false),
        Encoding::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
        Encoding::Windows1252 => decode_windows_1252(bytes),
    };
    (text, encoding)
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> String {
    let units = bytes.chunks_exact(2).map(|pair| {
        if little_endian {
            u16::from_le_bytes([pair[0], pair[1]])
        } else {
            u16::from_be_bytes([pair[0], pair[1]])
        }
    });
    char::decode_utf16(units).map(|unit| unit.unwrap_or('\u{FFFD}')).collect()
}

fn decode_windows_1252(bytes: &[u8]) -> String {
    bytes.iter().copied().map(windows_1252_byte).collect()
}

fn windows_1252_byte(byte: u8) -> char {
    match byte {
        0x80 => '\u{20AC}', // €
        0x82 => '\u{201A}', // ‚
        0x83 => '\u{0192}', // ƒ
        0x84 => '\u{201E}', // „
        0x85 => '\u{2026}', // …
        0x86 => '\u{2020}', // †
        0x87 => '\u{2021}', // ‡
        0x88 => '\u{02C6}', // ˆ
        0x89 => '\u{2030}', // ‰
        0x8A => '\u{0160}', // Š
        0x8B => '\u{2039}', // ‹
        0x8C => '\u{0152}', // Œ
        0x8E => '\u{017D}', // Ž
        0x91 => '\u{2018}', // ‘
        0x92 => '\u{2019}', // ’
        0x93 => '\u{201C}', // “
        0x94 => '\u{201D}', // ”
        0x95 => '\u{2022}', // •
        0x96 => '\u{2013}', // –
        0x97 => '\u{2014}', // —
        0x98 => '\u{02DC}', // ˜
        0x99 => '\u{2122}', // ™
        0x9A => '\u{0161}', // š
        0x9B => '\u{203A}', // ›
        0x9C => '\u{0153}', // œ
        0x9E => '\u{017E}', // ž
        0x9F => '\u{0178}', // Ÿ
        // 0x81, 0x8D, 0x8F, 0x90 and 0x9D are undefined in windows-1252.
        // Like most decoders we map them to the matching C1 control code.
        other => other as char,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16_bytes(text: &str, little_endian: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        for unit in text.encode_utf16() {
            if little_endian {
                bytes.extend_from_slice(&unit.to_le_bytes());
            } else {
                bytes.extend_from_slice(&unit.to_be_bytes());
            }
        }
        bytes
    }

    #[test]
    fn detects_plain_utf8() {
        let (text, encoding) = decode("Olá, mundo".as_bytes());
        assert_eq!(text, "Olá, mundo");
        assert_eq!(encoding, Encoding::Utf8);
    }

    #[test]
    fn strips_utf8_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("Olá".as_bytes());
        let (text, encoding) = decode(&bytes);
        assert_eq!(text, "Olá");
        assert_eq!(encoding, Encoding::Utf8Bom);
    }

    #[test]
    fn decodes_utf16_le_with_surrogate_pair() {
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(utf16_bytes("Olá 🌍", true));
        let (text, encoding) = decode(&bytes);
        assert_eq!(text, "Olá 🌍");
        assert_eq!(encoding, Encoding::Utf16Le);
    }

    #[test]
    fn decodes_utf16_be() {
        let mut bytes = vec![0xFE, 0xFF];
        bytes.extend(utf16_bytes("Até logo", false));
        let (text, encoding) = decode(&bytes);
        assert_eq!(text, "Até logo");
        assert_eq!(encoding, Encoding::Utf16Be);
    }

    #[test]
    fn decodes_legacy_windows_1252_srt() {
        // "Olá, mundo €" in windows-1252; 0xE1 is invalid as a lone UTF-8 byte.
        let bytes = [0x4F, 0x6C, 0xE1, 0x2C, 0x20, 0x80];
        let (text, encoding) = decode(&bytes);
        assert_eq!(text, "Olá, €");
        assert_eq!(encoding, Encoding::Windows1252);
    }

    #[test]
    fn windows_1252_undefined_bytes_become_c1_controls() {
        assert_eq!(windows_1252_byte(0x81), '\u{0081}');
        assert_eq!(windows_1252_byte(0x9D), '\u{009D}');
    }
}
