//! Known-vector and property tests for milestone-2 operations.

use legenda_core::lint::{LintConfig, LintReport, LintRule};
use legenda_core::ops::{self, clamp, scale, shift, sort_and_fix_gaps, sync, FrameRate};
use legenda_core::reflow::{self, Preset, ReflowOptions};
use legenda_core::{Error, PlainCue, Rational, Time};
use proptest::prelude::*;

fn cue(start_ms: i64, end_ms: i64, text: &str) -> PlainCue {
    PlainCue {
        start: Time::from_millis(start_ms),
        end: Time::from_millis(end_ms),
        text: text.to_owned(),
    }
}

fn short_cue() -> impl Strategy<Value = PlainCue> {
    let words = prop::sample::select(vec![
        "olá", "mundo", "casa", "azul", "rápido", "que", "de", "a", "o", "não", "falou", "hoje",
        "tudo", "bem", "isso", "para", "com", "filme", "ontem", "gostou",
    ]);
    (0u64..600_000, 1u64..4_000, prop::collection::vec(words, 1..9)).prop_map(
        |(start, length, words)| cue(start as i64, (start + length) as i64, &words.join(" ")),
    )
}

fn cue_set() -> impl Strategy<Value = Vec<PlainCue>> {
    prop::collection::vec(short_cue(), 1..8)
}

fn netflix_options() -> ReflowOptions {
    ReflowOptions::new(Preset::NetflixPtBr, FrameRate::FPS_24).unwrap()
}

/// Reflow sorts cues chronologically, so word preservation is checked as a
/// multiset: every input word must appear the same number of times in output.
fn words(cues: &[PlainCue]) -> Vec<String> {
    let mut result: Vec<String> = cues.iter().flat_map(|cue| ops::word_tokens(&cue.text)).collect();
    result.sort();
    result
}

#[test]
fn known_vectors_for_every_operation() {
    let mut cues = vec![cue(1_000, 2_000, "a"), cue(3_000, 4_000, "b")];

    shift(&mut cues, Time::from_millis(500)).unwrap();
    assert_eq!(cues[0].start, Time::from_millis(1_500));

    scale(&mut cues, FrameRate::FPS_24, FrameRate::FPS_25).unwrap();
    // 1500 ms at 24 fps becomes 1440 ms at 25 fps (factor 24/25).
    assert_eq!(cues[0].start, Time::from_millis(1_440));

    sync(
        &mut cues,
        Time::from_millis(0),
        Time::from_millis(0),
        Time::from_rational_seconds(Rational::new(10, 1).unwrap()),
        Time::from_rational_seconds(Rational::new(20, 1).unwrap()),
    )
    .unwrap();
    assert_eq!(cues[0].start, Time::from_millis(2_880));

    clamp(&mut cues, Time::from_millis(3_000), Time::from_millis(7_500)).unwrap();
    assert_eq!(cues.len(), 2);
    assert_eq!(cues[0].start, Time::from_millis(3_000));
    assert_eq!(cues[1].end, Time::from_millis(7_500));

    assert!(matches!(
        sync(
            &mut cues,
            Time::from_millis(1_000),
            Time::ZERO,
            Time::from_millis(1_000),
            Time::from_millis(2_000),
        ),
        Err(Error::DegenerateSync)
    ));
    assert!(clamp(&mut cues, Time::from_millis(10), Time::from_millis(1)).is_err());
}

#[test]
fn text_removal_known_vectors() {
    let mut cues = vec![
        cue(0, 1_000, "[MÚSICA] Olá, tudo bem?"),
        cue(1_000, 2_000, "(risos)"),
        cue(2_000, 3_000, "JOÃO: Como vai?"),
        cue(3_000, 4_000, "- MARIA: Bem, obrigada."),
    ];
    ops::strip_hearing_impaired(&mut cues);
    assert_eq!(cues.len(), 3);
    assert_eq!(cues[0].text, "Olá, tudo bem?");
    ops::strip_speaker_labels(&mut cues);
    assert_eq!(cues[1].text, "Como vai?");
    assert_eq!(cues[2].text, "- Bem, obrigada.");
}

proptest! {
    #[test]
    fn shift_then_unshift_is_identity(mut cues in cue_set(), millis in -3_600_000i64..3_600_000) {
        let delta = Time::from_millis(millis);
        let inverse = Time::from_millis(-millis);
        let original = cues.clone();
        shift(&mut cues, delta)?;
        shift(&mut cues, inverse)?;
        prop_assert_eq!(cues, original);
    }

    #[test]
    fn scale_round_trip_is_identity(mut cues in cue_set()) {
        let original = cues.clone();
        scale(&mut cues, FrameRate::FPS_23_976, FrameRate::FPS_25)?;
        scale(&mut cues, FrameRate::FPS_25, FrameRate::FPS_23_976)?;
        prop_assert_eq!(cues, original);
    }

    #[test]
    fn sync_then_inverse_is_identity(mut cues in cue_set()) {
        let original = cues.clone();
        let a = Time::from_millis(1_000);
        let b = Time::from_millis(5_000);
        let mapped_a = Time::from_millis(500);
        let mapped_b = Time::from_millis(7_500);
        sync(&mut cues, a, mapped_a, b, mapped_b)?;
        sync(&mut cues, mapped_a, a, mapped_b, b)?;
        prop_assert_eq!(cues, original);
    }

    #[test]
    fn fix_ops_leave_sorted_nonoverlapping_cues(mut cues in cue_set()) {
        let min_gap = FrameRate::FPS_24.frame_duration(2)?;
        sort_and_fix_gaps(&mut cues, min_gap)?;
        for pair in cues.windows(2) {
            prop_assert!(pair[0].start <= pair[1].start);
            let gap = pair[1].start.as_rational() - pair[0].end.as_rational();
            prop_assert!(gap >= min_gap.as_rational());
        }
    }

    #[test]
    fn reflow_never_loses_words(mut cues in cue_set()) {
        let before = words(&cues);
        reflow::reflow(&mut cues, &netflix_options())?;
        prop_assert_eq!(words(&cues), before);
    }

    #[test]
    fn reflow_respects_line_and_cps_limits(mut cues in cue_set()) {
        reflow::reflow(&mut cues, &netflix_options())?;
        let config = LintConfig::netflix_ptbr(FrameRate::FPS_24, false)?;
        for detected in LintReport::new(&cues, &config).issues {
            prop_assert!(!matches!(
                detected.rule,
                LintRule::Cps
                    | LintRule::LineLength
                    | LintRule::TooManyLines
                    | LintRule::Overlap
                    | LintRule::Gap
            ));
        }
        prop_assert!(cues.windows(2).all(|pair| pair[0].start <= pair[1].start));
    }

    #[test]
    fn shorts_reflow_keeps_one_line_per_page(mut cues in cue_set()) {
        let options = ReflowOptions::new(Preset::Shorts, FrameRate::FPS_24)?;
        let before = words(&cues);
        reflow::reflow(&mut cues, &options)?;
        prop_assert_eq!(words(&cues), before);
        prop_assert!(cues.iter().all(|cue| !cue.text.contains('\n')));
    }
}
