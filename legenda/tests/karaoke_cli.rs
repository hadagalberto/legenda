//! End-to-end tests for `legenda karaoke` and `legenda burn`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use legenda_core::{ass, Format, Subtitle};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn temp_dir(name: &str) -> PathBuf {
    let sequence = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let directory = std::env::temp_dir().join(format!("legenda-{name}-{nanos}-{sequence}"));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_legenda")).args(arguments).output().unwrap()
}

const TRANSCRIPT: &str = r#"{
  "segments": [
    {"start": 0.0, "end": 1.2, "words": [
      {"word": "Hello", "start": 0.0, "end": 0.4, "score": 0.9},
      {"word": "world.", "start": 0.5, "end": 1.0, "score": 0.9}
    ]},
    {"start": 1.4, "end": 2.0, "words": [
      {"word": "Again", "start": 1.4, "end": 2.0, "score": 0.9}
    ]}
  ]
}"#;

fn write_transcript(path: &Path) {
    std::fs::write(path, TRANSCRIPT).unwrap();
}

#[test]
fn karaoke_command_writes_parseable_ass_for_every_preset() {
    let directory = temp_dir("karaoke-cli");
    let input = directory.join("transcript.json");
    write_transcript(&input);
    for preset in ["hormozi", "clean", "karaoke", "bounce"] {
        let output = directory.join(format!("{preset}.ass"));
        let result = run(&[
            "karaoke",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--preset",
            preset,
        ]);
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        let stdout = String::from_utf8_lossy(&result.stdout);
        assert!(stdout.contains("2 karaoke pages"), "{stdout}");
        assert!(stdout.contains("from 3 words"), "{stdout}");
        let text = std::fs::read_to_string(&output).unwrap();
        let document = ass::parse(&text).unwrap();
        assert_eq!(document.dialogues().len(), 2);
    }
}

#[test]
fn karaoke_command_applies_font_emphasis_and_canvas_options() {
    let directory = temp_dir("karaoke-options");
    let input = directory.join("transcript.json");
    let output = directory.join("out.ass");
    write_transcript(&input);
    let result = run(&[
        "karaoke",
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--preset",
        "bounce",
        "--font",
        "Anton",
        "--font-size",
        "96",
        "--outline",
        "7",
        "--shadow",
        "2",
        "--safe-bottom",
        "300",
        "--width",
        "720",
        "--height",
        "1280",
        "--emphasis",
        "world",
    ]);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let text = std::fs::read_to_string(&output).unwrap();
    assert!(text.contains("PlayResX: 720"));
    assert!(text.contains("PlayResY: 1280"));
    let style: Vec<&str> =
        text.lines().find(|line| line.starts_with("Style:")).unwrap().split(',').collect();
    assert_eq!(style[1], "Anton");
    assert_eq!(style[2], "96");
    assert_eq!(style[16], "7", "outline option is applied");
    assert_eq!(style[17], "2", "shadow option is applied");
    assert_eq!(style[21], "300", "bottom safe area is applied");
    assert!(text.contains("\\pos(360,980)"));
    assert!(text.contains("\\fscx145"), "emphasis word gets stronger bounce");
    let reparsed = Subtitle::decode(text.as_bytes(), Format::Ass).unwrap();
    assert_eq!(reparsed.plain_cues().unwrap().len(), 2);
}

#[test]
fn karaoke_command_rejects_invalid_options() {
    let directory = temp_dir("karaoke-invalid");
    let input = directory.join("transcript.json");
    let output = directory.join("out.ass");
    write_transcript(&input);
    let input = input.to_str().unwrap();
    let output = output.to_str().unwrap();
    assert!(!run(&["karaoke", input, "-o", output, "--preset", "nope"]).status.success());
    assert!(!run(&["karaoke", input, "-o", output, "--font-size", "0"]).status.success());
    std::fs::write(input, "{}").unwrap();
    assert!(!run(&["karaoke", input, "-o", output]).status.success());
}

#[test]
fn burn_embeds_captions_into_generated_video() {
    let directory = temp_dir("burn, [e2e]; #1's");
    let video = directory.join("input.mp4");
    let transcript = directory.join("transcript.json");
    let captions = directory.join("captions, [burn]; #1's.ass");
    let output = directory.join("output.mp4");
    write_transcript(&transcript);

    let generate = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=1080x1920:d=1:r=10",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-y",
        ])
        .arg(&video)
        .status()
        .expect("ffmpeg must be installed for the burn e2e test");
    assert!(generate.success(), "failed to generate the fixture video");

    let karaoke = run(&[
        "karaoke",
        transcript.to_str().unwrap(),
        "-o",
        captions.to_str().unwrap(),
        "--preset",
        "hormozi",
    ]);
    assert!(karaoke.status.success(), "{}", String::from_utf8_lossy(&karaoke.stderr));

    let result = run(&[
        "burn",
        video.to_str().unwrap(),
        captions.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--encoder-option",
        "crf=28",
    ]);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert!(output.exists());

    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(&output)
        .output()
        .expect("ffprobe must be installed for the burn e2e test");
    assert!(probe.status.success(), "{}", String::from_utf8_lossy(&probe.stderr));
    let duration: f64 = String::from_utf8_lossy(&probe.stdout).trim().parse().unwrap();
    assert!((duration - 1.0).abs() < 0.25, "unexpected output duration {duration}");
}

#[test]
fn burn_rejects_malformed_encoder_options_before_spawning_ffmpeg() {
    let directory = temp_dir("burn-invalid");
    let video = directory.join("video.mp4");
    let captions = directory.join("captions.ass");
    let output = directory.join("out.mp4");
    let result = run(&[
        "burn",
        video.to_str().unwrap(),
        captions.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--encoder-option",
        "no-equals-sign",
    ]);
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("expected key=value"));
}
