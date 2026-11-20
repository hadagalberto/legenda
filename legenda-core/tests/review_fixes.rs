//! Regression tests for the independent-review findings (milestone 8).
//!
//! Every expected value in this file is derived by hand from the SPEC, the
//! official ASR writer source, or simple arithmetic written out in a comment.
//! No expected value is taken from a snapshot accepted without reading it and
//! no oracle is computed by the library under test.

use legenda_core::ass;
use legenda_core::encoding::{self, Encoding};
use legenda_core::karaoke::{render, KaraokeOptions, KaraokePreset};
use legenda_core::lint::{LintConfig, LintReport, LintRule};
use legenda_core::ops::{self, visible_char_count, FrameRate};
use legenda_core::reflow::{self, Preset, ReflowOptions};
use legenda_core::time::Time;
use legenda_core::transcript::{parse_transcript, TranscriptFormat, Word};
use legenda_core::{warnings, PlainCue};

fn cue(start_ms: i64, end_ms: i64, text: &str) -> PlainCue {
    PlainCue {
        start: Time::from_millis(start_ms),
        end: Time::from_millis(end_ms),
        text: text.to_owned(),
    }
}

fn word(text: &str, start_ms: i64, end_ms: i64) -> Word {
    Word {
        text: text.to_owned(),
        start: Time::from_millis(start_ms),
        end: Time::from_millis(end_ms),
        score: None,
    }
}

fn netflix() -> ReflowOptions {
    ReflowOptions::new(Preset::NetflixPtBr, FrameRate::FPS_24).unwrap()
}

// ---------------------------------------------------------------------------
// HIGH: karaoke \k fill must cover gaps and start each word on its spoken time
// ---------------------------------------------------------------------------

/// Pulls the numeric argument of every `\kNN` tag (not `\kfNN`) out of an ASS
/// dialogue line, in text order.
fn k_values(text: &str) -> Vec<i64> {
    let bytes = text.as_bytes();
    let mut result = Vec::new();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'\\' && bytes[index + 1] == b'k' {
            let next = bytes.get(index + 2).copied();
            if next.is_some_and(|byte| byte == b'f' || byte == b'o') {
                index += 2;
                continue;
            }
            let mut end = index + 2;
            while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            if end > index + 2 {
                result.push(text[index + 2..end].parse().unwrap());
                index = end;
                continue;
            }
        }
        index += 1;
    }
    result
}

/// First timestamp (ms) of each `\t(from,to,...)` transform.
fn transform_starts(text: &str) -> Vec<i64> {
    let bytes = text.as_bytes();
    let mut result = Vec::new();
    let pattern = b"\\t(";
    let mut index = 0;
    while index + pattern.len() <= bytes.len() {
        if &bytes[index..index + pattern.len()] == pattern {
            let mut end = index + pattern.len();
            while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            result.push(text[index + pattern.len()..end].parse().unwrap());
            index = end;
            continue;
        }
        index += 1;
    }
    result
}

#[test]
fn karaoke_fill_covers_gaps_and_starts_each_word_at_its_spoken_time() {
    // Four words spoken at 1000/1450/1850/2250 ms, each ~350 ms long. The
    // line runs from the first word's start (1000 ms) to the last word's end
    // (2600 ms): 1600 ms = 160 centiseconds. ASS \k durations are cumulative,
    // so the four fills must be:
    //   word 1: 1000 -> 1450 = 45 cs   (includes the 110 ms gap after speech)
    //   word 2: 1450 -> 1850 = 40 cs
    //   word 3: 1850 -> 2250 = 40 cs
    //   word 4: 2250 -> 2600 = 35 cs
    // Sum = 160 cs. The old code emitted only spoken durations
    // (\k35 \k35 \k35 \k35 = 140 cs, with \t(450) following a \k35), so the
    // fill drifted ahead of the voice by the accumulated gaps.
    let words = vec![
        word("um", 1_000, 1_350),
        word("dois", 1_450, 1_800),
        word("tres", 1_850, 2_200),
        word("quatro", 2_250, 2_600),
    ];
    let document = render(&words, &KaraokeOptions::new(KaraokePreset::Karaoke)).unwrap();
    let text = ass::write(&document).unwrap();
    let reparsed = ass::parse(&text).unwrap();
    let dialogues = reparsed.dialogues();
    assert_eq!(dialogues.len(), 1, "four short words must form one page");
    let (format, event) = dialogues[0];
    let body = event.text(format).unwrap();

    let durations = k_values(body);
    assert_eq!(durations, [45, 40, 40, 35], "cumulative \\k fill: {body}");
    assert_eq!(durations.iter().sum::<i64>(), 160, "fill must span 1000..2600 ms");

    // The pop-in transform for word N fires at that word's spoken start, so
    // the end of fill N and the start of transform N+1 must coincide.
    let starts = transform_starts(body);
    assert!(starts.contains(&450), "word 2 transform must start at 450 ms: {body}");
    assert!(starts.contains(&850), "word 3 transform must start at 850 ms: {body}");
    assert!(starts.contains(&1_250), "word 4 transform must start at 1250 ms: {body}");
}

// ---------------------------------------------------------------------------
// HIGH: whisper.cpp -ojf uses offsets.from/to (milliseconds)
// ---------------------------------------------------------------------------

#[test]
fn whisper_cpp_real_ojf_full_json_offsets_from_to() {
    // The official writer (whisper.cpp examples/cli/cli.cpp, times_o()) emits
    // `offsets: {"from": t0*10, "to": t1*10}` with t0/t1 in centiseconds, i.e.
    // millisecond integers, and `tokens[].offsets` with the same shape. It
    // never emits `start`/`end` inside `offsets`.
    let json = include_str!("fixtures/whisper_cpp_full_ojf.json");
    let transcript = parse_transcript(json).unwrap();
    assert_eq!(transcript.format, TranscriptFormat::WhisperCpp);
    assert_eq!(transcript.segments.len(), 2);
    assert_eq!(transcript.words.len(), 4);
    assert_eq!(transcript.words[0].text, "Olá");
    assert_eq!(transcript.words[0].start, Time::from_millis(1_000));
    assert_eq!(transcript.words[0].end, Time::from_millis(1_600));
    assert_eq!(transcript.words[1].text, "mundo");
    assert_eq!(transcript.words[1].start, Time::from_millis(1_650));
    assert_eq!(transcript.words[1].end, Time::from_millis(2_500));
    assert_eq!(transcript.words[2].start, Time::from_millis(2_600));
    assert_eq!(transcript.words[3].end, Time::from_millis(3_200));
}

// ---------------------------------------------------------------------------
// HIGH: reflow must split long cues by text length across time
// ---------------------------------------------------------------------------

#[test]
fn reflow_splits_a_142_character_cue_into_linted_lines() {
    // 11 words of 12 visible characters plus 10 spaces = 11*12 + 10 = 142.
    let text = (0..11).map(|i| format!("w{i:02}abcdefghi")).collect::<Vec<_>>().join(" ");
    assert_eq!(visible_char_count(&text), 142);

    let mut cues = vec![cue(1_000, 3_000, &text)];
    let before: Vec<String> = ops::word_tokens(&text);
    reflow::reflow(&mut cues, &netflix()).unwrap();
    let after: Vec<String> = cues.iter().flat_map(|cue| ops::word_tokens(&cue.text)).collect();
    assert_eq!(before, after, "reflow must not lose or duplicate words");
    assert!(cues.len() >= 2, "142 chars cannot fit in one 2-line cue");

    let config = LintConfig::netflix_ptbr(FrameRate::FPS_24, false).unwrap();
    let report = LintReport::new(&cues, &config);
    assert!(!report.issues.iter().any(|issue| issue.rule == LintRule::LineLength));
    assert!(!report.issues.iter().any(|issue| issue.rule == LintRule::TooManyLines));
    assert!(!report.issues.iter().any(|issue| issue.rule == LintRule::TooLong));
    assert!(!report.issues.iter().any(|issue| issue.rule == LintRule::Overlap));
    assert!(!report.issues.iter().any(|issue| issue.rule == LintRule::Gap));
    for cue in &cues {
        assert!(cue.end > cue.start);
        assert!(cue.text.lines().all(|line| visible_char_count(line) <= 42));
    }
}

// ---------------------------------------------------------------------------
// HIGH (design): CPS extension may use only the gap before the next cue
// ---------------------------------------------------------------------------

#[test]
fn cps_extension_never_moves_a_later_cue_start() {
    // The third cue needs far more than 100 ms to be readable. The old code
    // extended it and shifted every following cue forward by the same delta,
    // moving "Quarto" from 10 s to ~19 s. With the gap-limited policy the
    // fourth cue must still start at exactly 10 s; any unmet CPS requirement
    // is left for `lint` to report instead.
    let mut cues = vec![
        cue(0, 1_000, "Curto"),
        cue(2_000, 2_100, "Este texto e realmente bastante longo"),
        cue(4_000, 4_100, "Outro texto tambem bem longo aqui"),
        cue(10_000, 11_000, "Quarto"),
    ];
    reflow::reflow(&mut cues, &netflix()).unwrap();
    let fourth = cues.iter().find(|cue| cue.text.contains("Quarto")).expect("fourth cue");
    assert_eq!(fourth.start, Time::from_millis(10_000));
    assert_eq!(fourth.end, Time::from_millis(11_000));
    assert!(cues.iter().all(|cue| cue.start <= Time::from_millis(10_000)));
}

// ---------------------------------------------------------------------------
// MED: shifting before zero clamps / drops and warns
// ---------------------------------------------------------------------------

#[test]
fn shift_to_negative_time_clamps_drops_and_warns() {
    let _ = warnings::take();
    let mut cues = vec![
        cue(0, 1_000, "gone"),        // -5000..-4000 -> dropped
        cue(3_000, 6_000, "partial"), // -2000.. 1000 -> start clamped to 0
        cue(8_000, 9_000, "keep"),    //  3000.. 4000 -> untouched
    ];
    let emitted = ops::shift(&mut cues, Time::from_millis(-5_000)).unwrap();

    assert_eq!(cues.len(), 2);
    assert_eq!(cues[0].text, "partial");
    assert_eq!(cues[0].start, Time::ZERO);
    assert_eq!(cues[0].end, Time::from_millis(1_000));
    assert_eq!(cues[1].text, "keep");
    assert_eq!(cues[1].start, Time::from_millis(3_000));

    assert!(emitted.iter().any(|message| message.contains("dropped")), "{emitted:?}");
    assert!(emitted.iter().any(|message| message.contains("clamp")), "{emitted:?}");
}

#[test]
fn every_writer_formats_negative_time_as_zero() {
    // A writer can never emit "-00:00:05,000": the parser rejects it, so the
    // round-trip guarantee would break. Defensive clamping belongs in the
    // shared formatter used by SRT, WebVTT and ASS alike.
    let negative = Time::from_millis(-5_000);
    assert_eq!(negative.format_srt(), "00:00:00,000");
    assert_eq!(negative.format_vtt(), "00:00:00.000");
    assert_eq!(negative.format_ass(), "00:00:00.00");
}

// ---------------------------------------------------------------------------
// MED: per-sequence UTF-8 -> windows-1252 fallback
// ---------------------------------------------------------------------------

#[test]
fn stray_cp1252_byte_does_not_mojibake_the_whole_utf8_file() {
    let _ = warnings::take();
    // "Ação" is 41 C3 A7 C3 A3 6F in UTF-8. One stray 0x92 (cp1252 right
    // single quote) used to make `detect` classify the whole file as cp1252,
    // turning the valid sequences into "AÃ§Ã£o". Only the stray byte may be
    // remapped; the result must be "Ação" + U+2019 + "!".
    let mut bytes = b"A".to_vec();
    bytes.extend_from_slice("ção".as_bytes());
    bytes.push(0x92);
    bytes.extend_from_slice(b"!");

    let (text, detected) = encoding::decode(&bytes);
    assert_eq!(text, "Ação’!");
    assert_eq!(detected, Encoding::Utf8With1252Fallback);
    let warnings = warnings::take();
    assert!(warnings.iter().any(|message| message.contains("windows-1252")), "{warnings:?}");
}

#[test]
fn pure_cp1252_file_is_still_reported_as_windows_1252() {
    let _ = warnings::take();
    let bytes = [0x4F, 0x6C, 0xE1, 0x2C, 0x20, 0x80];
    let (text, detected) = encoding::decode(&bytes);
    assert_eq!(text, "Olá, €");
    assert_eq!(detected, Encoding::Windows1252);
}

// ---------------------------------------------------------------------------
// LOW: lint overlap units
// ---------------------------------------------------------------------------

#[test]
fn lint_overlap_values_are_reported_in_seconds_like_every_other_rule() {
    // Cue 1 ends at 2.000 s, cue 2 starts at 1.500 s. `value` is the observed
    // start and `limit` the previous end, both in SECONDS as for CPS/gap
    // issues. The old code wrote 1500.0 / 2000.0 (milliseconds) here while
    // every other rule used seconds.
    let config = LintConfig::netflix_ptbr(FrameRate::FPS_24, false).unwrap();
    let report =
        LintReport::new(&[cue(1_000, 2_000, "Primeira"), cue(1_500, 3_000, "Segunda")], &config);
    let overlap =
        report.issues.iter().find(|issue| issue.rule == LintRule::Overlap).expect("overlap issue");
    assert!((overlap.value - 1.5).abs() < 1e-9, "value in seconds: {overlap:?}");
    assert!((overlap.limit - 2.0).abs() < 1e-9, "limit in seconds: {overlap:?}");
}
