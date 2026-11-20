//! Criterion benchmarks for word-level transcript ingestion and rendering.
//!
//! Also measures the spec's parse+write bound for 100k SRT cues.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use legenda_core::karaoke::{render, KaraokeOptions, KaraokePreset};
use legenda_core::{srt, transcript};
use serde_json::{json, Value};

fn whisperx_json(segments: usize, words_per_segment: usize) -> String {
    let segments: Vec<Value> = (0..segments)
        .map(|segment| {
            let words: Vec<Value> = (0..words_per_segment)
                .map(|index| {
                    let offset = segment as f64 * 5.0 + index as f64 * 0.4;
                    json!({
                        "word": format!(" word{index}"),
                        "start": offset,
                        "end": offset + 0.35,
                        "score": 0.98,
                    })
                })
                .collect();
            json!({
                "start": segment as f64 * 5.0,
                "end": segment as f64 * 5.0 + 5.0,
                "text": "words",
                "words": words,
            })
        })
        .collect();
    json!({ "segments": segments }).to_string()
}

fn srt_with_cues(count: usize) -> String {
    let mut output = String::with_capacity(count * 48);
    for index in 0..count {
        let start = index as u64 * 2;
        let end = start + 1;
        output.push_str(&format!(
            "{}\n{:02}:{:02}:{:02},000 --> {:02}:{:02}:{:02},500\nword{}\n\n",
            index + 1,
            start / 3_600,
            (start / 60) % 60,
            start % 60,
            end / 3_600,
            (end / 60) % 60,
            end % 60,
            index
        ));
    }
    output
}

fn benchmarks(criterion: &mut Criterion) {
    let json = whisperx_json(20, 10);
    let parsed = transcript::parse_transcript(&json).expect("benchmark fixture is valid");

    criterion.bench_function("transcript_parse_whisperx_200_words", |b| {
        b.iter(|| transcript::parse_transcript(black_box(&json)).expect("valid fixture"))
    });

    for preset in KaraokePreset::ALL {
        let options = KaraokeOptions::new(preset);
        criterion.bench_function(preset.name(), |b| {
            b.iter(|| render(black_box(&parsed.words), black_box(&options)).expect("valid render"))
        });
    }

    let large_srt = srt_with_cues(100_000);
    criterion.bench_function("srt_parse_write_100k_cues", |b| {
        b.iter(|| {
            let document = srt::parse(black_box(&large_srt)).expect("valid fixture");
            srt::write(black_box(&document))
        })
    });
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
