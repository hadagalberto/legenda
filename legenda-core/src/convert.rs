//! Format conversion and the documented styling mapping.
//!
//! The neutral text representation used between formats is SRT/WebVTT-like
//! inline markup (`<i>`, `<b>`, `<u>`, `<font color="#RRGGBB">`). ASS override
//! tags are translated to and from that representation:
//!
//! | Neutral markup | ASS override |
//! | --- | --- |
//! | `<i>` / `</i>` | `{\i1}` / `{\i0}` |
//! | `<b>` / `</b>` | `{\b1}` / `{\b0}` |
//! | `<u>` / `</u>` | `{\u1}` / `{\u0}` |
//! | `<font color="#RRGGBB">` | `{\c&H00BBGGRR&}` |
//! | `</font>` | `{\c&H00FFFFFF&}` (the default style colour) |
//! | newline | `\N` |
//!
//! Known limitations, deliberately documented rather than silently guessed:
//! ASS positioning/transform overrides (`\pos`, `\move`, `\t`, ...) and style
//! resets (`\r`) are dropped on conversion to text formats. WebVTT class
//! markers (`<c.class>`) and voice markers (`<v>`) have no SRT equivalent and
//! are dropped when converting WebVTT to ASS. Colours are converted exactly
//! between `#RRGGBB` and ASS's `&HAABBGGRR&`; alpha is always set to opaque.

use crate::ass::{
    self, AssDocument, AssSection, AssStyle, DEFAULT_EVENT_FORMAT, DEFAULT_STYLE_FORMAT,
    DEFAULT_STYLE_VALUES,
};
use crate::error::Result;
use crate::model::{Format, PlainCue, Subtitle};

pub fn convert(source: &Subtitle, target: Format) -> Result<Subtitle> {
    Subtitle::from_plain(target, &source.plain_cues()?)
}

/// Converts ASS override markup into the neutral inline-markup representation.
pub fn ass_to_plain_text(text: &str) -> String {
    let characters: Vec<char> = text.chars().collect();
    let mut output = String::new();
    let mut index = 0;

    while index < characters.len() {
        if characters[index] == '{' {
            if let Some(end) =
                (index + 1..characters.len()).find(|&position| characters[position] == '}')
            {
                let overrides: String = characters[index + 1..end].iter().collect();
                output.push_str(&translate_ass_overrides(&overrides));
                index = end + 1;
                continue;
            }
        }
        // ASS also allows bare line-break escapes outside override groups.
        if characters[index] == '\\' && index + 1 < characters.len() {
            match characters[index + 1] {
                'N' | 'n' => {
                    output.push('\n');
                    index += 2;
                    continue;
                }
                'h' => {
                    output.push(' ');
                    index += 2;
                    continue;
                }
                '{' | '}' => {
                    output.push(characters[index + 1]);
                    index += 2;
                    continue;
                }
                _ => {}
            }
        }
        output.push(characters[index]);
        index += 1;
    }
    output
}

fn translate_ass_overrides(overrides: &str) -> String {
    let mut output = String::new();
    for tag in overrides.split('\\') {
        let tag = tag.trim();
        if tag.is_empty() {
            continue;
        }
        if tag.eq_ignore_ascii_case("N") || tag.eq_ignore_ascii_case("n") {
            output.push('\n');
        } else if tag.eq_ignore_ascii_case("h") {
            output.push(' ');
        } else if let Some(enabled) = ass_flag(tag, 'i') {
            output.push_str(if enabled { "<i>" } else { "</i>" });
        } else if let Some(enabled) = ass_flag(tag, 'b') {
            output.push_str(if enabled { "<b>" } else { "</b>" });
        } else if let Some(enabled) = ass_flag(tag, 'u') {
            output.push_str(if enabled { "<u>" } else { "</u>" });
        } else if let Some(colour) = ass_colour(tag) {
            output.push_str(&format!("<font color=\"{colour}\">"));
        }
        // Unsupported overrides are intentionally discarded.
    }
    output
}

fn ass_flag(tag: &str, letter: char) -> Option<bool> {
    let lowered = tag.to_ascii_lowercase();
    let value = lowered.strip_prefix(letter)?;
    value.parse::<u32>().ok().map(|number| number != 0)
}

fn ass_colour(tag: &str) -> Option<String> {
    let lowered = tag.to_ascii_lowercase();
    let value = lowered.strip_prefix("1c").or_else(|| lowered.strip_prefix('c'))?;
    let digits = value.strip_prefix("&h")?.trim_end_matches('&');
    if digits.len() != 6 && digits.len() != 8 {
        return None;
    }
    let bgr = u32::from_str_radix(&digits[digits.len() - 6..], 16).ok()?;
    let red = bgr & 0xFF;
    let green = (bgr >> 8) & 0xFF;
    let blue = (bgr >> 16) & 0xFF;
    Some(format!("#{red:02X}{green:02X}{blue:02X}"))
}

/// Converts neutral inline markup into ASS override markup.
pub fn plain_to_ass_text(text: &str) -> String {
    let characters: Vec<char> = text.chars().collect();
    let mut output = String::new();
    let mut index = 0;

    while index < characters.len() {
        match characters[index] {
            '\n' => output.push_str("\\N"),
            '{' | '}' => {
                // Literal braces would be read as override groups by libass.
                output.push('\\');
                output.push(characters[index]);
            }
            '<' => {
                if let Some(end) =
                    (index + 1..characters.len()).find(|&position| characters[position] == '>')
                {
                    let tag: String = characters[index + 1..end].iter().collect();
                    output.push_str(&translate_neutral_tag(&tag));
                    index = end;
                } else {
                    output.push('<');
                }
            }
            character => output.push(character),
        }
        index += 1;
    }
    output
}

fn translate_neutral_tag(tag: &str) -> String {
    let lowered = tag.trim().to_ascii_lowercase();
    match lowered.as_str() {
        "i" => "{\\i1}".to_owned(),
        "/i" => "{\\i0}".to_owned(),
        "b" => "{\\b1}".to_owned(),
        "/b" => "{\\b0}".to_owned(),
        "u" => "{\\u1}".to_owned(),
        "/u" => "{\\u0}".to_owned(),
        "br" | "/br" => "\\N".to_owned(),
        "/font" | "/c" => "{\\c&H00FFFFFF&}".to_owned(),
        "c" => String::new(),
        _ => match font_colour(&lowered) {
            Some(colour) => format!("{{\\c&H00{colour}&}}"),
            None => String::new(),
        },
    }
}

fn font_colour(tag: &str) -> Option<String> {
    let after_font = tag.strip_prefix("font")?;
    let after_color = after_font.split_once("color")?.1;
    let value = after_color.trim_start().strip_prefix('=')?.trim_start();
    let value = value.trim_matches('"').trim_matches('\'').trim_start_matches('#');
    if value.len() != 6 {
        return None;
    }
    let rgb = u32::from_str_radix(value, 16).ok()?;
    let red = (rgb >> 16) & 0xFF;
    let green = (rgb >> 8) & 0xFF;
    let blue = rgb & 0xFF;
    let bgr = (blue << 16) | (green << 8) | red;
    Some(format!("{bgr:06X}"))
}

/// Builds a canonical ASS document from neutral cues.
pub fn ass_from_plain(cues: &[PlainCue]) -> AssDocument {
    let style_format = default_format(DEFAULT_STYLE_FORMAT);
    let event_format = default_format(DEFAULT_EVENT_FORMAT);
    let style_values: Vec<String> =
        DEFAULT_STYLE_VALUES.iter().map(|value| (*value).to_owned()).collect();

    let mut events = Vec::new();
    for cue in cues {
        events.push(ass::AssEvent {
            kind: "Dialogue".to_owned(),
            values: vec![
                "0".to_owned(),
                cue.start.format_ass(),
                cue.end.format_ass(),
                "Default".to_owned(),
                String::new(),
                "0".to_owned(),
                "0".to_owned(),
                "0".to_owned(),
                String::new(),
                plain_to_ass_text(&cue.text),
            ],
        });
    }

    AssDocument {
        sections: vec![
            AssSection::ScriptInfo {
                lines: vec![
                    "ScriptType: v4.00+".to_owned(),
                    "PlayResX: 384".to_owned(),
                    "PlayResY: 288".to_owned(),
                    "WrapStyle: 2".to_owned(),
                    "ScaledBorderAndShadow: yes".to_owned(),
                ],
            },
            AssSection::Styles {
                header: "[V4+ Styles]".to_owned(),
                format: style_format,
                styles: vec![AssStyle { values: style_values }],
                comments: Vec::new(),
            },
            AssSection::Events { format: event_format, events, comments: Vec::new() },
        ],
    }
}

fn default_format(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::Time;

    #[test]
    fn ass_override_tags_map_to_neutral_markup() {
        assert_eq!(
            ass_to_plain_text("{\\i1}Olá{\\i0} {\\b1}{\\u1}mundo{\\u0}{\\b0}"),
            "<i>Olá</i> <b><u>mundo</u></b>"
        );
        assert_eq!(ass_to_plain_text("linha{\\N}segunda"), "linha\nsegunda");
        assert_eq!(ass_to_plain_text("linha\\Nsegunda\\hespaço"), "linha\nsegunda espaço");
        assert_eq!(
            ass_to_plain_text("{\\c&H0000FF&}vermelho{\\r}"),
            "<font color=\"#FF0000\">vermelho"
        );
    }

    #[test]
    fn neutral_markup_maps_to_ass_overrides() {
        assert_eq!(
            plain_to_ass_text("<i>Olá</i> e <b><u>mundo</u></b>"),
            "{\\i1}Olá{\\i0} e {\\b1}{\\u1}mundo{\\u0}{\\b0}"
        );
        assert_eq!(
            plain_to_ass_text("<font color=\"#FF0000\">vermelho</font>\nnova"),
            "{\\c&H000000FF&}vermelho{\\c&H00FFFFFF&}\\Nnova"
        );
    }

    #[test]
    fn colour_conversion_round_trips() {
        assert_eq!(ass_colour("c&H00FF8000&").as_deref(), Some("#0080FF"));
        assert_eq!(font_colour("font color=\"#0080FF\""), Some("FF8000".to_owned()));
    }

    #[test]
    fn ass_document_is_built_from_neutral_cues() {
        let cues = vec![PlainCue {
            start: Time::from_millis(1_000),
            end: Time::from_millis(2_000),
            text: "<i>Olá</i>".to_owned(),
        }];
        let document = ass_from_plain(&cues);
        let written = ass::write(&document).unwrap();
        assert!(
            written.contains("Dialogue: 0,00:00:01.00,00:00:02.00,Default,,0,0,0,,{\\i1}Olá{\\i0}")
        );
        assert_eq!(ass::parse(&written).unwrap(), document);
    }
}
