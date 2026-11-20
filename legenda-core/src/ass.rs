//! Tolerant ASS/SSA parser and strict, libass-compatible writer.
//!
//! Script info, style and event sections are parsed field-by-field according
//! to their `Format:` lines. Unknown sections, comments and non-Dialogue event
//! kinds are preserved so that a full document survives a round trip.

use crate::error::{parse_error, Error, Result};
use crate::text_util::normalize_newlines;
use crate::time::Time;

pub const DEFAULT_STYLE_FORMAT: &[&str] = &[
    "Name",
    "Fontname",
    "Fontsize",
    "PrimaryColour",
    "SecondaryColour",
    "OutlineColour",
    "BackColour",
    "Bold",
    "Italic",
    "Underline",
    "StrikeOut",
    "ScaleX",
    "ScaleY",
    "Spacing",
    "Angle",
    "BorderStyle",
    "Outline",
    "Shadow",
    "Alignment",
    "MarginL",
    "MarginR",
    "MarginV",
    "Encoding",
];

pub const DEFAULT_STYLE_VALUES: &[&str] = &[
    "Default",
    "Arial",
    "20",
    "&H00FFFFFF&",
    "&H000000FF&",
    "&H00000000&",
    "&H80000000&",
    "0",
    "0",
    "0",
    "0",
    "100",
    "100",
    "0",
    "0",
    "1",
    "2",
    "2",
    "2",
    "10",
    "10",
    "10",
    "1",
];

pub const DEFAULT_EVENT_FORMAT: &[&str] =
    &["Layer", "Start", "End", "Style", "Name", "MarginL", "MarginR", "MarginV", "Effect", "Text"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssStyle {
    /// Values aligned to the enclosing section's `Format:` fields.
    pub values: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssEvent {
    /// `Dialogue`, `Comment`, `Picture`, ... exactly as written.
    pub kind: String,
    /// Values aligned to the enclosing section's `Format:` fields.
    pub values: Vec<String>,
}

impl AssEvent {
    pub fn field<'a>(&'a self, format: &[String], name: &str) -> Option<&'a str> {
        let index = format.iter().position(|field| field.eq_ignore_ascii_case(name))?;
        self.values.get(index).map(String::as_str)
    }

    pub fn start(&self, format: &[String]) -> Result<Time> {
        let value = self.required_field(format, "Start")?;
        Time::parse_ass(value)
    }

    pub fn end(&self, format: &[String]) -> Result<Time> {
        let value = self.required_field(format, "End")?;
        Time::parse_ass(value)
    }

    pub fn text(&self, format: &[String]) -> Result<&str> {
        self.required_field(format, "Text")
    }

    pub fn required_field<'a>(&'a self, format: &[String], name: &'static str) -> Result<&'a str> {
        self.field(format, name)
            .ok_or_else(|| Error::MissingAssField { field: name, kind: self.kind.clone() })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssSection {
    ScriptInfo { lines: Vec<String> },
    Styles { header: String, format: Vec<String>, styles: Vec<AssStyle>, comments: Vec<String> },
    Events { format: Vec<String>, events: Vec<AssEvent>, comments: Vec<String> },
    Raw { header: String, lines: Vec<String> },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AssDocument {
    pub sections: Vec<AssSection>,
}

impl AssDocument {
    /// Iterates over every event together with the field format of its section.
    pub fn events(&self) -> impl Iterator<Item = (&[String], &AssEvent)> + '_ {
        self.sections
            .iter()
            .filter_map(|section| match section {
                AssSection::Events { format, events, .. } => Some((format.as_slice(), events)),
                _ => None,
            })
            .flat_map(|(format, events)| events.iter().map(move |event| (format, event)))
    }

    /// Returns all dialogue events with their section's field format.
    pub fn dialogues(&self) -> Vec<(&[String], &AssEvent)> {
        let mut result = Vec::new();
        for section in &self.sections {
            if let AssSection::Events { format, events, .. } = section {
                for event in events {
                    if event.kind.eq_ignore_ascii_case("Dialogue") {
                        result.push((format.as_slice(), event));
                    }
                }
            }
        }
        result
    }
}

pub fn parse(input: &str) -> Result<AssDocument> {
    let normalised = normalize_newlines(input);
    let mut sections = Vec::new();

    for (header, lines) in split_sections(&normalised) {
        let lowered = header.to_ascii_lowercase();
        let section = match lowered.as_str() {
            "[script info]" => AssSection::ScriptInfo { lines },
            "[v4+ styles]" | "[v4 styles]" => parse_styles(header, lines),
            "[events]" => parse_events(lines)?,
            _ => AssSection::Raw { header, lines },
        };
        sections.push(section);
    }

    Ok(AssDocument { sections })
}

fn split_sections(input: &str) -> Vec<(String, Vec<String>)> {
    let mut sections = Vec::new();
    let mut header = String::new();
    let mut lines: Vec<String> = Vec::new();
    let mut have_header = false;

    for raw_line in input.lines() {
        let line = raw_line.trim_end();
        let trimmed = line.trim();
        let is_header = trimmed.len() >= 2 && trimmed.starts_with('[') && trimmed.ends_with(']');
        if is_header {
            if have_header || !lines.is_empty() {
                sections.push((std::mem::take(&mut header), std::mem::take(&mut lines)));
            }
            have_header = true;
            header = trimmed.to_owned();
        } else {
            lines.push(line.to_owned());
        }
    }
    if have_header || !lines.is_empty() {
        sections.push((header, lines));
    }
    sections
}

fn parse_styles(header: String, lines: Vec<String>) -> AssSection {
    let mut format: Option<Vec<String>> = None;
    let mut styles = Vec::new();
    let mut comments = Vec::new();

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) =
            trimmed.strip_prefix("Format:").or_else(|| trimmed.strip_prefix("format:"))
        {
            format = Some(split_fields(rest, DEFAULT_STYLE_FORMAT.len()));
        } else if let Some(rest) =
            trimmed.strip_prefix("Style:").or_else(|| trimmed.strip_prefix("style:"))
        {
            let fields = format.clone().unwrap_or_else(|| default_format(DEFAULT_STYLE_FORMAT));
            styles.push(AssStyle { values: split_values(rest.trim(), fields.len()) });
        } else {
            comments.push(line);
        }
    }

    AssSection::Styles {
        header,
        format: format.unwrap_or_else(|| default_format(DEFAULT_STYLE_FORMAT)),
        styles,
        comments,
    }
}

fn parse_events(lines: Vec<String>) -> Result<AssSection> {
    let mut format: Option<Vec<String>> = None;
    let mut events = Vec::new();
    let mut comments = Vec::new();

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.get(..7).is_some_and(|prefix| prefix.eq_ignore_ascii_case("Format:")) {
            format = Some(split_fields(&trimmed[7..], DEFAULT_EVENT_FORMAT.len()));
            continue;
        }
        if let Some((kind, rest)) = trimmed.split_once(':') {
            if !kind.is_empty() && kind.chars().all(|character| character.is_ascii_alphabetic()) {
                let field_names =
                    format.clone().unwrap_or_else(|| default_format(DEFAULT_EVENT_FORMAT));
                let values = split_values(rest.trim(), field_names.len());
                events.push(AssEvent { kind: kind.to_owned(), values });
                continue;
            }
        }
        comments.push(line);
    }

    let format =
        format.ok_or_else(|| parse_error("ASS", "[Events] section has no Format: line"))?;
    Ok(AssSection::Events { format, events, comments })
}

fn default_format(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn split_fields(text: &str, expected: usize) -> Vec<String> {
    let mut fields: Vec<String> = text.split(',').map(|field| field.trim().to_owned()).collect();
    fields.resize(expected, String::new());
    fields
}

fn split_values(text: &str, field_count: usize) -> Vec<String> {
    let mut values: Vec<String> =
        text.splitn(field_count, ',').map(|value| value.trim().to_owned()).collect();
    values.resize(field_count, String::new());
    values
}

pub fn write(document: &AssDocument) -> Result<String> {
    let mut output = String::new();
    for section in &document.sections {
        match section {
            AssSection::ScriptInfo { lines } => {
                output.push_str("[Script Info]\n");
                for line in lines {
                    output.push_str(line);
                    output.push('\n');
                }
            }
            AssSection::Styles { header, format, styles, comments } => {
                output.push_str(header);
                output.push('\n');
                output.push_str("Format: ");
                output.push_str(&format.join(", "));
                output.push('\n');
                for comment in comments {
                    output.push_str(comment);
                    output.push('\n');
                }
                for style in styles {
                    output.push_str("Style: ");
                    output.push_str(&aligned_values(&style.values, format.len()));
                    output.push('\n');
                }
            }
            AssSection::Events { format, events, comments } => {
                output.push_str("[Events]\n");
                output.push_str("Format: ");
                output.push_str(&format.join(", "));
                output.push('\n');
                for comment in comments {
                    output.push_str(comment);
                    output.push('\n');
                }
                for event in events {
                    output.push_str(&event.kind);
                    output.push_str(": ");
                    output.push_str(&aligned_values(&event.values, format.len()));
                    output.push('\n');
                }
            }
            AssSection::Raw { header, lines } => {
                if !header.is_empty() {
                    output.push_str(header);
                    output.push('\n');
                }
                for line in lines {
                    output.push_str(line);
                    output.push('\n');
                }
            }
        }
    }
    Ok(output)
}

fn aligned_values(values: &[String], field_count: usize) -> String {
    let mut adjusted: Vec<&str> = values.iter().map(String::as_str).collect();
    adjusted.resize(field_count, "");
    adjusted.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 480\n\
                          \n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, \
                          SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, \
                          StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, \
                          Alignment, MarginL, MarginR, MarginV, Encoding\n\
                          Style: Default,Arial,20,&H00FFFFFF&,&H000000FF&,&H00000000&,&H80000000&,\
                          0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\
                          \n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
                          Dialogue: 0,0:00:01.00,0:00:02.50,Default,,0,0,0,,{\\i1}Olá{\\i0}\n";

    #[test]
    fn parses_full_ass_document() {
        let document = parse(SAMPLE).unwrap();
        assert_eq!(document.sections.len(), 3);
        let dialogues = document.dialogues();
        assert_eq!(dialogues.len(), 1);
        let (format, dialogue) = dialogues[0];
        assert_eq!(dialogue.start(format).unwrap(), Time::from_millis(1_000));
        assert_eq!(dialogue.end(format).unwrap(), Time::from_millis(2_500));
        assert_eq!(dialogue.text(format).unwrap(), "{\\i1}Olá{\\i0}");
    }

    #[test]
    fn round_trip_preserves_all_sections() {
        let document = parse(SAMPLE).unwrap();
        let written = write(&document).unwrap();
        assert_eq!(parse(&written).unwrap(), document);
    }

    #[test]
    fn preserves_unknown_sections_and_comments() {
        let input = "[Script Info]\n; gerado por teste\nScriptType: v4.00+\n\
                     [Fonts]\nfontname: Arial\n\
                     [Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
                     Comment: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,nota\n\
                     Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,texto\n";
        let document = parse(input).unwrap();
        assert_eq!(parse(&write(&document).unwrap()).unwrap(), document);
        assert!(
            matches!(&document.sections[1], AssSection::Raw { header, .. } if header == "[Fonts]")
        );
    }

    #[test]
    fn rejects_events_without_format() {
        let error = parse("[Events]\nDialogue: 0,0:00:00.00,0:00:01.00\n").unwrap_err();
        assert!(error.to_string().contains("no Format: line"));
    }

    #[test]
    fn handles_ssa_v4_styles_header() {
        let input = "[Script Info]\nScriptType: v4.00\n\
                     [V4 Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, \
                     OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, \
                     Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
                     Style: Default,Arial,20,&H00FFFFFF&,&H000000FF&,&H00000000&,&H80000000&,\
                     0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\
                     [Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n";
        let document = parse(input).unwrap();
        assert!(
            matches!(&document.sections[1], AssSection::Styles { header, .. } if header == "[V4 Styles]")
        );
    }
}
