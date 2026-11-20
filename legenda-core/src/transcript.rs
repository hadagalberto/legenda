//! Word-level transcript ingestion for karaoke captions.
//!
//! Three ASR JSON shapes are supported and normalised to one [`Transcript`]:
//!
//! * **WhisperX**: `segments[].words[]` with `word`, `start`, `end`, `score`
//!   (times in seconds).
//! * **whisper.cpp** `-ojf`: `transcription[].tokens[]` with `text` and
//!   `offsets: { from, to }` (milliseconds, written as `t0 * 10` by the
//!   official `times_o()` writer in `examples/cli/cli.cpp`). Legacy
//!   `start`/`end` keys are also accepted.
//! * **OpenAI** `verbose_json`: top-level `words[]` with `word`, `start`,
//!   `end` (times in seconds), optionally accompanied by `segments[]`.
//!
//! Missing word timings are interpolated linearly within the enclosing
//! segment, weighted by visible character count. Explicit timings are kept
//! verbatim, so pauses between words survive and can drive page grouping.

use serde_json::Value;

use crate::error::{parse_error, Result};
use crate::time::{Rational, Time};

/// The ASR format a transcript was read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TranscriptFormat {
    WhisperX,
    WhisperCpp,
    OpenAiVerbose,
}

impl TranscriptFormat {
    pub const fn name(self) -> &'static str {
        match self {
            Self::WhisperX => "whisperx",
            Self::WhisperCpp => "whisper.cpp",
            Self::OpenAiVerbose => "openai-verbose-json",
        }
    }
}

/// One fully timed word. `score` is the ASR confidence when supplied.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub start: Time,
    pub end: Time,
    pub score: Option<f64>,
}

/// A word as it appears in ASR JSON, where timings may be absent.
#[derive(Clone, Debug, PartialEq)]
struct RawWord {
    text: String,
    start: Option<Time>,
    end: Option<Time>,
    score: Option<f64>,
}

/// An ASR segment; retained for provenance and as the interpolation domain.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub start: Time,
    pub end: Time,
    pub text: Option<String>,
    pub words: Vec<Word>,
}

/// A normalised transcript. `words` is the flattened, time-ordered word list
/// used by karaoke rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct Transcript {
    pub format: TranscriptFormat,
    pub segments: Vec<Segment>,
    pub words: Vec<Word>,
}

/// Parses transcript JSON, detecting the ASR format from its shape.
pub fn parse_transcript(input: &str) -> Result<Transcript> {
    let value: Value = serde_json::from_str(input)?;
    parse_value(&value)
}

/// Parses an already-decoded JSON value.
pub fn parse_value(value: &Value) -> Result<Transcript> {
    let detected = detect_format(value)
        .ok_or_else(|| parse_error("transcript", "unrecognised ASR JSON shape"))?;
    let segments = match detected {
        TranscriptFormat::WhisperX => parse_whisperx(value)?,
        TranscriptFormat::WhisperCpp => parse_whisper_cpp(value)?,
        TranscriptFormat::OpenAiVerbose => parse_openai(value)?,
    };
    let words = segments.iter().flat_map(|segment| segment.words.iter().cloned()).collect();
    Ok(Transcript { format: detected, segments, words })
}

/// Detects the producer from the top-level JSON shape.
pub fn detect_format(value: &Value) -> Option<TranscriptFormat> {
    if value.get("words").is_some_and(Value::is_array) {
        Some(TranscriptFormat::OpenAiVerbose)
    } else if value.get("transcription").is_some_and(Value::is_array) {
        Some(TranscriptFormat::WhisperCpp)
    } else if value.get("segments").is_some_and(Value::is_array) {
        Some(TranscriptFormat::WhisperX)
    } else {
        None
    }
}

fn parse_whisperx(value: &Value) -> Result<Vec<Segment>> {
    let segments = value["segments"]
        .as_array()
        .ok_or_else(|| parse_error("transcript", "WhisperX JSON has no segments array"))?;
    let mut result = Vec::with_capacity(segments.len());
    for segment in segments {
        let raw_words = segment
            .get("words")
            .and_then(Value::as_array)
            .ok_or_else(|| parse_error("transcript", "WhisperX segment has no words array"))?;
        let raw = raw_words
            .iter()
            .filter_map(|word| {
                let text = clean_word(word.get("word").and_then(Value::as_str));
                (!text.is_empty()).then(|| RawWord {
                    text,
                    start: seconds_value(word.get("start")),
                    end: seconds_value(word.get("end")),
                    score: word
                        .get("score")
                        .and_then(Value::as_f64)
                        .filter(|score| score.is_finite()),
                })
            })
            .collect::<Vec<_>>();
        if raw.is_empty() {
            continue;
        }
        let (start, end) = segment_bounds(segment, &raw)?;
        let words = interpolate_word_timings(&raw, start, end)?;
        result.push(Segment {
            start,
            end,
            text: segment.get("text").and_then(Value::as_str).map(ToOwned::to_owned),
            words,
        });
    }
    if result.is_empty() {
        return Err(parse_error("transcript", "WhisperX JSON has no words"));
    }
    Ok(result)
}

fn parse_whisper_cpp(value: &Value) -> Result<Vec<Segment>> {
    let entries = value["transcription"]
        .as_array()
        .ok_or_else(|| parse_error("transcript", "whisper.cpp JSON has no transcription array"))?;
    let mut result = Vec::with_capacity(entries.len());
    for entry in entries {
        let offsets = entry.get("offsets");
        let start = offset_value(offsets, "from", "start");
        let end = offset_value(offsets, "to", "end");
        let raw: Vec<RawWord> = if let Some(tokens) = entry.get("tokens").and_then(Value::as_array)
        {
            tokens
                .iter()
                .filter_map(|token| {
                    let text = clean_word(token.get("text").and_then(Value::as_str));
                    // The full JSON also contains control tokens such as
                    // `<|endoftext|>` and `<|startoftranscript|>`; they carry
                    // no timings and are not words a viewer should see.
                    if text.is_empty() || is_special_token(&text) {
                        return None;
                    }
                    let token_offsets = token.get("offsets");
                    Some(RawWord {
                        text,
                        start: offset_value(token_offsets, "from", "start"),
                        end: offset_value(token_offsets, "to", "end"),
                        score: token
                            .get("p")
                            .and_then(Value::as_f64)
                            .filter(|probability| probability.is_finite()),
                    })
                })
                .collect()
        } else {
            entry
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .split_whitespace()
                .map(|text| RawWord {
                    text: clean_word(Some(text)),
                    start: None,
                    end: None,
                    score: None,
                })
                .collect()
        };
        if raw.is_empty() {
            continue;
        }
        let fallback_start = raw.iter().find_map(|word| word.start);
        let fallback_end = raw.iter().rev().find_map(|word| word.end);
        let start = start.or(fallback_start).ok_or_else(|| {
            parse_error("transcript", "whisper.cpp segment has no start time and no timed token")
        })?;
        let end = end.or(fallback_end).ok_or_else(|| {
            parse_error("transcript", "whisper.cpp segment has no end time and no timed token")
        })?;
        let words = interpolate_word_timings(&raw, start, end)?;
        result.push(Segment {
            start,
            end,
            text: entry.get("text").and_then(Value::as_str).map(ToOwned::to_owned),
            words,
        });
    }
    if result.is_empty() {
        return Err(parse_error("transcript", "whisper.cpp JSON has no words"));
    }
    Ok(result)
}

fn parse_openai(value: &Value) -> Result<Vec<Segment>> {
    let raw_words = value["words"]
        .as_array()
        .ok_or_else(|| parse_error("transcript", "verbose_json has no words array"))?;
    let raw: Vec<RawWord> = raw_words
        .iter()
        .filter_map(|word| {
            let text = clean_word(word.get("word").and_then(Value::as_str));
            if text.is_empty() {
                return None;
            }
            let start = seconds_value(word.get("start"));
            let mut end = seconds_value(word.get("end"));
            if let (Some(start), Some(explicit_end)) = (start, end) {
                if explicit_end <= start {
                    // verbose_json occasionally emits a zero-length final
                    // word; give it a 10 ms duration so \k stays well formed.
                    end = Some(Time::from_rational_seconds(
                        start.as_rational() + Rational::new(1, 100).ok()?,
                    ));
                }
            }
            Some(RawWord { text, start, end, score: None })
        })
        .collect();
    if raw.is_empty() {
        return Err(parse_error("transcript", "verbose_json has no words"));
    }

    let bounds: Vec<(Time, Time)> = value["segments"]
        .as_array()
        .map(|segments| {
            segments
                .iter()
                .filter_map(|segment| {
                    Some((seconds_value(segment.get("start"))?, seconds_value(segment.get("end"))?))
                })
                .collect()
        })
        .unwrap_or_default();

    if bounds.is_empty() {
        let start = raw
            .iter()
            .find_map(|word| word.start)
            .ok_or_else(|| parse_error("transcript", "verbose_json words have no timings"))?;
        let end = raw.iter().rev().find_map(|word| word.end).unwrap_or(start);
        let words = interpolate_word_timings(&raw, start, end)?;
        return Ok(vec![Segment { start, end, text: None, words }]);
    }

    // Assign every word to exactly one segment. Untimed words follow the end
    // of the previous timed word, so they are never duplicated across
    // segments (or dropped when they fall outside the declared bounds).
    let mut assignments = Vec::with_capacity(raw.len());
    let mut cursor = bounds.first().map(|bound| bound.0);
    for word in &raw {
        let position = word.start.or(cursor).unwrap_or(Time::ZERO);
        let index = bounds
            .iter()
            .position(|(start, end)| position >= *start && position < *end)
            .unwrap_or(bounds.len() - 1);
        assignments.push(index);
        if let Some(end) = word.end {
            cursor = Some(end);
        }
    }

    let mut segments = Vec::with_capacity(bounds.len());
    for (index, (start, end)) in bounds.into_iter().enumerate() {
        let selected: Vec<RawWord> = raw
            .iter()
            .zip(&assignments)
            .filter(|(_, assigned)| **assigned == index)
            .map(|(word, _)| word.clone())
            .collect();
        if selected.is_empty() {
            continue;
        }
        let words = interpolate_word_timings(&selected, start, end)?;
        segments.push(Segment { start, end, text: None, words });
    }
    if segments.is_empty() {
        return Err(parse_error("transcript", "verbose_json segments do not cover any words"));
    }
    Ok(segments)
}

fn segment_bounds(segment: &Value, raw: &[RawWord]) -> Result<(Time, Time)> {
    let fallback_start = raw.iter().find_map(|word| word.start);
    let fallback_end = raw.iter().rev().find_map(|word| word.end);
    let start = seconds_value(segment.get("start"))
        .or(fallback_start)
        .ok_or_else(|| parse_error("transcript", "segment has no start time and no timed word"))?;
    let end = seconds_value(segment.get("end"))
        .or(fallback_end)
        .ok_or_else(|| parse_error("transcript", "segment has no end time and no timed word"))?;
    Ok((start, end))
}

/// Fills unknown word timings by distributing time between explicit anchors.
///
/// Words with a known start or end keep it exactly. An unknown start is set to
/// the previous word's end (or the segment start); an unknown end is the
/// start plus a length proportional to the word's character count, capped at
/// the next known start and at the segment end for the last word.
fn interpolate_word_timings(
    raw: &[RawWord],
    segment_start: Time,
    segment_end: Time,
) -> Result<Vec<Word>> {
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    let start = segment_start.as_rational();
    let end = segment_end.as_rational();
    if end <= start {
        return Err(parse_error("transcript", "segment end must be after segment start"));
    }
    let weights: Vec<Rational> = raw
        .iter()
        .map(|word| Rational::from_int(word.text.chars().count().max(1) as i64))
        .collect();
    let total_weight: Rational =
        weights.iter().copied().fold(Rational::ZERO, |sum, weight| sum + weight);
    let rate = (end - start) / total_weight;

    let mut words = Vec::with_capacity(raw.len());
    let mut previous_end = start;
    for (index, source) in raw.iter().enumerate() {
        let duration = weights[index] * rate;
        let word_start = source.start.map(Time::as_rational).unwrap_or(previous_end);
        let mut word_end = source.end.map(Time::as_rational).unwrap_or(word_start + duration);
        if word_end <= word_start {
            word_end = word_start + duration;
        }
        if let Some(next_start) =
            raw.get(index + 1).and_then(|word| word.start).map(Time::as_rational)
        {
            if word_end > next_start {
                word_end = next_start;
            }
        }
        if index + 1 == raw.len() && word_end > end {
            word_end = end;
        }
        words.push(Word {
            text: source.text.clone(),
            start: Time::from_rational_seconds(word_start),
            end: Time::from_rational_seconds(word_end),
            score: source.score,
        });
        previous_end = word_end;
    }
    Ok(words)
}

/// Rules for splitting a word stream into caption pages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageOptions {
    pub max_words: usize,
    pub max_chars: usize,
    pub pause_threshold: Time,
    pub break_on_punctuation: bool,
}

impl Default for PageOptions {
    fn default() -> Self {
        Self {
            max_words: 8,
            max_chars: 34,
            pause_threshold: Time::from_millis(300),
            break_on_punctuation: true,
        }
    }
}

/// A caption page: a group of whole words with the timing of its first and
/// last word.
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub words: Vec<Word>,
    pub start: Time,
    pub end: Time,
}

/// Groups words into pages. A word is never split: when a limit is reached the
/// page is flushed *before* the word that would exceed it.
pub fn group_pages(words: &[Word], options: &PageOptions) -> Vec<Page> {
    let max_words = options.max_words.max(1);
    let max_chars = options.max_chars.max(1);
    let mut pages = Vec::new();
    let mut current: Vec<Word> = Vec::new();
    let mut current_chars = 0usize;

    for word in words {
        let word_chars = word.text.chars().count();
        if let Some(previous) = current.last() {
            let pause = word.start.as_rational() - previous.end.as_rational();
            let pause_exceeded = pause > options.pause_threshold.as_rational();
            let punctuation_break =
                options.break_on_punctuation && ends_sentence(&previous.text, current.len());
            let size_exceeded =
                current.len() >= max_words || current_chars + 1 + word_chars > max_chars;
            if pause_exceeded || punctuation_break || size_exceeded {
                pages.push(page_from(std::mem::take(&mut current)));
                current_chars = 0;
            }
        }
        current_chars += if current.is_empty() { word_chars } else { 1 + word_chars };
        current.push(word.clone());
    }
    if !current.is_empty() {
        pages.push(page_from(current));
    }
    pages
}

fn page_from(words: Vec<Word>) -> Page {
    Page {
        start: words.first().expect("non-empty page").start,
        end: words.last().expect("non-empty page").end,
        words,
    }
}

fn ends_sentence(text: &str, current_words: usize) -> bool {
    match text.trim_end().chars().next_back() {
        Some('.' | '!' | '?' | '…' | '。' | '¿' | '¡') => true,
        Some(',' | ';' | ':' | '、') => current_words >= 2,
        _ => false,
    }
}

fn clean_word(text: Option<&str>) -> String {
    text.unwrap_or("").trim().to_owned()
}

/// Reads a time in seconds (`f64`) from JSON, quantised to microseconds.
fn seconds_value(value: Option<&Value>) -> Option<Time> {
    let number = match value? {
        Value::Number(number) => number.as_f64()?,
        Value::String(text) => text.parse::<f64>().ok()?,
        _ => return None,
    };
    if !number.is_finite() {
        return None;
    }
    let micros = (number * 1_000_000.0).round() as i64;
    Some(Time::from_rational_seconds(Rational::new(micros, 1_000_000).ok()?))
}

/// Reads a millisecond timestamp from a whisper.cpp `offsets` object.
///
/// The official writer emits `{"from": t0*10, "to": t1*10}` where t0/t1 are
/// centiseconds, i.e. integer milliseconds. `start`/`end` are accepted as a
/// legacy alias so third-party exporters keep working.
fn offset_value(offsets: Option<&Value>, name: &str, legacy: &str) -> Option<Time> {
    let offsets = offsets?;
    millis_value(offsets.get(name)).or_else(|| millis_value(offsets.get(legacy)))
}

/// True for whisper.cpp control tokens (`<|endoftext|>` and friends), which
/// are never spoken words.
fn is_special_token(text: &str) -> bool {
    (text.starts_with("<|") && text.ends_with("|>"))
        || (text.starts_with("[_") && text.ends_with("_]"))
}

/// Reads a time in milliseconds (whisper.cpp) from JSON.
fn millis_value(value: Option<&Value>) -> Option<Time> {
    let number = match value? {
        Value::Number(number) => number.as_f64()?,
        Value::String(text) => text.parse::<f64>().ok()?,
        _ => return None,
    };
    if !number.is_finite() {
        return None;
    }
    Some(Time::from_millis(number.round() as i64))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(text: &str, start: Option<Time>, end: Option<Time>) -> RawWord {
        RawWord { text: text.to_owned(), start, end, score: None }
    }

    #[test]
    fn detects_every_supported_shape() {
        assert_eq!(
            detect_format(&serde_json::json!({"segments": []})),
            Some(TranscriptFormat::WhisperX)
        );
        assert_eq!(
            detect_format(&serde_json::json!({"transcription": []})),
            Some(TranscriptFormat::WhisperCpp)
        );
        assert_eq!(
            detect_format(&serde_json::json!({"words": []})),
            Some(TranscriptFormat::OpenAiVerbose)
        );
        assert_eq!(detect_format(&serde_json::json!({"text": "hi"})), None);
    }

    #[test]
    fn interpolation_distributes_untimed_words_by_character_weight() {
        let raw = vec![raw("a", None, None), raw("bbb", None, None)];
        let words =
            interpolate_word_timings(&raw, Time::from_millis(0), Time::from_millis(800)).unwrap();
        assert_eq!(words[0].start, Time::from_millis(0));
        assert_eq!(words[0].end, Time::from_millis(200));
        assert_eq!(words[1].start, Time::from_millis(200));
        assert_eq!(words[1].end, Time::from_millis(800));
    }

    #[test]
    fn interpolation_keeps_explicit_timings_and_rejects_bad_segments() {
        let sources = vec![
            raw("a", Some(Time::from_millis(100)), Some(Time::from_millis(300))),
            raw("b", None, Some(Time::from_millis(700))),
        ];
        let words =
            interpolate_word_timings(&sources, Time::ZERO, Time::from_millis(1_000)).unwrap();
        assert_eq!(words[0].start, Time::from_millis(100));
        assert_eq!(words[0].end, Time::from_millis(300));
        assert_eq!(words[1].start, Time::from_millis(300));
        assert_eq!(words[1].end, Time::from_millis(700));
        assert!(interpolate_word_timings(
            &[raw("x", None, None)],
            Time::from_millis(5),
            Time::from_millis(5)
        )
        .is_err());
    }
}
