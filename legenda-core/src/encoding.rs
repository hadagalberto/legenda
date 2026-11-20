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
    /// Predominantly UTF-8, with a handful of bytes recovered as windows-1252.
    Utf8With1252Fallback,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Windows1252,
}

impl fmt::Display for Encoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8With1252Fallback => "UTF-8 (with windows-1252 fallback)",
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
/// BOM-marked UTF-8 and UTF-16 are decoded as a whole. Byte streams without a
/// BOM are decoded as UTF-8 *per sequence*: valid UTF-8 runs are kept as-is and
/// only the offending bytes are recovered through the windows-1252 table. A
/// single stray legacy byte can therefore no longer turn a whole UTF-8 file
/// into mojibake. Pure legacy files are still reported as `Windows1252`; mixed
/// files are reported as `Utf8With1252Fallback` and raise a warning.
///
/// Decoding is total: malformed UTF-16 sequences become U+FFFD.
pub fn decode(bytes: &[u8]) -> (String, Encoding) {
    let encoding = detect(bytes);
    let text = match encoding {
        Encoding::Utf8Bom => String::from_utf8_lossy(&bytes[3..]).into_owned(),
        Encoding::Utf16Le => decode_utf16(&bytes[2..], true),
        Encoding::Utf16Be => decode_utf16(&bytes[2..], false),
        Encoding::Utf8 | Encoding::Utf8With1252Fallback | Encoding::Windows1252 => {
            let (text, fallback_bytes, has_multibyte_utf8) = decode_utf8_with_fallback(bytes);
            if fallback_bytes == 0 {
                // `detect` classified it as UTF-8: nothing to recover.
                text
            } else if has_multibyte_utf8 {
                crate::warnings::warn(format!(
                    "recovered {fallback_bytes} byte(s) as windows-1252 inside an otherwise UTF-8 file"
                ));
                return (text, Encoding::Utf8With1252Fallback);
            } else {
                // No valid multibyte UTF-8 anywhere: a genuine legacy file.
                // Decoding byte-by-byte through cp1252 gives the same text as
                // the whole-file path but keeps the reported encoding stable.
                decode_windows_1252(bytes)
            }
        }
    };
    (text, encoding)
}

/// Decodes UTF-8, recovering each invalid byte sequence through windows-1252.
///
/// Returns the text, the number of recovered bytes and whether any valid
/// non-ASCII UTF-8 sequence was seen (the signal that distinguishes a mixed
/// file from a pure single-byte legacy file).
fn decode_utf8_with_fallback(bytes: &[u8]) -> (String, usize, bool) {
    let mut output = String::with_capacity(bytes.len());
    let mut fallback_bytes = 0usize;
    let mut has_multibyte_utf8 = false;
    let mut position = 0usize;
    while position < bytes.len() {
        match std::str::from_utf8(&bytes[position..]) {
            Ok(rest) => {
                has_multibyte_utf8 |= rest.bytes().any(|byte| byte >= 0x80);
                output.push_str(rest);
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                // `valid_up_to()` guarantees this slice is valid UTF-8.
                let prefix = std::str::from_utf8(&bytes[position..position + valid])
                    .expect("valid_up_to prefix must be valid UTF-8");
                has_multibyte_utf8 |= prefix.bytes().any(|byte| byte >= 0x80);
                output.push_str(prefix);
                position += valid;
                // `error_len()` is None only for an incomplete tail, which we
                // still consume one byte at a time to keep decoding total.
                let invalid = error.error_len().unwrap_or(1).min(bytes.len() - position);
                for &byte in &bytes[position..position + invalid] {
                    output.push(windows_1252_byte(byte));
                    fallback_bytes += 1;
                }
                position += invalid;
            }
        }
    }
    (output, fallback_bytes, has_multibyte_utf8)
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
