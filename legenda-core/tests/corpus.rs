//! Byte-exact fixture corpus for the tolerant parsers and strict writers.
//!
//! Every fixture below is a real file under `tests/fixtures/`. Binary files
//! (UTF-16 BOMs, windows-1252, CR-only line endings, UTF-8 BOMs) are marked as
//! `binary` in `.gitattributes` so their bytes survive any checkout on any
//! operating system. For each fixture the harness snapshots the parsed model
//! and the rewritten document, checks a hand-verified known vector and proves
//! that parse -> write -> parse is lossless and byte-stable.
//!
//! The corpus is deliberately broad: SRT (BOMs, UTF-16, windows-1252, CR-only,
//! missing indices, `.` separators, coordinates, overlaps, empty cues, emoji,
//! partial fractions, hour-less timestamps), WebVTT (NOTE/STYLE/REGION,
//! settings, voice tags, ids, short timestamps, empty cues) and ASS/SSA
//! (comments, embedded fonts, override tags, reordered `Format:` fields,
//! SSA v4, multiple sections, overlaps).

use std::path::{Path, PathBuf};

use insta::{assert_debug_snapshot, assert_snapshot};
use legenda_core::convert;
use legenda_core::encoding::{self, Encoding};
use legenda_core::time::Time;
use legenda_core::{Format, PlainCue, Subtitle};
use proptest::prelude::*;

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture_path(name))
        .unwrap_or_else(|error| panic!("missing fixture {name}: {error}"))
}

/// Guards the SPEC's "fixture corpus" bar against accidental shrinkage.
#[test]
fn fixture_corpus_meets_minimum_size() {
    let count = std::fs::read_dir(fixture_path(""))
        .unwrap()
        .filter(|entry| entry.as_ref().is_ok_and(|entry| entry.path().is_file()))
        .count();
    assert!(count >= 40, "corpus has only {count} fixtures, expected at least 40");
}

/// `.gitattributes` marks these files `binary`; this fails loudly if a CRLF
/// checkout, an editor or a formatter ever rewrites their exact bytes.
#[test]
fn byte_exact_fixtures_keep_their_original_bytes() {
    let cr_only = fixture_bytes("srt_cr_only.srt");
    assert!(cr_only.contains(&b'\r'));
    assert!(!cr_only.contains(&b'\n'), "CR-only fixture was normalised to LF");

    let utf16_le = fixture_bytes("srt_utf16le.bin");
    assert!(utf16_le.starts_with(&[0xFF, 0xFE]));
    let utf16_be = fixture_bytes("srt_utf16be.bin");
    assert!(utf16_be.starts_with(&[0xFE, 0xFF]));

    let cp1252 = fixture_bytes("srt_cp1252.bin");
    assert!(cp1252.contains(&0xE1), "windows-1252 accent was rewritten");

    let bom = fixture_bytes("srt_bom.srt");
    assert!(bom.starts_with(&[0xEF, 0xBB, 0xBF]));
}

/// Generates four independent tests per fixture: a parsed-model snapshot with
/// known-vector assertions, a writer snapshot, a lossless round trip and a
/// byte-stability check for a second write.
macro_rules! fixture_tests {
    ($($name:ident: { file: $file:literal, format: $format:ident, encoding: $encoding:ident,
                      cues: $cues:literal, start: $start:literal, end: $end:literal,
                      text: $text:literal };)+) => {
        $(
            mod $name {
                use super::*;

                #[test]
                fn parsed_model_snapshot() {
                    let bytes = fixture_bytes($file);
                    let (_, detected) = encoding::decode(&bytes);
                    assert_eq!(detected, Encoding::$encoding, "encoding of {}", $file);

                    let document = Subtitle::decode(&bytes, Format::$format)
                        .unwrap_or_else(|error| panic!("{}: {error}", $file));
                    let cues = document.plain_cues().unwrap();
                    assert_eq!(cues.len(), $cues, "cue count of {}", $file);
                    let first = &cues[0];
                    assert_eq!(first.start, Time::from_millis($start), "start of {}", $file);
                    assert_eq!(first.end, Time::from_millis($end), "end of {}", $file);
                    assert_eq!(first.text, $text, "first cue text of {}", $file);
                    assert_debug_snapshot!(
                        concat!("corpus__", stringify!($name), "_parsed"), document
                    );
                }

                #[test]
                fn rewritten_output_snapshot() {
                    let document = Subtitle::decode(&fixture_bytes($file), Format::$format).unwrap();
                    let written = document.encode().unwrap();
                    let text = String::from_utf8(written).expect("writers emit UTF-8");
                    assert!(!text.contains('\r'), "writer must emit LF: {}", $file);
                    assert_snapshot!(concat!("corpus__", stringify!($name), "_written"), text);
                }

                #[test]
                fn round_trip_is_lossless() {
                    let original =
                        Subtitle::decode(&fixture_bytes($file), Format::$format).unwrap();
                    let written = original.encode().unwrap();
                    let reparsed = Subtitle::decode(&written, Format::$format).unwrap();
                    assert_eq!(reparsed, original, "round trip of {}", $file);
                }

                #[test]
                fn writing_twice_is_byte_stable() {
                    let original =
                        Subtitle::decode(&fixture_bytes($file), Format::$format).unwrap();
                    let first = original.encode().unwrap();
                    let reparsed = Subtitle::decode(&first, Format::$format).unwrap();
                    let second = reparsed.encode().unwrap();
                    assert_eq!(second, first, "second write differs for {}", $file);
                }
            }
        )+
    };
}

fixture_tests! {
    srt_bom_srt: { file: "srt_bom.srt", format: Srt, encoding: Utf8Bom, cues: 1, start: 0, end: 1000, text: "BOM" };
    srt_cr_only_srt: { file: "srt_cr_only.srt", format: Srt, encoding: Utf8, cues: 2, start: 0, end: 1000, text: "Primeira" };
    srt_missing_indices_srt: { file: "srt_missing_indices.srt", format: Srt, encoding: Utf8, cues: 2, start: 1000, end: 2000, text: "Sem índice" };
    srt_dot_ms_srt: { file: "srt_dot_ms.srt", format: Srt, encoding: Utf8, cues: 1, start: 1250, end: 2750, text: "Separador ponto" };
    srt_overlapping_srt: { file: "srt_overlapping.srt", format: Srt, encoding: Utf8, cues: 3, start: 1000, end: 3000, text: "Sobreposta" };
    srt_empty_cues_srt: { file: "srt_empty_cues.srt", format: Srt, encoding: Utf8, cues: 2, start: 1000, end: 2000, text: "" };
    srt_unicode_emoji_srt: { file: "srt_unicode_emoji.srt", format: Srt, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "Olá, 🌍 é básico! 😊" };
    srt_coordinates_srt: { file: "srt_coordinates.srt", format: Srt, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "Com coordenadas" };
    srt_html_tags_srt: { file: "srt_html_tags.srt", format: Srt, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "<i>itálico</i> e <b>negrito</b>\n<font color=\"#00FF00\">verde</font>" };
    srt_extra_blanks_srt: { file: "srt_extra_blanks.srt", format: Srt, encoding: Utf8, cues: 2, start: 1000, end: 2000, text: "Primeira" };
    srt_hourless_srt: { file: "srt_hourless.srt", format: Srt, encoding: Utf8, cues: 1, start: 1000, end: 2500, text: "Sem horas" };
    srt_partial_fraction_srt: { file: "srt_partial_fraction.srt", format: Srt, encoding: Utf8, cues: 1, start: 1500, end: 2250, text: "Fração parcial" };
    srt_no_final_newline_srt: { file: "srt_no_final_newline.srt", format: Srt, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "Sem newline final" };
    srt_large_indices_srt: { file: "srt_large_indices.srt", format: Srt, encoding: Utf8, cues: 2, start: 1000, end: 2000, text: "Índice 100" };
    srt_multiline_markup_srt: { file: "srt_multiline_markup.srt", format: Srt, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "<i>Primeira</i>\n<b>segunda</b>\nTerceira" };
    srt_utf16be_bin: { file: "srt_utf16be.bin", format: Srt, encoding: Utf16Be, cues: 1, start: 1000, end: 2000, text: "UTF-16 BE olá" };
    srt_cp1252_extra_bin: { file: "srt_cp1252_extra.bin", format: Srt, encoding: Windows1252, cues: 1, start: 1000, end: 2000, text: "Ação, coração, página €" };
    vtt_bom_vtt: { file: "vtt_bom.vtt", format: Vtt, encoding: Utf8Bom, cues: 1, start: 1000, end: 2000, text: "BOM VTT" };
    vtt_cr_only_vtt: { file: "vtt_cr_only.vtt", format: Vtt, encoding: Utf8, cues: 2, start: 1000, end: 2000, text: "Primeira" };
    vtt_note_style_region_vtt: { file: "vtt_note_style_region.vtt", format: Vtt, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "Texto" };
    vtt_cue_settings_vtt: { file: "vtt_cue_settings.vtt", format: Vtt, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "Configurado" };
    vtt_hourless_vtt: { file: "vtt_hourless.vtt", format: Vtt, encoding: Utf8, cues: 1, start: 1000, end: 2500, text: "Sem horas" };
    vtt_cue_ids_vtt: { file: "vtt_cue_ids.vtt", format: Vtt, encoding: Utf8, cues: 2, start: 1000, end: 2000, text: "Primeira" };
    vtt_empty_cues_vtt: { file: "vtt_empty_cues.vtt", format: Vtt, encoding: Utf8, cues: 2, start: 1000, end: 2000, text: "" };
    vtt_unicode_emoji_vtt: { file: "vtt_unicode_emoji.vtt", format: Vtt, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "Olá, 🌍 😊" };
    vtt_voice_tags_vtt: { file: "vtt_voice_tags.vtt", format: Vtt, encoding: Utf8, cues: 2, start: 1000, end: 2000, text: "<v Ana>Olá!</v>" };
    vtt_header_metadata_vtt: { file: "vtt_header_metadata.vtt", format: Vtt, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "Com cabeçalho" };
    vtt_no_final_newline_vtt: { file: "vtt_no_final_newline.vtt", format: Vtt, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "Sem newline final" };
    vtt_overlapping_vtt: { file: "vtt_overlapping.vtt", format: Vtt, encoding: Utf8, cues: 2, start: 1000, end: 3000, text: "Sobreposta" };
    vtt_utf16le_bin: { file: "vtt_utf16le.bin", format: Vtt, encoding: Utf16Le, cues: 1, start: 1000, end: 2000, text: "UTF-16 LE olá" };
    vtt_utf16be_bin: { file: "vtt_utf16be.bin", format: Vtt, encoding: Utf16Be, cues: 1, start: 1000, end: 2000, text: "UTF-16 BE olá" };
    vtt_cp1252_bin: { file: "vtt_cp1252.bin", format: Vtt, encoding: Windows1252, cues: 1, start: 1000, end: 2000, text: "Ação, coração, página €" };
    ass_bom_ass: { file: "ass_bom.ass", format: Ass, encoding: Utf8Bom, cues: 1, start: 1000, end: 2000, text: "BOM ASS" };
    ass_cr_only_ass: { file: "ass_cr_only.ass", format: Ass, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "Primeira" };
    ass_comments_ass: { file: "ass_comments.ass", format: Ass, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "Texto" };
    ass_fonts_ass: { file: "ass_fonts.ass", format: Ass, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "Com fonte" };
    ass_override_tags_ass: { file: "ass_override_tags.ass", format: Ass, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "<i>Itálico</i>\nnova\nlinha espaço" };
    ass_reordered_format_ass: { file: "ass_reordered_format.ass", format: Ass, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "Texto reordenado" };
    ass_ssa_v4_ass: { file: "ass_ssa_v4.ass", format: Ass, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "SSA v4 legado" };
    ass_overlapping_ass: { file: "ass_overlapping.ass", format: Ass, encoding: Utf8, cues: 3, start: 1000, end: 3000, text: "Sobreposta" };
    ass_empty_dialogue_ass: { file: "ass_empty_dialogue.ass", format: Ass, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "" };
    ass_unicode_emoji_ass: { file: "ass_unicode_emoji.ass", format: Ass, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "Olá, 🌍 😊" };
    ass_multi_section_ass: { file: "ass_multi_section.ass", format: Ass, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "Várias seções" };
    ass_partial_fraction_ass: { file: "ass_partial_fraction.ass", format: Ass, encoding: Utf8, cues: 1, start: 1500, end: 2250, text: "Fração parcial" };
    ass_no_final_newline_ass: { file: "ass_no_final_newline.ass", format: Ass, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "Sem newline final" };
    ass_utf16le_bin: { file: "ass_utf16le.bin", format: Ass, encoding: Utf16Le, cues: 1, start: 1000, end: 2000, text: "UTF-16 LE olá" };
    ass_utf16be_bin: { file: "ass_utf16be.bin", format: Ass, encoding: Utf16Be, cues: 1, start: 1000, end: 2000, text: "UTF-16 BE olá" };
    ass_cp1252_bin: { file: "ass_cp1252.bin", format: Ass, encoding: Windows1252, cues: 1, start: 1000, end: 2000, text: "Ação, coração, página €" };
    srt_basic_srt: { file: "srt_basic.srt", format: Srt, encoding: Utf8, cues: 2, start: 1000, end: 3200, text: "Olá, mundo!" };
    srt_edge_cases_srt: { file: "srt_edge_cases.srt", format: Srt, encoding: Utf8, cues: 3, start: 500, end: 1250, text: "<b>Primeira</b> sem índice\nlinha dois" };
    srt_crlf_bin: { file: "srt_crlf.bin", format: Srt, encoding: Utf8, cues: 1, start: 1000, end: 2000, text: "CRLF e <b>tags</b>" };
    srt_utf16le_bin: { file: "srt_utf16le.bin", format: Srt, encoding: Utf16Le, cues: 1, start: 1000, end: 2000, text: "UTF-16 olá" };
    srt_cp1252_bin: { file: "srt_cp1252.bin", format: Srt, encoding: Windows1252, cues: 1, start: 1000, end: 2000, text: "Olá, mundo €" };
    vtt_basic_vtt: { file: "vtt_basic.vtt", format: Vtt, encoding: Utf8, cues: 2, start: 1000, end: 2500, text: "Olá, mundo!" };
    vtt_edge_cases_vtt: { file: "vtt_edge_cases.vtt", format: Vtt, encoding: Utf8, cues: 2, start: 1000, end: 3500, text: "Primeira linha\nSegunda linha" };
    ass_basic_ass: { file: "ass_basic.ass", format: Ass, encoding: Utf8, cues: 2, start: 1000, end: 2500, text: "Olá, mundo!" };
    ass_edge_cases_ass: { file: "ass_edge_cases.ass", format: Ass, encoding: Utf8, cues: 2, start: 1000, end: 2000, text: "<i>Olá</i>\nsegunda linha" };
}

/// Each invalid input must be rejected by `Subtitle::decode`. These are
/// independent tests so a single regression is reported with the input name.
macro_rules! invalid_tests {
    ($($name:ident: ($format:ident, $input:expr));+ $(;)?) => {
        $(
            #[test]
            fn $name() {
                let error = Subtitle::decode($input.as_bytes(), Format::$format);
                assert!(error.is_err(), "{} should be rejected", stringify!($name));
            }
        )+
    };
}

invalid_tests! {
    srt_missing_timestamp: (Srt, "1\nthis is not a timestamp\ntexto\n");
    srt_seconds_out_of_range: (Srt, "1\n00:00:60,000 --> 00:00:61,000\ntexto\n");
    srt_minutes_out_of_range: (Srt, "1\n00:60:00,000 --> 00:61:00,000\ntexto\n");
    srt_non_numeric_fraction: (Srt, "1\n00:00:01,abc --> 00:00:02,000\ntexto\n");
    srt_missing_end_time: (Srt, "1\n00:00:01,000 -->\ntexto\n");
    srt_too_many_components: (Srt, "1\n00:00:00:01,000 --> 00:00:02,000\ntexto\n");
    vtt_missing_marker: (Vtt, "NOT WEBVTT\n\n00:00:01.000 --> 00:00:02.000\ntexto\n");
    vtt_seconds_out_of_range: (Vtt, "WEBVTT\n\n00:00:60.000 --> 00:00:61.000\ntexto\n");
    vtt_missing_end_time: (Vtt, "WEBVTT\n\n00:00:01.000 -->\ntexto\n");
    vtt_non_numeric_fraction: (Vtt, "WEBVTT\n\n00:00:01.abc --> 00:00:02.000\ntexto\n");
    ass_events_without_format: (Ass, "[Events]\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,texto\n");
}

/// ASS parsing is structural: timestamps are validated lazily when the neutral
/// cue view is built. These vectors parse as documents but must fail on read.
macro_rules! invalid_cue_tests {
    ($($name:ident: $input:expr);+ $(;)?) => {
        $(
            #[test]
            fn $name() {
                let document = Subtitle::decode($input.as_bytes(), Format::Ass).unwrap();
                assert!(
                    document.plain_cues().is_err(),
                    "{} should expose an invalid cue",
                    stringify!($name)
                );
            }
        )+
    };
}

invalid_cue_tests! {
    ass_non_numeric_hours: "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,aa:00:01.00,0:00:02.00,Default,,0,0,0,,texto\n";
    ass_minutes_out_of_range: "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:99:01.00,0:00:02.00,Default,,0,0,0,,texto\n";
    ass_seconds_out_of_range: "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:99.00,0:00:02.00,Default,,0,0,0,,texto\n";
    ass_dialogue_missing_start_field: "[Events]\nFormat: Layer, End, Style, Text\nDialogue: 0,0:00:02.00,Default,texto\n";
    ass_invalid_time_when_read: "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:aa.00,0:00:02.00,Default,,0,0,0,,texto\n";
}

proptest! {
    /// parse -> write -> parse for generated, valid cues in every format.
    /// Times are generated in centiseconds so ASS's centisecond writer is
    /// exact for all three formats.
    #[test]
    fn srt_generated_round_trip(start in 0i64..100_000, duration in 1i64..10_000, word in prop::sample::select(vec!["Olá", "mundo", "coração", "🌍", "casa"])) {
        let cues = vec![PlainCue {
            start: Time::from_millis(start * 10),
            end: Time::from_millis((start + duration) * 10),
            text: word.to_owned(),
        }];
        let document = Subtitle::from_plain(Format::Srt, &cues).unwrap();
        let bytes = document.encode().unwrap();
        prop_assert_eq!(Subtitle::decode(&bytes, Format::Srt).unwrap(), document);
    }

    #[test]
    fn vtt_generated_round_trip(start in 0i64..100_000, duration in 1i64..10_000, word in prop::sample::select(vec!["Olá", "mundo", "coração", "🌍", "casa"])) {
        let cues = vec![PlainCue {
            start: Time::from_millis(start * 10),
            end: Time::from_millis((start + duration) * 10),
            text: word.to_owned(),
        }];
        let document = Subtitle::from_plain(Format::Vtt, &cues).unwrap();
        let bytes = document.encode().unwrap();
        prop_assert_eq!(Subtitle::decode(&bytes, Format::Vtt).unwrap(), document);
    }

    #[test]
    fn ass_generated_round_trip(start in 0i64..100_000, duration in 1i64..10_000, word in prop::sample::select(vec!["Olá", "mundo", "coração", "🌍", "casa"])) {
        let cues = vec![PlainCue {
            start: Time::from_millis(start * 10),
            end: Time::from_millis((start + duration) * 10),
            text: word.to_owned(),
        }];
        let document = Subtitle::from_plain(Format::Ass, &cues).unwrap();
        let bytes = document.encode().unwrap();
        prop_assert_eq!(Subtitle::decode(&bytes, Format::Ass).unwrap(), document);
    }
}

/// All nine directed conversions from one representative styled fixture per
/// source format. Snapshots pin the documented styling mapping.
macro_rules! conversion_tests {
    ($($name:ident: ($source:literal, $from:ident, $to:ident));+ $(;)?) => {
        $(
            #[test]
            fn $name() {
                let bytes = fixture_bytes($source);
                let source = Subtitle::decode(&bytes, Format::$from).unwrap();
                let converted = convert::convert(&source, Format::$to).unwrap();
                let encoded = converted.encode().unwrap();
                let reparsed = Subtitle::decode(&encoded, Format::$to).unwrap();
                assert_eq!(reparsed, converted, "conversion of {}", $source);
                assert_snapshot!(
                    concat!("conversion__", stringify!($from), "_to_", stringify!($to)),
                    String::from_utf8(encoded).unwrap()
                );
            }
        )+
    };
}

conversion_tests! {
    srt_to_srt: ("srt_edge_cases.srt", Srt, Srt);
    srt_to_vtt: ("srt_edge_cases.srt", Srt, Vtt);
    srt_to_ass: ("srt_edge_cases.srt", Srt, Ass);
    vtt_to_srt: ("vtt_edge_cases.vtt", Vtt, Srt);
    vtt_to_vtt: ("vtt_edge_cases.vtt", Vtt, Vtt);
    vtt_to_ass: ("vtt_edge_cases.vtt", Vtt, Ass);
    ass_to_srt: ("ass_edge_cases.ass", Ass, Srt);
    ass_to_vtt: ("ass_edge_cases.ass", Ass, Vtt);
    ass_to_ass: ("ass_edge_cases.ass", Ass, Ass);
}
