//! End-to-end tests for the `legenda` binary.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use legenda_core::{Format, Subtitle};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn temp_dir() -> PathBuf {
    let sequence = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let directory = std::env::temp_dir().join(format!("legenda-cli-{nanos}-{sequence}"));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_legenda")).args(arguments).output().unwrap()
}

#[test]
fn convert_srt_to_ass_translates_style_and_writes_valid_ass() {
    let directory = temp_dir();
    let input = directory.join("input.srt");
    let output = directory.join("output.ass");
    std::fs::write(&input, "1\n00:00:01,000 --> 00:00:02,000\n<i>Olá</i>\n").unwrap();

    let result = run(&["convert", input.to_str().unwrap(), output.to_str().unwrap()]);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert!(String::from_utf8_lossy(&result.stdout).contains("converted srt -> ass (1 cues)"));

    let text = std::fs::read_to_string(&output).unwrap();
    assert!(text.contains("[Script Info]"));
    assert!(text.contains("[V4+ Styles]"));
    assert!(text.contains("Dialogue: 0,00:00:01.00,00:00:02.00,Default,,0,0,0,,{\\i1}Olá{\\i0}"));
    let reparsed = Subtitle::decode(text.as_bytes(), Format::Ass).unwrap();
    assert_eq!(reparsed.plain_cues().unwrap()[0].text, "<i>Olá</i>");
}

#[test]
fn convert_vtt_short_timestamps_to_srt() {
    let directory = temp_dir();
    let input = directory.join("input.vtt");
    let output = directory.join("output.srt");
    std::fs::write(&input, "WEBVTT\n\n00:01.000 --> 00:02.500 align:start\nOlá, mundo!\n").unwrap();

    let result = run(&["convert", input.to_str().unwrap(), output.to_str().unwrap()]);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let text = std::fs::read_to_string(&output).unwrap();
    assert!(text.contains("00:00:01,000 --> 00:00:02,500"));
    assert!(text.contains("Olá, mundo!"));
}

#[test]
fn info_reports_format_encoding_and_cue_count() {
    let directory = temp_dir();
    let input = directory.join("input.srt");
    std::fs::write(
        &input,
        "1\n00:00:01,000 --> 00:00:02,000\nPrimeira\n\n2\n00:00:03,000 --> 00:00:04,000\nSegunda\n",
    )
    .unwrap();

    let result = run(&["info", input.to_str().unwrap()]);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("format: srt"));
    assert!(stdout.contains("encoding: UTF-8"));
    assert!(stdout.contains("cues: 2"));
    assert!(stdout.contains("first start: 00:00:01,000"));
    assert!(stdout.contains("last end: 00:00:04,000"));
}

#[test]
fn unknown_format_flag_fails_with_clear_error() {
    let directory = temp_dir();
    let input = directory.join("input.srt");
    let output = directory.join("output.srt");
    std::fs::write(&input, "1\n00:00:01,000 --> 00:00:02,000\nOlá\n").unwrap();

    let result =
        run(&["convert", input.to_str().unwrap(), output.to_str().unwrap(), "--from", "bogus"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("unsupported subtitle format"));
}

#[test]
fn cli_round_trip_through_vtt_preserves_cues() {
    let directory = temp_dir();
    let srt = directory.join("original.srt");
    let vtt = directory.join("middle.vtt");
    let restored = directory.join("restored.srt");
    std::fs::write(
        &srt,
        "1\n00:00:01,000 --> 00:00:02,000\n<i>Olá</i>\n\n2\n00:00:03,000 --> 00:00:04,000\nMundo\n",
    )
    .unwrap();

    assert!(run(&["convert", srt.to_str().unwrap(), vtt.to_str().unwrap()]).status.success());
    assert!(run(&["convert", vtt.to_str().unwrap(), restored.to_str().unwrap()]).status.success());

    let original = Subtitle::decode(&std::fs::read(&srt).unwrap(), Format::Srt).unwrap();
    let round_tripped = Subtitle::decode(&std::fs::read(&restored).unwrap(), Format::Srt).unwrap();
    assert_eq!(original.plain_cues().unwrap(), round_tripped.plain_cues().unwrap());
}
