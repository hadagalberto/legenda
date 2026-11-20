//! End-to-end tests for the milestone-2 CLI operations.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use legenda_core::{Format, Subtitle};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn temp_dir() -> PathBuf {
    let sequence = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let directory = std::env::temp_dir().join(format!("legenda-ops-{nanos}-{sequence}"));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_legenda")).args(arguments).output().unwrap()
}

fn cue_texts(path: &PathBuf) -> Vec<String> {
    let subtitle = Subtitle::decode(&std::fs::read(path).unwrap(), Format::Srt).unwrap();
    subtitle.plain_cues().unwrap().into_iter().map(|cue| cue.text).collect()
}

const SAMPLE: &str = "1\n00:00:01,000 --> 00:00:02,000\nOlá, mundo\n\n\
                      2\n00:00:03,000 --> 00:00:04,000\nAté logo\n";

#[test]
fn shift_command_moves_every_cue() {
    let directory = temp_dir();
    let input = directory.join("in.srt");
    let output = directory.join("out.srt");
    std::fs::write(&input, SAMPLE).unwrap();

    let result = run(&["shift", input.to_str().unwrap(), output.to_str().unwrap(), "--by", "1.5s"]);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let text = std::fs::read_to_string(&output).unwrap();
    assert!(text.contains("00:00:02,500 --> 00:00:03,500"));
    assert!(text.contains("00:00:04,500 --> 00:00:05,500"));
}

#[test]
fn scale_command_converts_framerate_exactly() {
    let directory = temp_dir();
    let input = directory.join("in.srt");
    let output = directory.join("out.srt");
    std::fs::write(&input, "1\n00:00:01,000 --> 00:00:02,000\nTexto\n").unwrap();

    let result = run(&[
        "scale",
        input.to_str().unwrap(),
        output.to_str().unwrap(),
        "--from",
        "23.976",
        "--to",
        "25",
    ]);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let text = std::fs::read_to_string(&output).unwrap();
    assert!(text.contains("00:00:00,959 --> 00:00:01,918"), "{text}");
}

#[test]
fn sync_command_maps_reference_points() {
    let directory = temp_dir();
    let input = directory.join("in.srt");
    let output = directory.join("out.srt");
    std::fs::write(&input, "1\n00:00:10,000 --> 00:00:11,000\nTexto\n").unwrap();

    let result = run(&[
        "sync",
        input.to_str().unwrap(),
        output.to_str().unwrap(),
        "--from-a",
        "10s",
        "--to-a",
        "20s",
        "--from-b",
        "20s",
        "--to-b",
        "40s",
    ]);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let text = std::fs::read_to_string(&output).unwrap();
    assert!(text.contains("00:00:20,000 --> 00:00:22,000"), "{text}");
}

#[test]
fn clamp_command_discards_cues_outside_range() {
    let directory = temp_dir();
    let input = directory.join("in.srt");
    let output = directory.join("out.srt");
    std::fs::write(&input, SAMPLE).unwrap();

    let result = run(&[
        "clamp",
        input.to_str().unwrap(),
        output.to_str().unwrap(),
        "--start",
        "1.5s",
        "--end",
        "2.5s",
    ]);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert_eq!(cue_texts(&output), ["Olá, mundo"]);
}

#[test]
fn merge_and_split_commands() {
    let directory = temp_dir();
    let first = directory.join("a.srt");
    let second = directory.join("b.srt");
    let merged = directory.join("merged.srt");
    let head = directory.join("head.srt");
    let tail = directory.join("tail.srt");
    std::fs::write(&first, "1\n00:00:03,000 --> 00:00:04,000\nTerceiro\n").unwrap();
    std::fs::write(&second, "1\n00:00:01,000 --> 00:00:02,000\nPrimeiro\n").unwrap();

    let result = run(&[
        "merge",
        first.to_str().unwrap(),
        second.to_str().unwrap(),
        "--output",
        merged.to_str().unwrap(),
    ]);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert_eq!(cue_texts(&merged), ["Primeiro", "Terceiro"]);

    let result = run(&[
        "split",
        merged.to_str().unwrap(),
        head.to_str().unwrap(),
        tail.to_str().unwrap(),
        "--at",
        "00:00:02,500",
    ]);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert_eq!(cue_texts(&head), ["Primeiro"]);
    assert_eq!(cue_texts(&tail), ["Terceiro"]);
}

#[test]
fn cleaning_commands_remove_annotations_and_labels() {
    let directory = temp_dir();
    let input = directory.join("in.srt");
    let hi = directory.join("hi.srt");
    let labels = directory.join("labels.srt");
    let clean = directory.join("clean.srt");
    std::fs::write(
        &input,
        "1\n00:00:01,000 --> 00:00:02,000\n[MÚSICA] Olá, tudo bem?\n\n\
         2\n00:00:03,000 --> 00:00:04,000\nJOÃO: Como vai?\n",
    )
    .unwrap();

    assert!(run(&["strip-hi", input.to_str().unwrap(), hi.to_str().unwrap()]).status.success());
    assert_eq!(cue_texts(&hi), ["Olá, tudo bem?", "JOÃO: Como vai?"]);

    assert!(run(&["strip-speakers", input.to_str().unwrap(), labels.to_str().unwrap()])
        .status
        .success());
    assert_eq!(cue_texts(&labels), ["[MÚSICA] Olá, tudo bem?", "Como vai?"]);

    assert!(run(&["clean", input.to_str().unwrap(), clean.to_str().unwrap()]).status.success());
    assert_eq!(cue_texts(&clean), ["Olá, tudo bem?", "Como vai?"]);
}

#[test]
fn lint_exit_codes_and_json_report() {
    let directory = temp_dir();
    let clean_input = directory.join("clean.srt");
    let bad_input = directory.join("bad.srt");
    std::fs::write(&clean_input, "1\n00:00:01,000 --> 00:00:04,000\nOlá, tudo bem?\n").unwrap();
    std::fs::write(&bad_input, "1\n00:00:01,000 --> 00:00:01,050\nOlá, tudo bem?\n").unwrap();

    let clean_result = run(&["lint", clean_input.to_str().unwrap(), "--json"]);
    assert_eq!(
        clean_result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&clean_result.stderr)
    );
    let clean_json: serde_json::Value = serde_json::from_slice(&clean_result.stdout).unwrap();
    assert_eq!(clean_json["violations"], 0);

    let bad_result = run(&["lint", bad_input.to_str().unwrap(), "--json"]);
    assert_eq!(bad_result.status.code(), Some(1));
    let bad_json: serde_json::Value = serde_json::from_slice(&bad_result.stdout).unwrap();
    assert!(bad_json["violations"].as_u64().unwrap() > 0);
    assert_eq!(bad_json["by_rule"]["too_short"], 1);
    assert_eq!(bad_json["by_rule"]["cps"], 1);

    let error_result = run(&["lint", bad_input.to_str().unwrap(), "--fps", "nonsense"]);
    assert_eq!(error_result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&error_result.stderr).contains("invalid frame rate"));
}

#[test]
fn reflow_command_enforces_preset_limits() {
    let directory = temp_dir();
    let input = directory.join("in.srt");
    let output = directory.join("out.srt");
    std::fs::write(
        &input,
        "1\n00:00:01,000 --> 00:00:01,200\nMuitos acreditam que só encontramos um amor verdadeiro em nossas vidas.\n",
    )
    .unwrap();

    let result = run(&[
        "reflow",
        input.to_str().unwrap(),
        output.to_str().unwrap(),
        "--preset",
        "netflix-ptbr",
    ]);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));

    let subtitle = Subtitle::decode(&std::fs::read(&output).unwrap(), Format::Srt).unwrap();
    let cues = subtitle.plain_cues().unwrap();
    assert!(!cues.is_empty());
    for cue in &cues {
        assert!(cue.text.lines().count() <= 2);
        assert!(cue.text.lines().all(|line| legenda_core::ops::visible_char_count(line) <= 42));
    }

    let lint_result = run(&["lint", output.to_str().unwrap(), "--json"]);
    assert_eq!(
        lint_result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&lint_result.stdout)
    );
}
