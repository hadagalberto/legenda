//! Fixture-driven parser, writer, snapshot and conversion tests.

use std::path::{Path, PathBuf};

use insta::assert_snapshot;
use legenda_core::ass::{self, AssSection};
use legenda_core::convert;
use legenda_core::encoding::{self, Encoding};
use legenda_core::error::Error;
use legenda_core::srt;
use legenda_core::time::Time;
use legenda_core::vtt::{self, VttBlock};
use legenda_core::{Format, Subtitle};

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture_path(name))
        .unwrap_or_else(|error| panic!("missing fixture {name}: {error}"))
}

fn decode_fixture(name: &str, format: Format) -> Subtitle {
    Subtitle::decode(&fixture_bytes(name), format).unwrap()
}

fn as_srt(document: Subtitle) -> srt::SrtDocument {
    match document {
        Subtitle::Srt(value) => value,
        other => panic!("expected SRT, got {:?}", other.format()),
    }
}

#[test]
fn srt_basic_known_vectors_and_snapshot() {
    let document = as_srt(decode_fixture("srt_basic.srt", Format::Srt));
    assert_eq!(document.cues.len(), 2);
    assert_eq!(document.cues[0].index, 1);
    assert_eq!(document.cues[0].start, Time::from_hms_millis(0, 0, 1, 0).unwrap());
    assert_eq!(document.cues[0].end, Time::from_millis(3_200));
    assert_eq!(document.cues[0].text, "Olá, mundo!");
    assert_eq!(document.cues[1].index, 2);
    assert_eq!(document.cues[1].text, "Segunda linha com <i>ênfase</i>.");
    assert_snapshot!("srt_basic_writer", srt::write(&document));
}

#[test]
fn srt_edge_cases_are_tolerated() {
    let document = as_srt(decode_fixture("srt_edge_cases.srt", Format::Srt));
    assert_eq!(document.cues.iter().map(|cue| cue.index).collect::<Vec<_>>(), vec![1, 2, 9]);
    assert_eq!(document.cues[0].start, Time::from_millis(500));
    assert_eq!(document.cues[0].end, Time::from_millis(1_250));
    assert_eq!(document.cues[0].text, "<b>Primeira</b> sem índice\nlinha dois");
    assert_eq!(document.cues[1].text, "<font color=\"#FF0000\">Vermelho</font>");
    assert_eq!(document.cues[2].text, "Última");
    assert_snapshot!("srt_edge_cases_writer", srt::write(&document));
    assert_eq!(srt::parse(&srt::write(&document)).unwrap(), document);
}

#[test]
fn srt_binary_encoding_variants() {
    let cases = [
        ("srt_crlf.bin", Encoding::Utf8, "CRLF e <b>tags</b>"),
        ("srt_utf16le.bin", Encoding::Utf16Le, "UTF-16 olá"),
        ("srt_cp1252.bin", Encoding::Windows1252, "Olá, mundo €"),
    ];
    for (name, expected_encoding, expected_text) in cases {
        let bytes = fixture_bytes(name);
        let (text, detected) = encoding::decode(&bytes);
        assert_eq!(detected, expected_encoding, "encoding for {name}");
        let document = srt::parse(&text).unwrap();
        assert_eq!(document.cues.len(), 1, "cue count for {name}");
        assert_eq!(document.cues[0].text, expected_text, "text for {name}");
        assert_eq!(srt::parse(&srt::write(&document)).unwrap(), document);
    }
}

#[test]
fn vtt_basic_known_vectors_and_snapshot() {
    let document = decode_fixture("vtt_basic.vtt", Format::Vtt);
    let legenda_core::Subtitle::Vtt(document) = document else {
        panic!("expected WebVTT");
    };
    assert_eq!(document.header, vec!["WEBVTT"]);
    assert_eq!(document.blocks.len(), 2);
    let VttBlock::Cue(first) = &document.blocks[0] else {
        panic!("expected cue");
    };
    assert_eq!(first.start, Time::from_millis(1_000));
    assert_eq!(first.end, Time::from_millis(2_500));
    assert_eq!(first.text, "Olá, mundo!");
    let VttBlock::Cue(second) = &document.blocks[1] else {
        panic!("expected cue");
    };
    assert_eq!(second.settings, "line:90% align:middle");
    assert_eq!(second.text, "<i>Ênfase</i>");
    assert_snapshot!("vtt_basic_writer", vtt::write(&document));
}

#[test]
fn vtt_edge_cases_preserve_notes_styles_ids_and_settings() {
    let document = decode_fixture("vtt_edge_cases.vtt", Format::Vtt);
    let legenda_core::Subtitle::Vtt(document) = document else {
        panic!("expected WebVTT");
    };
    assert_eq!(document.header.len(), 3);
    assert_eq!(document.header[0], "WEBVTT - versão de teste");
    assert!(matches!(&document.blocks[0], VttBlock::Raw(raw) if raw.starts_with("NOTE")));
    assert!(matches!(&document.blocks[1], VttBlock::Raw(raw) if raw.starts_with("STYLE")));
    let VttBlock::Cue(first) = &document.blocks[2] else {
        panic!("expected cue");
    };
    assert_eq!(first.id.as_deref(), Some("intro"));
    assert_eq!(first.start, Time::from_millis(1_000));
    assert_eq!(first.end, Time::from_millis(3_500));
    assert_eq!(first.settings, "position:50%");
    assert_eq!(first.text, "Primeira linha\nSegunda linha");
    let VttBlock::Cue(second) = &document.blocks[3] else {
        panic!("expected cue");
    };
    assert_eq!(second.settings, "align:end");
    assert_eq!(second.text, "<v João>Olá!</v>");
    assert_snapshot!("vtt_edge_cases_writer", vtt::write(&document));
    assert_eq!(vtt::parse(&vtt::write(&document)).unwrap(), document);
}

#[test]
fn ass_basic_known_vectors_and_snapshot() {
    let document = decode_fixture("ass_basic.ass", Format::Ass);
    let legenda_core::Subtitle::Ass(document) = document else {
        panic!("expected ASS");
    };
    assert_eq!(document.sections.len(), 3);
    let dialogues = document.dialogues();
    assert_eq!(dialogues.len(), 2);
    let (format, first) = dialogues[0];
    assert_eq!(first.start(format).unwrap(), Time::from_millis(1_000));
    assert_eq!(first.end(format).unwrap(), Time::from_millis(2_500));
    assert_eq!(first.text(format).unwrap(), "Olá, mundo!");
    let (format, second) = dialogues[1];
    assert_eq!(second.text(format).unwrap(), "{\\i1}Ênfase{\\i0}");
    assert_snapshot!("ass_basic_writer", ass::write(&document).unwrap());
}

#[test]
fn ass_edge_cases_preserve_unknown_sections_and_comments() {
    let document = decode_fixture("ass_edge_cases.ass", Format::Ass);
    let legenda_core::Subtitle::Ass(document) = document else {
        panic!("expected ASS");
    };
    assert_eq!(document.sections.len(), 5);
    assert!(matches!(&document.sections[2], AssSection::Raw { header, .. } if header == "[Fonts]"));
    assert!(
        matches!(&document.sections[3], AssSection::Raw { header, .. } if header == "[Aegisub Project Garbage]")
    );
    let AssSection::Styles { styles, .. } = &document.sections[1] else {
        panic!("expected styles");
    };
    assert_eq!(styles.len(), 2);
    let dialogues = document.dialogues();
    assert_eq!(dialogues.len(), 2);
    assert_eq!(dialogues[0].1.text(dialogues[0].0).unwrap(), "{\\i1}Olá{\\i0}\\Nsegunda linha");
    assert_eq!(
        dialogues[1].1.text(dialogues[1].0).unwrap(),
        "{\\b1}{\\c&H000000FF&}VERMELHO{\\b0}"
    );
    let AssSection::Events { events, .. } = &document.sections[4] else {
        panic!("expected events");
    };
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].kind, "Comment");
    assert_snapshot!("ass_edge_cases_writer", ass::write(&document).unwrap());
    assert_eq!(ass::parse(&ass::write(&document).unwrap()).unwrap(), document);
}

#[test]
fn every_text_fixture_round_trips_losslessly() {
    let cases = [
        ("srt_basic.srt", Format::Srt),
        ("srt_edge_cases.srt", Format::Srt),
        ("vtt_basic.vtt", Format::Vtt),
        ("vtt_edge_cases.vtt", Format::Vtt),
        ("ass_basic.ass", Format::Ass),
        ("ass_edge_cases.ass", Format::Ass),
    ];
    for (name, format) in cases {
        let original = decode_fixture(name, format);
        let encoded = original.encode().unwrap();
        let reparsed = Subtitle::decode(&encoded, format).unwrap();
        assert_eq!(reparsed, original, "round trip for {name}");
    }
}

#[test]
fn conversions_preserve_cues_and_translate_styling() {
    let srt = decode_fixture("srt_edge_cases.srt", Format::Srt);

    let vtt = convert::convert(&srt, Format::Vtt).unwrap();
    assert_eq!(vtt.plain_cues().unwrap(), srt.plain_cues().unwrap());
    let vtt_bytes = vtt.encode().unwrap();
    assert_eq!(Subtitle::decode(&vtt_bytes, Format::Vtt).unwrap(), vtt);
    assert_snapshot!("convert_srt_to_vtt", String::from_utf8(vtt_bytes).unwrap());

    let ass = convert::convert(&srt, Format::Ass).unwrap();
    let ass_bytes = ass.encode().unwrap();
    let ass_text = String::from_utf8(ass_bytes).unwrap();
    assert!(ass_text.contains("{\\b1}Primeira{\\b0} sem índice\\Nlinha dois"));
    assert!(ass_text.contains("{\\c&H000000FF&}Vermelho{\\c&H00FFFFFF&}"));
    assert_eq!(Subtitle::decode(ass_text.as_bytes(), Format::Ass).unwrap(), ass);
    assert_snapshot!("convert_srt_to_ass", ass_text);

    let ass_source = decode_fixture("ass_basic.ass", Format::Ass);
    let srt_from_ass = convert::convert(&ass_source, Format::Srt).unwrap();
    let srt_text = String::from_utf8(srt_from_ass.encode().unwrap()).unwrap();
    assert!(srt_text.contains("<i>Ênfase</i>"));
    assert_snapshot!("convert_ass_to_srt", srt_text);

    let vtt_source = decode_fixture("vtt_edge_cases.vtt", Format::Vtt);
    let ass_from_vtt = convert::convert(&vtt_source, Format::Ass).unwrap();
    let cues = ass_from_vtt.plain_cues().unwrap();
    assert_eq!(cues[0].text, "Primeira linha\nSegunda linha");
    assert_eq!(cues[1].text, "Olá!");
    assert_snapshot!(
        "convert_vtt_to_ass",
        String::from_utf8(ass_from_vtt.encode().unwrap()).unwrap()
    );
}

#[test]
fn conversion_to_every_target_is_reparseable() {
    let sources = [
        (Format::Srt, "srt_basic.srt"),
        (Format::Vtt, "vtt_basic.vtt"),
        (Format::Ass, "ass_basic.ass"),
    ];
    for (source_format, name) in sources {
        let source = decode_fixture(name, source_format);
        for target in Format::ALL {
            let converted = convert::convert(&source, target).unwrap();
            let bytes = converted.encode().unwrap();
            let reparsed = Subtitle::decode(&bytes, target).unwrap();
            assert_eq!(reparsed, converted, "{name} -> {target:?}");
        }
    }
}

#[test]
fn known_invalid_inputs_are_rejected() {
    assert!(Subtitle::decode(b"1\n00:00:60,000 --> 00:00:61,000\ntexto\n", Format::Srt).is_err());
    assert!(Subtitle::decode(b"NOT WEBVTT\n\n00:00:01.000 --> 00:00:02.000\ntexto\n", Format::Vtt)
        .is_err());
    assert!(Subtitle::decode(
        b"[Events]\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,texto\n",
        Format::Ass
    )
    .is_err());

    let invalid_ass_time = b"[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:aa.00,0:00:02.00,Default,,0,0,0,,texto\n";
    let document = Subtitle::decode(invalid_ass_time, Format::Ass).unwrap();
    assert!(document.plain_cues().is_err());

    assert!(matches!(
        Format::detect(b"sem extensao", "file.unknown"),
        Err(Error::UnsupportedFormat(_))
    ));
    assert!(Format::from_name("bogus").is_none());
}

#[test]
fn format_detection_uses_extension_then_content() {
    assert_eq!(Format::detect(b"anything", "captions.srt").unwrap(), Format::Srt);
    assert_eq!(Format::detect(b"anything", "captions.vtt").unwrap(), Format::Vtt);
    assert_eq!(Format::detect(b"anything", "captions.ssa").unwrap(), Format::Ass);
    assert_eq!(
        Format::detect(b"WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nOla\n", "captions.txt").unwrap(),
        Format::Vtt
    );
    assert_eq!(
        Format::detect(b"[Script Info]\nScriptType: v4.00+\n", "captions.txt").unwrap(),
        Format::Ass
    );
}
