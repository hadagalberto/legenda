//! Format-neutral document types and dispatch helpers.

use std::path::Path;

use crate::ass::{self, AssDocument, AssSection};
use crate::convert::{ass_from_plain, ass_to_plain_text, plain_to_ass_text};
use crate::encoding;
use crate::error::{Error, Result};
use crate::srt::{self, SrtDocument};
use crate::time::Time;
use crate::vtt::{self, VttBlock, VttCue, WebVttDocument};

/// Supported subtitle formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Srt,
    Vtt,
    Ass,
}

impl Format {
    pub const ALL: [Self; 3] = [Self::Srt, Self::Vtt, Self::Ass];

    pub fn from_extension(path: impl AsRef<Path>) -> Option<Self> {
        let extension = path.as_ref().extension()?.to_str()?;
        Self::from_name(extension)
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "srt" => Some(Self::Srt),
            "vtt" | "webvtt" => Some(Self::Vtt),
            "ass" | "ssa" => Some(Self::Ass),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Srt => "srt",
            Self::Vtt => "vtt",
            Self::Ass => "ass",
        }
    }

    /// Detects the format from the file extension, falling back to content
    /// sniffing for files with an unusual or missing extension.
    pub fn detect(bytes: &[u8], path: impl AsRef<Path>) -> Result<Self> {
        if let Some(format) = Self::from_extension(&path) {
            return Ok(format);
        }
        let (text, _) = encoding::decode(bytes);
        let snippet = text.chars().take(256).collect::<String>();
        if snippet.trim_start().starts_with("WEBVTT") {
            Ok(Self::Vtt)
        } else if snippet.contains("[Script Info]") || snippet.contains("[Events]") {
            Ok(Self::Ass)
        } else if snippet.contains("-->") {
            Ok(Self::Srt)
        } else {
            Err(Error::UnsupportedFormat(
                path.as_ref()
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .unwrap_or("<none>")
                    .to_owned(),
            ))
        }
    }
}

/// A cue in the format-neutral representation used for conversions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlainCue {
    pub start: Time,
    pub end: Time,
    /// Neutral markup: SRT/WebVTT-style inline tags.
    pub text: String,
}

/// A parsed subtitle document in one of the supported formats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Subtitle {
    Srt(SrtDocument),
    Vtt(WebVttDocument),
    Ass(AssDocument),
}

impl Subtitle {
    /// Decodes the bytes (including encoding detection) and parses the format.
    pub fn decode(bytes: &[u8], format: Format) -> Result<Self> {
        let (text, _) = encoding::decode(bytes);
        Ok(match format {
            Format::Srt => Self::Srt(srt::parse(&text)?),
            Format::Vtt => Self::Vtt(vtt::parse(&text)?),
            Format::Ass => Self::Ass(ass::parse(&text)?),
        })
    }

    /// Serialises to UTF-8 bytes.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let text = match self {
            Self::Srt(document) => srt::write(document),
            Self::Vtt(document) => vtt::write(document),
            Self::Ass(document) => ass::write(document)?,
        };
        Ok(text.into_bytes())
    }

    pub fn format(&self) -> Format {
        match self {
            Self::Srt(_) => Format::Srt,
            Self::Vtt(_) => Format::Vtt,
            Self::Ass(_) => Format::Ass,
        }
    }

    /// Replaces the timing and text of every cue while preserving the
    /// document's format-specific structure (SRT indices, WebVTT ids/settings
    /// and raw NOTE/STYLE blocks, ASS styles and non-dialogue events).
    ///
    /// ASS override markup is converted to neutral markup on read by
    /// [`Subtitle::plain_cues`] and rebuilt from neutral markup here; unknown
    /// ASS overrides are therefore not retained by edit operations.
    pub fn replace_plain_cues(&mut self, cues: &[PlainCue]) -> Result<()> {
        match self {
            Self::Srt(document) => {
                check_count(document.cues.len(), cues.len())?;
                for (target, source) in document.cues.iter_mut().zip(cues) {
                    target.start = source.start;
                    target.end = source.end;
                    target.text.clone_from(&source.text);
                }
            }
            Self::Vtt(document) => {
                let count = document
                    .blocks
                    .iter()
                    .filter(|block| matches!(block, VttBlock::Cue(_)))
                    .count();
                check_count(count, cues.len())?;
                let mut source = cues.iter();
                for block in &mut document.blocks {
                    if let VttBlock::Cue(cue) = block {
                        if let Some(next) = source.next() {
                            cue.start = next.start;
                            cue.end = next.end;
                            cue.text.clone_from(&next.text);
                        }
                    }
                }
            }
            Self::Ass(document) => {
                let dialogue_count = document.dialogues().len();
                check_count(dialogue_count, cues.len())?;
                let mut cursor = cues.iter();
                for section in &mut document.sections {
                    let AssSection::Events { format, events, .. } = section else {
                        continue;
                    };
                    let start_index =
                        format.iter().position(|name| name.eq_ignore_ascii_case("Start"));
                    let end_index = format.iter().position(|name| name.eq_ignore_ascii_case("End"));
                    let text_index =
                        format.iter().position(|name| name.eq_ignore_ascii_case("Text"));
                    let (Some(start_index), Some(end_index), Some(text_index)) =
                        (start_index, end_index, text_index)
                    else {
                        continue;
                    };
                    for event in events.iter_mut() {
                        if !event.kind.eq_ignore_ascii_case("Dialogue") {
                            continue;
                        }
                        if let Some(next) = cursor.next() {
                            set_field(event, start_index, next.start.format_ass());
                            set_field(event, end_index, next.end.format_ass());
                            set_field(event, text_index, plain_to_ass_text(&next.text));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn plain_cues(&self) -> Result<Vec<PlainCue>> {
        Ok(match self {
            Self::Srt(document) => document
                .cues
                .iter()
                .map(|cue| PlainCue { start: cue.start, end: cue.end, text: cue.text.clone() })
                .collect(),
            Self::Vtt(document) => document
                .blocks
                .iter()
                .filter_map(|block| match block {
                    VttBlock::Cue(cue) => Some(cue),
                    VttBlock::Raw(_) => None,
                })
                .map(|cue| PlainCue { start: cue.start, end: cue.end, text: cue.text.clone() })
                .collect(),
            Self::Ass(document) => {
                let mut cues = Vec::new();
                for (format, event) in document.dialogues() {
                    cues.push(PlainCue {
                        start: event.start(format)?,
                        end: event.end(format)?,
                        text: ass_to_plain_text(event.text(format)?),
                    });
                }
                cues
            }
        })
    }

    /// Builds a document in `format` from neutral cues.
    pub fn from_plain(format: Format, cues: &[PlainCue]) -> Result<Self> {
        Ok(match format {
            Format::Srt => Self::Srt(SrtDocument {
                cues: cues
                    .iter()
                    .enumerate()
                    .map(|(position, cue)| crate::srt::SrtCue {
                        index: position as u32 + 1,
                        start: cue.start,
                        end: cue.end,
                        text: cue.text.clone(),
                    })
                    .collect(),
            }),
            Format::Vtt => Self::Vtt(WebVttDocument {
                header: vec!["WEBVTT".to_owned()],
                blocks: cues
                    .iter()
                    .map(|cue| {
                        VttBlock::Cue(VttCue {
                            id: None,
                            start: cue.start,
                            end: cue.end,
                            settings: String::new(),
                            text: cue.text.clone(),
                        })
                    })
                    .collect(),
            }),
            Format::Ass => Self::Ass(ass_from_plain(cues)),
        })
    }
}

fn check_count(expected: usize, actual: usize) -> Result<()> {
    if expected == actual {
        Ok(())
    } else {
        Err(Error::CueCountMismatch { expected, actual })
    }
}

fn set_field(event: &mut crate::ass::AssEvent, index: usize, value: String) {
    if let Some(field) = event.values.get_mut(index) {
        *field = value;
    }
}
