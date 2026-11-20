//! Visual end-to-end verification of karaoke captions.
//!
//! Captions are burned into a black video with ffmpeg, raw RGB frames are
//! extracted at the midpoint of several words, and the pixels are inspected
//! in Rust. This catches two regressions a text-only assertion cannot:
//! a highlight that never fires, and text that runs outside the safe area.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use legenda_core::ass;

mod common;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

const WIDTH: usize = 1_080;
const HEIGHT: usize = 1_920;

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

/// The maintainer's reproduction: 14 words, starts 380 ms apart.
const REPRO: &str = r#"{
  "segments": [{
    "start": 0.0, "end": 5.3,
    "words": [
      {"word": "Hoje", "start": 0.00, "end": 0.34},
      {"word": "eu", "start": 0.38, "end": 0.72},
      {"word": "vou", "start": 0.76, "end": 1.10},
      {"word": "te", "start": 1.14, "end": 1.48},
      {"word": "mostrar", "start": 1.52, "end": 1.86},
      {"word": "o", "start": 1.90, "end": 2.24},
      {"word": "segredo", "start": 2.28, "end": 2.62},
      {"word": "que", "start": 2.66, "end": 3.00},
      {"word": "ninguém", "start": 3.04, "end": 3.38},
      {"word": "conta", "start": 3.42, "end": 3.76},
      {"word": "sobre", "start": 3.80, "end": 4.14},
      {"word": "edição", "start": 4.18, "end": 4.52},
      {"word": "de", "start": 4.56, "end": 4.90},
      {"word": "vídeo", "start": 4.94, "end": 5.28}
    ]
  }]
}"#;

fn extract_frame(video: &Path, seconds: f64) -> Vec<u8> {
    let timestamp = format!("{seconds:.3}");
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(video)
        .args([
            "-ss",
            &timestamp,
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "pipe:1",
        ])
        .output()
        .expect("ffmpeg must be installed for the visual e2e test");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.stdout.len(), WIDTH * HEIGHT * 3, "unexpected raw frame size");
    output.stdout
}

fn pixel(frame: &[u8], x: usize, y: usize) -> (u8, u8, u8) {
    let index = (y * WIDTH + x) * 3;
    (frame[index], frame[index + 1], frame[index + 2])
}

fn strip_ass_tags(text: &str) -> String {
    let mut output = String::new();
    let mut in_tag = false;
    for character in text.chars() {
        match character {
            '{' => in_tag = true,
            '}' => in_tag = false,
            _ if !in_tag => output.push(character),
            _ => {}
        }
    }
    output
}

/// Word count of the Dialogue covering `seconds`. A one-word page is fully
/// highlighted, so it has no white pixels; longer pages must keep the other
/// words white while one word is active.
fn words_on_line_at(ass_text: &str, seconds: f64) -> usize {
    for (format, event) in ass::parse(ass_text).unwrap().dialogues() {
        let start = event.start(format).unwrap().as_secs_f64();
        let end = event.end(format).unwrap().as_secs_f64();
        if start <= seconds && seconds < end {
            return strip_ass_tags(event.text(format).unwrap()).split_whitespace().count();
        }
    }
    panic!("no dialogue covers t={seconds}");
}

#[test]
fn burned_frames_show_highlights_and_respect_the_safe_area() {
    if !common::libass_available() {
        return;
    }
    let directory = temp_dir("visual");
    let video = directory.join("input.mp4");
    let transcript = directory.join("transcript.json");
    let captions = directory.join("captions.ass");
    let output = directory.join("output.mp4");
    std::fs::write(&transcript, REPRO).unwrap();

    let generate = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=1080x1920:d=6:r=30",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-y",
        ])
        .arg(&video)
        .status()
        .expect("ffmpeg must be installed for the visual e2e test");
    assert!(generate.success(), "failed to generate the black fixture video");

    let karaoke = run(&["karaoke", transcript.to_str().unwrap(), "-o", captions.to_str().unwrap()]);
    assert!(karaoke.status.success(), "{}", String::from_utf8_lossy(&karaoke.stderr));

    let burn = run(&[
        "burn",
        video.to_str().unwrap(),
        captions.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--encoder-option",
        "crf=12",
    ]);
    assert!(burn.status.success(), "{}", String::from_utf8_lossy(&burn.stderr));
    let caption_text = std::fs::read_to_string(&captions).unwrap();

    // Midpoint of every word (start = i*0.38, duration = 0.34 s). This covers
    // words on both lines of the two-line pages too.
    for index in 0..14 {
        let midpoint = f64::from(index) * 0.38 + 0.17;
        let frame = extract_frame(&output, midpoint);

        // (a) The active word is painted in the hormozi accent yellow. The
        // caption box is centred on \pos(540,1500) and at most two lines tall.
        let mut accent = 0usize;
        let mut white = 0usize;
        for y in 1_150..1_560 {
            for x in 60..1_020 {
                let (red, green, blue) = pixel(&frame, x, y);
                if red > 170 && green > 140 && blue < 90 {
                    accent += 1;
                }
                if red > 200 && green > 200 && blue > 200 {
                    white += 1;
                }
            }
        }
        assert!(
            white + accent > 100,
            "no caption at t={midpoint}: {} bright pixels",
            white + accent
        );
        assert!(accent > 50, "word not highlighted at t={midpoint}: {accent} accent pixels");
        if words_on_line_at(&caption_text, midpoint) > 1 {
            // A multi-word page must not inherit the active colour: that
            // regression paints the entire line and leaves zero white pixels.
            assert!(white > 100, "highlight leaked at t={midpoint}: {white} white pixels");
        }

        // (b) Nothing rendered may reach the outer 2% of the frame.
        let edge = WIDTH * 2 / 100;
        let mut edge_pixels = 0usize;
        for y in 0..HEIGHT {
            for x in 0..edge {
                let (red, green, blue) = pixel(&frame, x, y);
                if u16::from(red) + u16::from(green) + u16::from(blue) > 90 {
                    edge_pixels += 1;
                }
            }
            for x in (WIDTH - edge)..WIDTH {
                let (red, green, blue) = pixel(&frame, x, y);
                if u16::from(red) + u16::from(green) + u16::from(blue) > 90 {
                    edge_pixels += 1;
                }
            }
        }
        assert_eq!(edge_pixels, 0, "caption touched the frame edge at t={midpoint}");
    }
}
