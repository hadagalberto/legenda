//! Tolerant WebVTT parser and strict writer.
//!
//! NOTE and STYLE blocks are kept as raw text in document order, so they
//! survive a parse -> write -> parse round trip unchanged.

use crate::error::{parse_error, Result};
use crate::text_util::{clean_lines, normalize_newlines, split_blocks};
use crate::time::Time;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VttCue {
    pub id: Option<String>,
    pub start: Time,
    pub end: Time,
    /// Raw cue settings, e.g. `line:0 position:50% align:middle`.
    pub settings: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VttBlock {
    Cue(VttCue),
    /// A NOTE or STYLE block, preserved verbatim.
    Raw(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebVttDocument {
    /// The `WEBVTT` marker line plus any following header lines.
    pub header: Vec<String>,
    pub blocks: Vec<VttBlock>,
}

impl Default for WebVttDocument {
    fn default() -> Self {
        Self { header: vec!["WEBVTT".to_owned()], blocks: Vec::new() }
    }
}

pub fn parse(input: &str) -> Result<WebVttDocument> {
    let normalised = normalize_newlines(input);
    let mut lines = normalised.lines();

    let first = lines.next().ok_or_else(|| parse_error("WebVTT", "empty document"))?;
    let first = first.trim_end();
    if first.trim_start().get(..6) != Some("WEBVTT") {
        return Err(parse_error("WebVTT", "document must start with the WEBVTT marker"));
    }

    let mut header = vec![first.to_owned()];
    for line in lines.by_ref() {
        if line.trim().is_empty() {
            break;
        }
        header.push(line.trim_end().to_owned());
    }

    let remaining: String = lines.collect::<Vec<_>>().join("\n");
    let mut blocks = Vec::new();
    for block in split_blocks(&remaining) {
        let cleaned = clean_lines(&block);
        blocks.push(parse_block(&cleaned)?);
    }

    Ok(WebVttDocument { header, blocks })
}

fn parse_block(lines: &[String]) -> Result<VttBlock> {
    let Some(timestamp_index) = lines.iter().position(|line| line.contains("-->")) else {
        return Ok(VttBlock::Raw(lines.join("\n")));
    };

    let id = if timestamp_index > 0 { Some(lines[..timestamp_index].join("\n")) } else { None };

    let line = &lines[timestamp_index];
    let (start_text, rest) =
        line.split_once("-->").ok_or_else(|| parse_error("WebVTT", "cue has no '-->'"))?;
    let mut tokens = rest.split_whitespace();
    let end_text = tokens.next().ok_or_else(|| parse_error("WebVTT", "cue has no end time"))?;
    let settings = tokens.collect::<Vec<_>>().join(" ");

    Ok(VttBlock::Cue(VttCue {
        id,
        start: Time::parse_vtt(start_text)?,
        end: Time::parse_vtt(end_text)?,
        settings,
        text: lines[timestamp_index + 1..].join("\n"),
    }))
}

pub fn write(document: &WebVttDocument) -> String {
    let mut output = String::new();
    for line in &document.header {
        output.push_str(line);
        output.push('\n');
    }
    output.push('\n');

    for block in &document.blocks {
        match block {
            VttBlock::Cue(cue) => {
                if let Some(id) = &cue.id {
                    output.push_str(id);
                    output.push('\n');
                }
                output.push_str(&cue.start.format_vtt());
                output.push_str(" --> ");
                output.push_str(&cue.end.format_vtt());
                if !cue.settings.is_empty() {
                    output.push(' ');
                    output.push_str(&cue.settings);
                }
                output.push('\n');
                output.push_str(&cue.text);
                output.push('\n');
            }
            VttBlock::Raw(raw) => {
                output.push_str(raw);
                output.push('\n');
            }
        }
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_header_ids_settings_and_short_timestamps() {
        let input = "WEBVTT - Legendado\nKind: captions\n\
                     Language: pt-BR\n\n\
                     intro\n00:01.000 --> 00:03.500 line:0 position:50%\n\
                     Olá, mundo\n\n\
                     00:00:04.000 --> 00:00:05.000\n<i>Segundo</i>\n";
        let document = parse(input).unwrap();
        assert_eq!(document.header.len(), 3);
        let VttBlock::Cue(first) = &document.blocks[0] else {
            panic!("first block should be a cue");
        };
        assert_eq!(first.id.as_deref(), Some("intro"));
        assert_eq!(first.start, Time::from_millis(1_000));
        assert_eq!(first.end, Time::from_millis(3_500));
        assert_eq!(first.settings, "line:0 position:50%");
        assert_eq!(first.text, "Olá, mundo");
        assert_eq!(document.blocks.len(), 2);
    }

    #[test]
    fn preserves_note_and_style_blocks() {
        let input = "WEBVTT\n\nNOTE tradução revisada\npor João\n\n\
                     STYLE\n::cue { color: yellow; }\n\n\
                     00:00:01.000 --> 00:00:02.000\nTexto\n";
        let document = parse(input).unwrap();
        assert!(matches!(&document.blocks[0], VttBlock::Raw(raw) if raw.starts_with("NOTE")));
        assert!(matches!(&document.blocks[1], VttBlock::Raw(raw) if raw.starts_with("STYLE")));
        assert_eq!(parse(&write(&document)).unwrap(), document);
    }

    #[test]
    fn rejects_document_without_marker() {
        let error = parse("NOT WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nOlá\n").unwrap_err();
        assert!(error.to_string().contains("WEBVTT marker"));
    }

    #[test]
    fn writer_uses_full_timestamps() {
        let document = WebVttDocument {
            header: vec!["WEBVTT".to_owned()],
            blocks: vec![VttBlock::Cue(VttCue {
                id: None,
                start: Time::from_millis(1_000),
                end: Time::from_millis(2_000),
                settings: String::new(),
                text: "Olá".to_owned(),
            })],
        };
        assert_eq!(write(&document), "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nOlá\n\n");
    }
}
