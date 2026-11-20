//! Timing and text operations shared by the library and the CLI.
//!
//! All timing math is performed on exact [`Rational`] values. Framerate
//! conversion multiplies every timestamp by `source_fps / target_fps`, so a
//! cue placed on a frame boundary in one rate lands on the same frame in the
//! other rate without accumulating floating-point error.

use std::fmt;

use crate::error::{invalid_time, Error, Result};
use crate::model::{Format, PlainCue, Subtitle};
use crate::time::{Rational, Time};

/// A frame rate expressed as an exact ratio of frames per second.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FrameRate {
    num: u64,
    den: u64,
}

impl FrameRate {
    /// 23.976 fps (24000/1001, NTSC film).
    pub const FPS_23_976: Self = Self { num: 24_000, den: 1_001 };
    pub const FPS_24: Self = Self { num: 24, den: 1 };
    pub const FPS_25: Self = Self { num: 25, den: 1 };
    /// 29.97 fps (30000/1001, NTSC video).
    pub const FPS_29_97: Self = Self { num: 30_000, den: 1_001 };
    pub const FPS_30: Self = Self { num: 30, den: 1 };
    pub const FPS_50: Self = Self { num: 50, den: 1 };
    pub const FPS_60: Self = Self { num: 60, den: 1 };

    /// Parses a common name (`23.976`, `pal`, `ntsc`, `25`, ...), a ratio
    /// (`24000/1001`) or a plain decimal/decimal-with-`fps` value.
    pub fn parse(input: &str) -> Result<Self> {
        let key = input.trim().to_ascii_lowercase().replace(' ', "");
        match key.as_str() {
            "23.976" | "23.976fps" | "24000/1001" | "ntsc" | "ntscfilm" => Ok(Self::FPS_23_976),
            "24" | "24fps" => Ok(Self::FPS_24),
            "25" | "25fps" | "pal" => Ok(Self::FPS_25),
            "29.97" | "29.97fps" | "30000/1001" => Ok(Self::FPS_29_97),
            "30" | "30fps" => Ok(Self::FPS_30),
            "50" | "50fps" => Ok(Self::FPS_50),
            "60" | "60fps" => Ok(Self::FPS_60),
            _ => {
                let body = key.strip_suffix("fps").unwrap_or(&key);
                if let Some((num, den)) = body.split_once('/') {
                    match (num.parse::<u64>(), den.parse::<u64>()) {
                        (Ok(num), Ok(den)) if num > 0 && den > 0 => Ok(Self { num, den }),
                        _ => Err(Error::InvalidFrameRate(input.to_owned())),
                    }
                } else {
                    parse_decimal_ratio(body)
                        .filter(|ratio| *ratio > Rational::ZERO)
                        .map(|ratio| Self { num: ratio.num() as u64, den: ratio.den() })
                        .ok_or_else(|| Error::InvalidFrameRate(input.to_owned()))
                }
            }
        }
    }

    /// The frame rate as an exact rational number of frames per second.
    pub fn as_rational(self) -> Rational {
        Rational::new(self.num as i64, self.den).expect("frame rate denominator is non-zero")
    }

    pub const fn numerator(self) -> u64 {
        self.num
    }

    pub const fn denominator(self) -> u64 {
        self.den
    }

    /// Canonical, human-readable name of the frame rate.
    pub fn name(self) -> String {
        self.to_string()
    }

    /// Exact duration of `frames` frames at this frame rate.
    pub fn frame_duration(self, frames: u32) -> Result<Time> {
        let num =
            i64::from(frames).checked_mul(self.den as i64).ok_or(Error::ArithmeticOverflow)?;
        Ok(Time::from_rational_seconds(Rational::new(num, self.num)?))
    }
}

impl fmt::Display for FrameRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.num, self.den) {
            (24_000, 1_001) => f.write_str("23.976"),
            (30_000, 1_001) => f.write_str("29.97"),
            (num, 1) => write!(f, "{num}"),
            (num, den) => write!(f, "{num}/{den}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Exact time helpers
// ---------------------------------------------------------------------------

fn add(a: Time, b: Rational) -> Result<Time> {
    Ok(Time::from_rational_seconds(a.as_rational().checked_add(b)?))
}

fn mul(a: Time, b: Rational) -> Result<Time> {
    Ok(Time::from_rational_seconds(a.as_rational().checked_mul(b)?))
}

fn time_difference(a: Time, b: Time) -> Result<Rational> {
    a.as_rational().checked_sub(b.as_rational())
}

/// Records a recoverable timing adjustment both in the operation's return
/// value and in the thread-local diagnostic buffer used by the CLI.
fn record_warning(warnings: &mut Vec<String>, message: String) {
    crate::warnings::warn(&message);
    warnings.push(message);
}

fn map_times(cues: &mut [PlainCue], f: impl Fn(Time) -> Result<Time>) -> Result<()> {
    for cue in cues.iter_mut() {
        let start = f(cue.start)?;
        let end = f(cue.end)?;
        cue.start = start;
        cue.end = end;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Timing operations
// ---------------------------------------------------------------------------

/// Adds `delta` to every cue timestamp. A negative delta shifts earlier.
///
/// The timeline starts at zero: a cue that ends at or before zero is dropped
/// and one that straddles zero has its start clamped. Every such change is
/// reported in the returned warnings so the CLI can surface it.
pub fn shift(cues: &mut Vec<PlainCue>, delta: Time) -> Result<Vec<String>> {
    let offset = delta.as_rational();
    let mut warnings = Vec::new();
    let mut kept = Vec::with_capacity(cues.len());
    for cue in cues.drain(..) {
        let start = cue.start.as_rational().checked_add(offset)?;
        let end = cue.end.as_rational().checked_add(offset)?;
        // Strictly before zero: a cue ending exactly at zero is kept and
        // clamped to a minimal visible duration below.
        if end < Rational::ZERO {
            record_warning(
                &mut warnings,
                format!("cue {:?} ends before 00:00:00,000 after shift and was dropped", cue.text),
            );
            continue;
        }
        let (start, end) = if start < Rational::ZERO {
            record_warning(
                &mut warnings,
                format!(
                    "cue {:?} started before 00:00:00,000 after shift and was clamped to zero",
                    cue.text
                ),
            );
            // An end exactly at zero has no visible duration; keep a
            // one-second minimum for the clamped edge.
            let end =
                if end <= Rational::ZERO { Time::from_millis(1_000).as_rational() } else { end };
            (Rational::ZERO, end)
        } else {
            (start, end)
        };
        kept.push(PlainCue {
            start: Time::from_rational_seconds(start),
            end: Time::from_rational_seconds(end),
            text: cue.text,
        });
    }
    *cues = kept;
    Ok(warnings)
}

/// Converts timing from `source` to `target` frame rate: `t' = t * source/target`.
pub fn scale(cues: &mut [PlainCue], source: FrameRate, target: FrameRate) -> Result<()> {
    let factor = source.as_rational().checked_div(target.as_rational())?;
    map_times(cues, |time| mul(time, factor))
}

/// Affine two-point sync. `(from_a, to_a)` and `(from_b, to_b)` are reference
/// pairs; all cues are mapped by the unique line through both points.
pub fn sync(
    cues: &mut Vec<PlainCue>,
    from_a: Time,
    to_a: Time,
    from_b: Time,
    to_b: Time,
) -> Result<Vec<String>> {
    let span = time_difference(from_b, from_a)?;
    if span == Rational::ZERO {
        return Err(Error::DegenerateSync);
    }
    let slope = time_difference(to_b, to_a)?.checked_div(span)?;
    let intercept = to_a.as_rational().checked_sub(slope.checked_mul(from_a.as_rational())?)?;
    let mapped: Vec<PlainCue> = cues
        .iter()
        .map(|cue| {
            Ok(PlainCue {
                start: Time::from_rational_seconds(
                    slope.checked_mul(cue.start.as_rational())?.checked_add(intercept)?,
                ),
                end: Time::from_rational_seconds(
                    slope.checked_mul(cue.end.as_rational())?.checked_add(intercept)?,
                ),
                text: cue.text.clone(),
            })
        })
        .collect::<Result<_>>()?;
    *cues = mapped;
    // `sync` is a pure affine map: reference points must land exactly where
    // requested, including when the target timeline runs backwards. Negative
    // results are clamped by the shared formatter at write time, so no writer
    // can emit an unparseable negative timestamp.
    Ok(Vec::new())
}

/// Keeps only the part of each cue inside `[lower, upper]`; cues entirely
/// outside the range are discarded.
pub fn clamp(cues: &mut Vec<PlainCue>, lower: Time, upper: Time) -> Result<()> {
    if lower > upper {
        return Err(Error::Parse {
            format: "ops",
            message: "clamp lower bound must not exceed the upper bound".to_owned(),
        });
    }
    cues.retain(|cue| cue.end > lower && cue.start < upper);
    for cue in cues.iter_mut() {
        if cue.start < lower {
            cue.start = lower;
        }
        if cue.end > upper {
            cue.end = upper;
        }
    }
    Ok(())
}

/// Sorts cues by start time and pushes overlapping cues forward so that at
/// least `min_gap` separates consecutive events. Cue durations are preserved.
pub fn sort_and_fix_gaps(cues: &mut [PlainCue], min_gap: Time) -> Result<()> {
    cues.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.end.cmp(&b.end)));
    let gap = min_gap.as_rational();
    for cue in cues.iter_mut() {
        if cue.end <= cue.start {
            cue.end = add(cue.start, gap)?;
        }
    }
    for index in 1..cues.len() {
        let required_start = add(cues[index - 1].end, gap)?;
        if cues[index].start < required_start {
            let offset = time_difference(required_start, cues[index].start)?;
            cues[index].start = required_start;
            cues[index].end = add(cues[index].end, offset)?;
        }
    }
    Ok(())
}

/// Concatenates subtitle documents in chronological order. The result uses
/// the format of the first input and is re-indexed by the writer.
pub fn merge(documents: &[Subtitle]) -> Result<Subtitle> {
    let first = documents.first().ok_or(Error::EmptyMerge)?;
    let format = first.format();
    let mut cues = Vec::new();
    for document in documents {
        cues.extend(document.plain_cues()?);
    }
    cues.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.end.cmp(&b.end)));
    Subtitle::from_plain(format, &cues)
}

/// Splits a document into cues starting strictly before `at` and the rest.
pub fn split_at_time(document: &Subtitle, at: Time) -> Result<(Subtitle, Subtitle)> {
    let cues = document.plain_cues()?;
    let index = cues.partition_point(|cue| cue.start < at);
    split_at_index(document, index)
}

/// Splits a document after `index` cues.
pub fn split_at_index(document: &Subtitle, index: usize) -> Result<(Subtitle, Subtitle)> {
    let cues = document.plain_cues()?;
    if index > cues.len() {
        return Err(Error::SplitOutOfRange { index, count: cues.len() });
    }
    let format = document.format();
    let (first, second) = cues.split_at(index);
    Ok((Subtitle::from_plain(format, first)?, Subtitle::from_plain(format, second)?))
}

// ---------------------------------------------------------------------------
// Text helpers
// ---------------------------------------------------------------------------

/// Removes inline markup and converts line breaks to spaces, leaving only
/// the text a viewer actually reads. Used for CPS and line-length checks.
pub fn visible_text(text: &str) -> String {
    let mut output = String::new();
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '<' => {
                for next in characters.by_ref() {
                    if next == '>' {
                        break;
                    }
                }
            }
            '{' => {
                for next in characters.by_ref() {
                    if next == '}' {
                        break;
                    }
                }
            }
            '\\' => match characters.peek().copied() {
                Some('N' | 'n' | 'h') => {
                    characters.next();
                    output.push(' ');
                }
                _ => output.push(character),
            },
            '\n' | '\r' => output.push(' '),
            other => output.push(other),
        }
    }
    output
}

/// Number of visible characters, counting spaces but not markup or line breaks.
pub fn visible_char_count(text: &str) -> usize {
    visible_text(text).chars().count()
}

/// Splits text into whitespace-delimited visible words, ignoring markup.
pub fn word_tokens(text: &str) -> Vec<String> {
    visible_text(text).split_whitespace().map(str::to_owned).collect()
}

fn normalize_spaces(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Removes hearing-impaired annotations: `[...]` sound descriptions,
/// `(...)` cues and `♪...♪` song markers, including when inline.
pub fn strip_hearing_impaired_text(text: &str) -> String {
    let mut lines = Vec::new();
    for line in text.lines() {
        let mut cleaned = String::new();
        let mut closing: Option<char> = None;
        for character in line.chars() {
            if let Some(expected) = closing {
                if character == expected {
                    closing = None;
                }
                continue;
            }
            match character {
                '[' => closing = Some(']'),
                '(' => closing = Some(')'),
                '♪' => closing = Some('♪'),
                other => cleaned.push(other),
            }
        }
        let normalized = normalize_spaces(&cleaned);
        if !normalized.is_empty() {
            lines.push(normalized);
        }
    }
    lines.join("\n")
}

/// Removes leading speaker labels such as `JOÃO:`, `- MARIA:` or ``NARRADOR>``.
/// A leading hyphen (the Netflix dual-speaker marker) is kept when a label
/// follows it.
pub fn strip_speaker_labels_text(text: &str) -> String {
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let (prefix, rest) = match trimmed.strip_prefix('-') {
            Some(rest) => ("- ", rest.trim_start()),
            None => ("", trimmed),
        };
        let stripped = strip_one_label(rest);
        if !stripped.is_empty() {
            lines.push(format!("{prefix}{stripped}"));
        }
    }
    lines.join("\n")
}

fn strip_one_label(line: &str) -> String {
    // A bracketed prefix is a speaker label only when explicitly followed by
    // a colon. `[MÚSICA] Olá` is a sound cue and belongs to the HI cleaner.
    if let Some(after_open) = line.strip_prefix('[') {
        if let Some(end) = after_open.find(']') {
            if let Some(remainder) = after_open[end + 1..].trim_start().strip_prefix(':') {
                if is_speaker_name(&after_open[..end]) {
                    return normalize_spaces(remainder.trim_start());
                }
            }
        }
    }
    if let Some(position) = line.find([':', '>']) {
        if is_speaker_name(&line[..position]) {
            return normalize_spaces(&line[position + 1..]);
        }
    }
    normalize_spaces(line)
}

fn is_speaker_name(candidate: &str) -> bool {
    let candidate = candidate.trim();
    if candidate.is_empty() || candidate.chars().count() > 40 {
        return false;
    }
    let mut has_uppercase = false;
    for character in candidate.chars() {
        if character.is_uppercase() {
            has_uppercase = true;
        } else if !(character.is_numeric()
            || character.is_whitespace()
            || matches!(character, '.' | '-' | '\'' | '_' | '&' | '/' | ','))
        {
            // Lowercase letters and punctuation other than name separators
            // cannot be part of an all-caps speaker label.
            return false;
        }
    }
    has_uppercase
}

/// Removes hearing-impaired annotations from every cue, dropping cues left
/// with no text.
pub fn strip_hearing_impaired(cues: &mut Vec<PlainCue>) {
    for cue in cues.iter_mut() {
        cue.text = strip_hearing_impaired_text(&cue.text);
    }
    cues.retain(|cue| !cue.text.trim().is_empty());
}

/// Removes speaker labels from every cue, dropping cues left with no text.
pub fn strip_speaker_labels(cues: &mut Vec<PlainCue>) {
    for cue in cues.iter_mut() {
        cue.text = strip_speaker_labels_text(&cue.text);
    }
    cues.retain(|cue| !cue.text.trim().is_empty());
}

// ---------------------------------------------------------------------------
// Document helpers
// ---------------------------------------------------------------------------

/// Writes a modified cue list back to a document. When the cue count is
/// unchanged, the format-specific structure (SRT indices, WebVTT raw blocks,
/// ASS styles) is preserved; otherwise a new canonical document is built.
pub fn with_cues(source: &Subtitle, cues: &[PlainCue]) -> Result<Subtitle> {
    if source.plain_cues()?.len() == cues.len() {
        let mut result = source.clone();
        result.replace_plain_cues(cues)?;
        Ok(result)
    } else {
        Subtitle::from_plain(source.format(), cues)
    }
}

/// Parses a CLI time expression: an SRT/WebVTT timestamp, a number of seconds
/// (`1.5`, `1.5s`), or a number of milliseconds (`1500ms`), with optional sign.
pub fn parse_time(input: &str) -> Result<Time> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(invalid_time("expression", input, "empty time expression"));
    }
    if let Ok(time) = Time::parse_srt(trimmed) {
        return Ok(time);
    }
    let (negative, body) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let (body, unit) = if let Some(rest) = body.strip_suffix("ms") {
        (rest, Rational::new(1, 1_000)?)
    } else if let Some(rest) = body.strip_suffix('s') {
        (rest, Rational::ONE)
    } else {
        (body, Rational::ONE)
    };
    let value = parse_decimal_ratio(body)
        .ok_or_else(|| invalid_time("expression", input, "expected a timestamp or decimal time"))?
        .checked_mul(unit)?;
    let value = if negative { value.checked_mul(Rational::new(-1, 1)?)? } else { value };
    Ok(Time::from_rational_seconds(value))
}

/// Parses a decimal string into an exact ratio (e.g. `23.976` -> `2997/125`).
fn parse_decimal_ratio(input: &str) -> Option<Rational> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    let (sign, body) = match input.strip_prefix('-') {
        Some(rest) => (-1i128, rest),
        None => match input.strip_prefix('+') {
            Some(rest) => (1, rest),
            None => (1, input),
        },
    };
    let (whole, fraction) = match body.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (body, None),
    };
    if whole.is_empty() || !whole.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let whole = whole.parse::<i128>().ok()?;
    match fraction {
        None => {
            let numerator = i64::try_from(whole.checked_mul(sign)?).ok()?;
            Rational::new(numerator, 1).ok()
        }
        Some(fraction) => {
            if fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            let denominator = 10i128.checked_pow(fraction.len() as u32)?;
            let fractional = fraction.parse::<i128>().ok()?;
            let numerator = whole.checked_mul(denominator)?.checked_add(fractional)?;
            let numerator = i64::try_from(numerator.checked_mul(sign)?).ok()?;
            Rational::new(numerator, u64::try_from(denominator).ok()?).ok()
        }
    }
}

/// Infers the output format strictly from the file extension. An unknown
/// extension is an error: silently writing SRT hides typos such as `out.xyz`.
/// Callers that intentionally want another format must pass it explicitly.
pub fn output_format_from_path(
    _source: &Format,
    path: impl AsRef<std::path::Path>,
) -> Result<Format> {
    let path = path.as_ref();
    Format::from_extension(path).ok_or_else(|| Error::CannotInferOutputFormat {
        extension: path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("<none>")
            .to_owned(),
    })
}

/// Infers a format for an output path from a subtitle document.
pub fn output_format(source: &Subtitle, path: impl AsRef<std::path::Path>) -> Result<Format> {
    output_format_from_path(&source.format(), path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(start: i64, end: i64, text: &str) -> PlainCue {
        PlainCue {
            start: Time::from_millis(start),
            end: Time::from_millis(end),
            text: text.to_owned(),
        }
    }

    #[test]
    fn frame_rates_are_exact() {
        assert_eq!(FrameRate::FPS_23_976.as_rational(), Rational::new(24_000, 1_001).unwrap());
        assert_eq!(FrameRate::FPS_29_97.frame_duration(1).unwrap().as_rational().num(), 1_001);
        assert_eq!(
            FrameRate::FPS_24.frame_duration(2).unwrap().as_rational(),
            Rational::new(1, 12).unwrap()
        );
        assert_eq!(FrameRate::parse("PAL").unwrap(), FrameRate::FPS_25);
        assert_eq!(FrameRate::parse("24000/1001").unwrap(), FrameRate::FPS_23_976);
        assert_eq!(FrameRate::parse("47.952").unwrap().numerator(), 5_994);
        assert!(FrameRate::parse("0").is_err());
    }

    #[test]
    fn parse_time_accepts_known_vectors() {
        assert_eq!(parse_time("00:00:01,500").unwrap(), Time::from_millis(1_500));
        assert_eq!(parse_time("1.5").unwrap(), Time::from_millis(1_500));
        assert_eq!(parse_time("1.5s").unwrap(), Time::from_millis(1_500));
        assert_eq!(parse_time("1500ms").unwrap(), Time::from_millis(1_500));
        assert_eq!(parse_time("-1.5s").unwrap(), Time::from_millis(-1_500));
        assert!(parse_time("not a time").is_err());
        assert!(parse_time("").is_err());
    }

    #[test]
    fn scaling_23_976_to_25_is_exact() {
        let mut cues = vec![cue(1_001, 2_002, "a")];
        scale(&mut cues, FrameRate::FPS_23_976, FrameRate::FPS_25).unwrap();
        assert_eq!(cues[0].start, Time::from_millis(960));
        assert_eq!(cues[0].end, Time::from_millis(1_920));
    }

    #[test]
    fn sync_maps_both_reference_points() {
        let mut cues = vec![cue(1_000, 2_000, "a"), cue(11_000, 12_000, "b")];
        sync(
            &mut cues,
            Time::from_millis(1_000),
            Time::from_millis(500),
            Time::from_millis(11_000),
            Time::from_millis(10_500),
        )
        .unwrap();
        assert_eq!(cues[0].start, Time::from_millis(500));
        assert_eq!(cues[1].start, Time::from_millis(10_500));
        assert!(matches!(
            sync(
                &mut cues,
                Time::from_millis(1_000),
                Time::ZERO,
                Time::from_millis(1_000),
                Time::from_millis(1_000),
            ),
            Err(Error::DegenerateSync)
        ));
    }

    #[test]
    fn clamp_keeps_only_overlapping_cues() {
        let mut cues = vec![cue(0, 1_000, "a"), cue(500, 2_500, "b"), cue(3_000, 4_000, "c")];
        clamp(&mut cues, Time::from_millis(500), Time::from_millis(2_000)).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, Time::from_millis(500));
        assert_eq!(cues[0].end, Time::from_millis(1_000));
        assert_eq!(cues[1].end, Time::from_millis(2_000));
    }

    #[test]
    fn text_cleaners_handle_known_vectors() {
        assert_eq!(strip_hearing_impaired_text("[MÚSICA] Olá, [risos] mundo"), "Olá, mundo");
        assert_eq!(strip_hearing_impaired_text("(risos)\nOlá"), "Olá");
        assert_eq!(strip_hearing_impaired_text("♪ la la ♪\nOlá"), "Olá");
        assert_eq!(strip_speaker_labels_text("JOÃO: Olá, tudo bem?"), "Olá, tudo bem?");
        assert_eq!(strip_speaker_labels_text("- MARIA: Obrigada."), "- Obrigada.");
        assert_eq!(strip_speaker_labels_text("NARRADOR> Era uma vez"), "Era uma vez");
        assert_eq!(strip_speaker_labels_text("Olá, mundo: tudo bem?"), "Olá, mundo: tudo bem?");
        assert_eq!(visible_char_count("<i>Olá</i>\nmundo"), 9);
    }

    #[test]
    fn sort_and_fix_gaps_removes_overlaps() {
        let mut cues = vec![cue(2_000, 3_000, "b"), cue(0, 2_000, "a")];
        sort_and_fix_gaps(&mut cues, Time::from_millis(100)).unwrap();
        assert_eq!(cues[0].start, Time::ZERO);
        assert_eq!(cues[1].start, Time::from_millis(2_100));
        assert_eq!(cues[1].end, Time::from_millis(3_100));
    }
}
