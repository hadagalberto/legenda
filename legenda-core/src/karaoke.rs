//! Word-level karaoke caption generation for vertical video.
//!
//! Every preset emits one ASS [`AssDocument`] with `PlayResX`/`PlayResY` set
//! to the requested canvas (1080x1920 by default), one base style and one
//! dialogue per caption page. Active words are animated with per-word `\t`
//! transforms whose times are relative to the dialogue start, so libass can
//! render them without any external scripting.

use std::collections::HashSet;

use crate::ass::{
    AssDocument, AssEvent, AssSection, AssStyle, DEFAULT_EVENT_FORMAT, DEFAULT_STYLE_FORMAT,
};
use crate::error::{parse_error, Error, Result};
use crate::time::{Rational, Time};
use crate::transcript::{group_pages, Page, PageOptions, Word};

/// Available visual presets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KaraokePreset {
    /// Bold uppercase words, accent-coloured current word with a scale pop.
    Hormozi,
    /// Quiet white text on a subtle translucent box with a soft reveal.
    Clean,
    /// Classic progressive `\k` fill.
    Karaoke,
    /// Accent colour with an overshoot-and-settle scale bounce.
    Bounce,
}

impl KaraokePreset {
    pub fn parse(input: &str) -> Result<Self> {
        match input.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "hormozi" => Ok(Self::Hormozi),
            "clean" => Ok(Self::Clean),
            "karaoke" => Ok(Self::Karaoke),
            "bounce" => Ok(Self::Bounce),
            _ => Err(parse_error("karaoke preset", format!("unknown preset {input:?}"))),
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Hormozi => "hormozi",
            Self::Clean => "clean",
            Self::Karaoke => "karaoke",
            Self::Bounce => "bounce",
        }
    }

    pub const ALL: [Self; 4] = [Self::Hormozi, Self::Clean, Self::Karaoke, Self::Bounce];
}

/// An ASS colour. `alpha` follows ASS convention: `0x00` opaque, `0xFF`
/// transparent. [`Color::from_hex`] accepts intuitive HTML-style opacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl Color {
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue, alpha: 0 }
    }

    pub const fn with_alpha(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self { red, green, blue, alpha }
    }

    /// Parses `#RRGGBB` or `#RRGGBBAA`. The last byte is *opacity* (`FF` =
    /// opaque), matching CSS rather than ASS, and is converted internally.
    pub fn from_hex(input: &str) -> Result<Self> {
        let digits = input.strip_prefix('#').unwrap_or(input);
        if !matches!(digits.len(), 6 | 8) || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::InvalidColour(input.to_owned()));
        }
        let parse = |range: std::ops::Range<usize>| {
            u8::from_str_radix(&digits[range], 16)
                .map_err(|_| Error::InvalidColour(input.to_owned()))
        };
        let red = parse(0..2)?;
        let green = parse(2..4)?;
        let blue = parse(4..6)?;
        let alpha = if digits.len() == 8 { 255 - parse(6..8)? } else { 0 };
        Ok(Self { red, green, blue, alpha })
    }

    /// Formats as the ASS `&HAABBGGRR&` used by `\c`, `\1c` and style fields.
    pub fn format_ass(self) -> String {
        format!("&H{:02X}{:02X}{:02X}{:02X}&", self.alpha, self.blue, self.green, self.red)
    }
}

/// Pixel margins that keep captions clear of platform UI overlays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SafeArea {
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
}

impl SafeArea {
    /// Reasonable defaults for TikTok/Reels/Shorts at 1080x1920.
    pub const fn shorts() -> Self {
        Self { left: 80, right: 80, top: 180, bottom: 420 }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct PresetStyle {
    font: String,
    font_size: u32,
    bold: bool,
    outline: u32,
    shadow: u32,
    border_style: u32,
    primary: Color,
    secondary: Color,
    back: Color,
    accent: Color,
    uppercase: bool,
}

fn preset_style(preset: KaraokePreset) -> PresetStyle {
    match preset {
        KaraokePreset::Hormozi => PresetStyle {
            font: "DejaVu Sans".to_owned(),
            font_size: 92,
            bold: true,
            outline: 6,
            shadow: 0,
            border_style: 1,
            primary: Color::rgb(255, 255, 255),
            secondary: Color::rgb(0, 0, 0),
            back: Color::with_alpha(0, 0, 0, 128),
            accent: Color::rgb(255, 229, 0),
            uppercase: true,
        },
        KaraokePreset::Clean => PresetStyle {
            font: "DejaVu Sans".to_owned(),
            font_size: 76,
            bold: false,
            outline: 2,
            shadow: 1,
            border_style: 3,
            primary: Color::with_alpha(255, 255, 255, 0x40),
            secondary: Color::rgb(255, 255, 255),
            back: Color::with_alpha(0, 0, 0, 128),
            accent: Color::rgb(140, 220, 255),
            uppercase: false,
        },
        KaraokePreset::Karaoke => PresetStyle {
            font: "DejaVu Sans".to_owned(),
            font_size: 84,
            bold: true,
            outline: 4,
            shadow: 1,
            border_style: 1,
            primary: Color::rgb(255, 229, 0),
            secondary: Color::rgb(245, 245, 245),
            back: Color::with_alpha(0, 0, 0, 128),
            accent: Color::rgb(255, 229, 0),
            uppercase: false,
        },
        KaraokePreset::Bounce => PresetStyle {
            font: "DejaVu Sans".to_owned(),
            font_size: 88,
            bold: true,
            outline: 5,
            shadow: 1,
            border_style: 1,
            primary: Color::rgb(255, 255, 255),
            secondary: Color::rgb(0, 0, 0),
            back: Color::with_alpha(0, 0, 0, 128),
            accent: Color::rgb(255, 59, 107),
            uppercase: false,
        },
    }
}

/// Fully resolved options for a karaoke render.
#[derive(Clone, Debug, PartialEq)]
pub struct KaraokeOptions {
    pub preset: KaraokePreset,
    pub width: u32,
    pub height: u32,
    pub font: String,
    pub font_size: u32,
    pub outline: u32,
    pub shadow: u32,
    pub primary: Color,
    pub accent: Color,
    pub emphasis_color: Color,
    pub safe_area: SafeArea,
    /// Words (case-insensitive, punctuation-insensitive) rendered in
    /// `emphasis_color` with a stronger animation.
    pub emphasis: Vec<String>,
    pub page: PageOptions,
}

impl KaraokeOptions {
    pub fn new(preset: KaraokePreset) -> Self {
        let style = preset_style(preset);
        Self {
            preset,
            width: 1_080,
            height: 1_920,
            font: style.font,
            font_size: style.font_size,
            outline: style.outline,
            shadow: style.shadow,
            primary: style.primary,
            accent: style.accent,
            emphasis_color: Color::rgb(0, 229, 255),
            safe_area: SafeArea::shorts(),
            emphasis: Vec::new(),
            page: PageOptions::default(),
        }
    }

    /// Builds an ASS document from words. Page grouping is applied here.
    pub fn render(&self, words: &[Word]) -> Result<AssDocument> {
        render(words, self)
    }
}

/// Builds an ASS document from a word stream using `options`.
pub fn render(words: &[Word], options: &KaraokeOptions) -> Result<AssDocument> {
    let style = preset_style(options.preset);
    let pages = group_pages(words, &options.page);
    let emphasis: HashSet<String> =
        options.emphasis.iter().filter_map(|word| normalise_word(word)).collect();

    let mut events = Vec::with_capacity(pages.len());
    for page in &pages {
        let text = render_page(page, options, &style, &emphasis)?;
        events.push(AssEvent {
            kind: "Dialogue".to_owned(),
            values: vec![
                "0".to_owned(),
                page.start.format_ass(),
                page.end.format_ass(),
                "Karaoke".to_owned(),
                String::new(),
                "0".to_owned(),
                "0".to_owned(),
                "0".to_owned(),
                String::new(),
                text,
            ],
        });
    }

    let style_values = build_style_values(options, &style);
    Ok(AssDocument {
        sections: vec![
            AssSection::ScriptInfo {
                lines: vec![
                    "ScriptType: v4.00+".to_owned(),
                    format!("PlayResX: {}", options.width),
                    format!("PlayResY: {}", options.height),
                    "WrapStyle: 2".to_owned(),
                    "ScaledBorderAndShadow: yes".to_owned(),
                    "YCbCr Matrix: TV.601".to_owned(),
                ],
            },
            AssSection::Styles {
                header: "[V4+ Styles]".to_owned(),
                format: DEFAULT_STYLE_FORMAT.iter().map(|name| (*name).to_owned()).collect(),
                styles: vec![AssStyle { values: style_values }],
                comments: Vec::new(),
            },
            AssSection::Events {
                format: DEFAULT_EVENT_FORMAT.iter().map(|name| (*name).to_owned()).collect(),
                events,
                comments: Vec::new(),
            },
        ],
    })
}

fn render_page(
    page: &Page,
    options: &KaraokeOptions,
    style: &PresetStyle,
    emphasis: &HashSet<String>,
) -> Result<String> {
    let anchor = page_anchor(options);
    let mut text = format!("{{\\pos({},{})}}", anchor.0, anchor.1);
    let last = page.words.len() - 1;
    for (index, word) in page.words.iter().enumerate() {
        let highlighted = normalise_word(&word.text).is_some_and(|text| emphasis.contains(&text));
        text.push_str(&render_word(word, page.start, options, style, highlighted)?);
        if index != last {
            text.push(' ');
        }
    }
    Ok(text)
}

fn render_word(
    word: &Word,
    page_start: Time,
    options: &KaraokeOptions,
    style: &PresetStyle,
    highlighted: bool,
) -> Result<String> {
    let displayed = if style.uppercase {
        escape_ass_text(&word.text.to_uppercase())
    } else {
        escape_ass_text(&word.text)
    };
    let start = centiseconds_since(page_start, word.start);
    let duration = centiseconds_between(word.start, word.end);
    let active = if highlighted { options.emphasis_color } else { options.accent };
    let primary = options.primary.format_ass();

    let tags = match options.preset {
        KaraokePreset::Hormozi => {
            let scale = if highlighted { 125 } else { 115 };
            let (up, down) = (start + 8, start + 16);
            format!(
                "\\t({start},{up},\\c{}\\fscx{scale}\\fscy{scale})\
                 \\t({up},{down},\\c{primary}\\fscx100\\fscy100)",
                active.format_ass()
            )
        }
        KaraokePreset::Clean => {
            let end = start + 10;
            let colour =
                if highlighted { format!("\\c{}", active.format_ass()) } else { String::new() };
            format!("\\t({start},{end},{colour}\\alpha&H00&)")
        }
        KaraokePreset::Karaoke => {
            let fill =
                if highlighted { format!("\\kf{duration}") } else { format!("\\k{duration}") };
            let scale = if highlighted { 112 } else { 108 };
            let (up, down) = (start + 6, start + 12);
            format!(
                "{fill}\\t({start},{up},\\fscx{scale}\\fscy{scale})\
                 \\t({up},{down},\\fscx100\\fscy100)"
            )
        }
        KaraokePreset::Bounce => {
            let (up, down, settle) = (start + 6, start + 12, start + 20);
            let scale = if highlighted { 145 } else { 135 };
            format!(
                "\\t({start},{up},\\fscx{scale}\\fscy{scale}\\c{})\
                 \\t({up},{down},\\fscx88\\fscy88)\
                 \\t({down},{settle},\\fscx100\\fscy100\\c{primary})",
                active.format_ass()
            )
        }
    };
    Ok(format!("{{{tags}}}{displayed}"))
}

fn build_style_values(options: &KaraokeOptions, style: &PresetStyle) -> Vec<String> {
    let mut result = Vec::with_capacity(DEFAULT_STYLE_FORMAT.len());
    result.push("Karaoke".to_owned()); // Name
    result.push(options.font.clone()); // Fontname
    result.push(options.font_size.to_string()); // Fontsize
    result.push(options.primary.format_ass()); // PrimaryColour
    result.push(style.secondary.format_ass()); // SecondaryColour
    result.push(Color::rgb(0, 0, 0).format_ass()); // OutlineColour
    result.push(style.back.format_ass()); // BackColour
    result.push(if style.bold { "-1" } else { "0" }.to_owned()); // Bold
    result.push("0".to_owned()); // Italic
    result.push("0".to_owned()); // Underline
    result.push("0".to_owned()); // StrikeOut
    result.push("100".to_owned()); // ScaleX
    result.push("100".to_owned()); // ScaleY
    result.push("0".to_owned()); // Spacing
    result.push("0".to_owned()); // Angle
    result.push(style.border_style.to_string()); // BorderStyle
    result.push(options.outline.to_string()); // Outline
    result.push(options.shadow.to_string()); // Shadow
    result.push("2".to_owned()); // Alignment: bottom centre
    result.push(options.safe_area.left.to_string()); // MarginL
    result.push(options.safe_area.right.to_string()); // MarginR
    result.push(options.safe_area.bottom.to_string()); // MarginV
    result.push("1".to_owned()); // Encoding
    debug_assert_eq!(result.len(), DEFAULT_STYLE_FORMAT.len());
    result
}

fn page_anchor(options: &KaraokeOptions) -> (u32, u32) {
    let x = options.width / 2;
    let bottom = options.height.saturating_sub(options.safe_area.bottom);
    let top = options.height.saturating_sub(options.safe_area.top);
    (x, bottom.min(top).max(1))
}

fn centiseconds_since(page_start: Time, time: Time) -> i64 {
    let scaled = (time.as_rational() - page_start.as_rational()) * Rational::from_int(100);
    scaled.round().max(0)
}

fn centiseconds_between(start: Time, end: Time) -> i64 {
    let scaled = (end.as_rational() - start.as_rational()) * Rational::from_int(100);
    scaled.round().max(1)
}

fn normalise_word(text: &str) -> Option<String> {
    let normalised = text.trim().trim_matches(|character: char| !character.is_alphanumeric());
    if normalised.is_empty() {
        None
    } else {
        Some(normalised.to_lowercase())
    }
}

/// Converts text to safe ASS text: braces would otherwise be interpreted as
/// override blocks, and control characters would corrupt the script. Emoji
/// and all other Unicode glyphs pass through untouched.
pub fn escape_ass_text(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '{' => output.push('｛'),
            '}' => output.push('｝'),
            '\n' | '\r' | '\t' => output.push(' '),
            character if character.is_control() => output.push(' '),
            _ => output.push(character),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_preset_names_round_trip() {
        for preset in KaraokePreset::ALL {
            assert_eq!(KaraokePreset::parse(preset.name()).unwrap(), preset);
            assert_eq!(KaraokePreset::parse(&preset.name().to_uppercase()).unwrap(), preset);
        }
        assert!(KaraokePreset::parse("unknown").is_err());
    }

    #[test]
    fn colours_follow_ass_channel_order() {
        assert_eq!(Color::rgb(0x12, 0x34, 0x56).format_ass(), "&H00563412&");
        assert_eq!(Color::from_hex("#FFE500").unwrap(), Color::rgb(255, 229, 0));
        assert_eq!(Color::from_hex("#FFFFFF80").unwrap().alpha, 127);
        assert!(Color::from_hex("#FFF").is_err());
        assert!(Color::from_hex("#GGGGGG").is_err());
    }

    #[test]
    fn ass_text_keeps_emoji_and_neutralises_override_blocks() {
        assert_eq!(escape_ass_text("olá 👋 {\\b1}x"), "olá 👋 ｛\\b1｝x");
        assert_eq!(escape_ass_text("a\nb"), "a b");
    }
}
