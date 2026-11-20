//! Generous performance bound for bulk SRT parsing and writing.
//!
//! The criterion benchmark in `benches/karaoke.rs` reports precise timings;
//! this test only guards against a gross regression and runs in debug builds.

use std::time::Instant;

use legenda_core::srt;

#[test]
fn parse_and_write_100k_cues_stays_well_bounded() {
    let mut input = String::with_capacity(5_000_000);
    for index in 0..100_000usize {
        let second = (index * 2) as u64;
        input.push_str(&format!(
            "{}\n{:02}:{:02}:{:02},000 --> {:02}:{:02}:{:02},500\nword{}\n\n",
            index + 1,
            second / 3_600,
            (second / 60) % 60,
            second % 60,
            (second + 1) / 3_600,
            ((second + 1) / 60) % 60,
            (second + 1) % 60,
            index
        ));
    }

    let started = Instant::now();
    let document = srt::parse(&input).expect("generated SRT is valid");
    assert_eq!(document.cues.len(), 100_000);
    let written = srt::write(&document);
    assert_eq!(srt::parse(&written).expect("written SRT is valid").cues.len(), 100_000);
    let elapsed = started.elapsed();
    // SPEC target: under one second for parse + write of 100k cues in an
    // optimized build. Debug builds are roughly ten times slower, so they
    // keep a generous regression bound instead of a flaky one-second limit.
    if cfg!(debug_assertions) {
        assert!(
            elapsed.as_secs() < 30,
            "parse+write of 100k cues took {elapsed:?}; expected well under 30 s in debug"
        );
    } else {
        assert!(
            elapsed.as_secs() < 1,
            "parse+write of 100k cues took {elapsed:?}; expected under 1 s in release"
        );
    }
}
