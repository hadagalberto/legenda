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

/// Duration of the scale pop-in, in milliseconds. ASS `\t` times are
/// milliseconds relative to the start of the Dialogue line.
const POP_MS: i64 = 80;

/// Largest scale reached during the pop-in (115%).
const POP_SCALE: f64 = 1.15;

/// Fraction of the safe width used when packing lines. The headroom absorbs
/// glyph outlines and small differences between the estimate and real metrics.
const WIDTH_SAFETY: f64 = 0.96;

/// Advance of an average lowercase glyph as a fraction of the font size.
const BASE_ADVANCE: f64 = 0.56;

/// Bold uppercase glyphs are about this much wider than lowercase ones.
const UPPERCASE_FACTOR: f64 = 1.28;

/// Approximates the rendered width of `text` in script pixels.
///
/// The estimate is intentionally conservative: each glyph contributes a
/// fraction of the font size, alphabetic glyphs are widened for uppercase
/// presets, and wide (`M`, `W`, `@`) and narrow (`i`, punctuation, spaces)
/// glyphs get their own advances. Callers pack lines against
/// `width - safe_left - safe_right`; the estimate is additive, so the width of
/// a line built by joining words with spaces equals the sum of the word widths
/// plus the space widths.
pub fn estimate_text_width(text: &str, font_size: u32, uppercase: bool) -> f64 {
    let em = f64::from(font_size);
    text.chars().map(|character| glyph_advance(character, em, uppercase)).sum()
}

fn glyph_advance(character: char, em: f64, uppercase: bool) -> f64 {
    let base = match character {
        ' ' | '\t' => 0.34,
        'i' | 'j' | 'l' | 'f' | 't' | 'I' | 'J' | 'L' | 'F' | 'T' | '.' | ',' | ':' | ';' | '!'
        | '\'' | '"' | '|' | '`' => 0.32,
        'm' | 'w' | 'M' | 'W' | '@' | '%' => 0.95,
        _ => BASE_ADVANCE,
    };
    // Wide glyphs already carry their extra width; only the average glyphs
    // need the uppercase correction.
    let factor = if uppercase && character.is_alphabetic() && base <= BASE_ADVANCE {
        UPPERCASE_FACTOR
    } else {
        1.0
    };
    em * base * factor
}

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

    /// Whether words are uppercased before rendering. Width estimates must use
    /// the same setting as the generated text.
    pub const fn uppercases_text(self) -> bool {
        matches!(self, Self::Hormozi)
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
            // Large vertical-shorts captions: 3-4 words per page, then the
            // width-aware layout below splits that into at most two lines.
            page: PageOptions { max_words: 4, ..PageOptions::default() },
        }
    }

    /// Builds an ASS document from words. Page grouping is applied here.
    pub fn render(&self, words: &[Word]) -> Result<AssDocument> {
        render(words, self)
    }
}

/// One Dialogue line produced by the layout: up to two visual lines, each of
/// which fits inside the safe width.
struct LayoutPage<'a> {
    start: Time,
    end: Time,
    lines: Vec<Vec<&'a Word>>,
}

/// Horizontal space available to caption glyphs, in script pixels.
fn available_width(options: &KaraokeOptions) -> f64 {
    options
        .width
        .saturating_sub(options.safe_area.left)
        .saturating_sub(options.safe_area.right)
        .max(1) as f64
}

/// Packs the words of each timed page into visual lines. A page is flushed
/// after two lines, so a single Dialogue never exceeds the safe width and
/// never needs more than two lines.
fn layout_pages<'a>(
    pages: &'a [Page],
    options: &KaraokeOptions,
    style: &PresetStyle,
) -> Vec<LayoutPage<'a>> {
    // Reserve room for the scale pop of the active word and for outlines.
    let limit = available_width(options) * WIDTH_SAFETY / POP_SCALE;
    let space = glyph_advance(' ', f64::from(options.font_size), false);
    let uppercase = style.uppercase;
    let font_size = options.font_size;
    let max_lines = 2;

    let mut layouts = Vec::new();
    for page in pages {
        let mut groups: Vec<Vec<Vec<&'a Word>>> = Vec::new();
        let mut lines: Vec<Vec<&'a Word>> = Vec::new();
        let mut current: Vec<&'a Word> = Vec::new();
        let mut current_width = 0.0f64;

        for word in &page.words {
            let width = estimate_text_width(&displayed_word(word, style), font_size, uppercase);
            let extra = if current.is_empty() { width } else { space + width };
            if current_width + extra <= limit {
                current.push(word);
                current_width += extra;
                continue;
            }
            if !current.is_empty() {
                flush_line(std::mem::take(&mut current), &mut lines, &mut groups, max_lines);
                current_width = 0.0;
            }
            // A single word longer than the safe area cannot be broken; put
            // it on its own line rather than silently merging it with others.
            if width <= limit {
                current.push(word);
                current_width = width;
            } else {
                flush_line(vec![word], &mut lines, &mut groups, max_lines);
            }
        }
        if !current.is_empty() {
            flush_line(current, &mut lines, &mut groups, max_lines);
        }
        if !lines.is_empty() {
            groups.push(lines);
        }

        for group in groups {
            let start = group.first().and_then(|line| line.first()).map(|word| word.start);
            let end = group.last().and_then(|line| line.last()).map(|word| word.end);
            layouts.push(LayoutPage {
                start: start.unwrap_or(page.start),
                end: end.unwrap_or(page.end),
                lines: group,
            });
        }
    }
    layouts
}

fn flush_line<'a>(
    line: Vec<&'a Word>,
    lines: &mut Vec<Vec<&'a Word>>,
    groups: &mut Vec<Vec<Vec<&'a Word>>>,
    max_lines: usize,
) {
    if lines.len() >= max_lines {
        groups.push(std::mem::take(lines));
    }
    lines.push(line);
}

/// Builds an ASS document from a word stream using `options`.
pub fn render(words: &[Word], options: &KaraokeOptions) -> Result<AssDocument> {
    let style = preset_style(options.preset);
    let pages = group_pages(words, &options.page);
    let layouts = layout_pages(&pages, options, &style);
    let emphasis: HashSet<String> =
        options.emphasis.iter().filter_map(|word| normalise_word(word)).collect();

    let mut events = Vec::with_capacity(layouts.len());
    for layout in &layouts {
        let text = render_page(layout, options, &style, &emphasis)?;
        events.push(AssEvent {
            kind: "Dialogue".to_owned(),
            values: vec![
                "0".to_owned(),
                layout.start.format_ass(),
                layout.end.format_ass(),
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

fn displayed_word(word: &Word, style: &PresetStyle) -> String {
    if style.uppercase {
        escape_ass_text(&word.text.to_uppercase())
    } else {
        escape_ass_text(&word.text)
    }
}

fn render_page(
    page: &LayoutPage<'_>,
    options: &KaraokeOptions,
    style: &PresetStyle,
    emphasis: &HashSet<String>,
) -> Result<String> {
    let anchor = page_anchor(options);
    let mut text = format!("{{\\pos({},{})}}", anchor.0, anchor.1);
    for (line_index, line) in page.lines.iter().enumerate() {
        if line_index != 0 {
            text.push_str("\\N");
        }
        let last = line.len() - 1;
        for (index, word) in line.iter().enumerate() {
            let highlighted =
                normalise_word(&word.text).is_some_and(|text| emphasis.contains(&text));
            text.push_str(&render_word(word, page.start, options, style, highlighted)?);
            if index != last {
                text.push(' ');
            }
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
    let displayed = displayed_word(word, style);

    // ASS transform times are MILLISECONDS relative to the Dialogue start.
    // The old code wrote centiseconds here, so every highlight fired within
    // the first ~0.1 s and lasted ~8 ms instead of the whole word.
    let start = millis_since(page_start, word.start);
    let end = millis_since(page_start, word.end);
    // `\k`/`\kf` are the exception: libass defines them in centiseconds.
    let karaoke_duration = centiseconds_between(word.start, word.end);

    let active = if highlighted { options.emphasis_color } else { options.accent };
    let primary = options.primary.format_ass();
    let primary_alpha = format!("{:02X}", options.primary.alpha);

    // One-millisecond ramps avoid a zero-width transform while still reading
    // as an instant colour change. The highlight is held until `end`.
    let on = start + 1;
    let off = end + 1;
    let up = start + POP_MS;
    let down = up + POP_MS;

    // ASS override tags are cumulative: a tag in a word's `{...}` block styles
    // that word *and every later word* in the line. Each word must therefore
    // reset to the base style before applying its own timed highlight, or one
    // active word paints the whole line in the accent colour.
    let reset_colour = format!("\\c{primary}");
    let reset_scale = "\\fscx100\\fscy100";

    let tags = match options.preset {
        KaraokePreset::Hormozi => {
            let scale = if highlighted { 125 } else { 115 };
            format!(
                "{reset_colour}{reset_scale}\
                 \\t({start},{on},\\c{})\
                 \\t({start},{up},\\fscx{scale}\\fscy{scale})\
                 \\t({up},{down},\\fscx100\\fscy100)\
                 \\t({end},{off},\\c{primary})",
                active.format_ass()
            )
        }
        KaraokePreset::Clean => {
            let colour_on =
                if highlighted { format!("\\c{}", active.format_ass()) } else { String::new() };
            let colour_off = if highlighted { format!("\\c{primary}") } else { String::new() };
            format!(
                "{reset_colour}\\alpha&H{primary_alpha}&\
                 \\t({start},{on},{colour_on}\\alpha&H00&)\
                 \\t({end},{off},{colour_off}\\alpha&H{primary_alpha}&)"
            )
        }
        KaraokePreset::Karaoke => {
            let fill = if highlighted {
                format!("\\kf{karaoke_duration}")
            } else {
                format!("\\k{karaoke_duration}")
            };
            let scale = if highlighted { 112 } else { 108 };
            format!(
                "{fill}{reset_scale}\
                 \\t({start},{up},\\fscx{scale}\\fscy{scale})\
                 \\t({up},{down},\\fscx100\\fscy100)"
            )
        }
        KaraokePreset::Bounce => {
            let settle = down + POP_MS;
            let scale = if highlighted { 145 } else { 135 };
            format!(
                "{reset_colour}{reset_scale}\
                 \\t({start},{on},\\c{})\
                 \\t({start},{up},\\fscx{scale}\\fscy{scale})\
                 \\t({up},{down},\\fscx88\\fscy88)\
                 \\t({down},{settle},\\fscx100\\fscy100)\
                 \\t({end},{off},\\c{primary})",
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

/// Milliseconds from the start of a Dialogue to `time`; used by `\t`.
fn millis_since(page_start: Time, time: Time) -> i64 {
    let scaled = (time.as_rational() - page_start.as_rational()) * Rational::from_int(1_000);
    scaled.round().max(0)
}

/// Word duration in centiseconds; used exclusively by `\k` and `\kf`.
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

    #[test]
    fn transform_times_are_milliseconds_while_k_is_centiseconds() {
        let page_start = Time::from_millis(1_520);
        let word = Word {
            text: "palavra".to_owned(),
            start: Time::from_millis(1_900),
            end: Time::from_millis(2_240),
            score: None,
        };
        assert_eq!(millis_since(page_start, word.start), 380);
        assert_eq!(millis_since(page_start, word.end), 720);
        assert_eq!(centiseconds_between(word.start, word.end), 34);
    }

    #[test]
    fn width_estimate_grows_with_size_and_uppercase() {
        let lower = estimate_text_width(" mostrar", 92, false);
        let upper = estimate_text_width(" MOSTRAR", 92, true);
        assert!(upper > lower, "{upper} must exceed {lower}");
        let single = estimate_text_width("MOSTRAR", 92, true);
        assert!((estimate_text_width("MOSTRAR", 184, true) - 2.0 * single).abs() < 1e-9);
        // The estimate is additive, which is what line packing relies on.
        let joined = estimate_text_width("MOSTRAR O SEGREDO", 92, true);
        let parts = estimate_text_width("MOSTRAR", 92, true)
            + estimate_text_width(" O ", 92, true)
            + estimate_text_width("SEGREDO", 92, true);
        assert!((joined - parts).abs() < 1e-9, "{joined} != {parts}");
    }

    #[test]
    fn available_width_subtracts_both_safe_margins() {
        let options = KaraokeOptions::new(KaraokePreset::Hormozi);
        assert_eq!(available_width(&options), 920.0);
    }
}
