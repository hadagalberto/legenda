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
    // Up to 60 words (~480 visible characters) so the generator exercises
    // cues that must be split by text length, not only short one-liners.
    (0u64..600_000, 1u64..4_000, prop::collection::vec(words, 1..61)).prop_map(
        |(start, length, words)| cue(start as i64, (start + length) as i64, &words.join(" ")),
    )
}

fn cue_set() -> impl Strategy<Value = Vec<PlainCue>> {
    prop::collection::vec(short_cue(), 1..8)
}

/// Cue starts stay at or beyond 2 s so that shifting by at most -2 s cannot
/// cross the zero boundary. Needed for the shift/unshift identity property:
/// clamping at zero is intentionally lossy, so identity only holds while the
/// whole timeline remains non-negative.
fn clamped_cue_set() -> impl Strategy<Value = Vec<PlainCue>> {
    let words = prop::sample::select(vec![
        "olá", "mundo", "casa", "azul", "rápido", "que", "de", "a", "o", "não", "falou", "hoje",
        "tudo", "bem", "isso", "para", "com", "filme", "ontem", "gostou",
    ]);
    let single = (2_000u64..600_000, 1u64..4_000, prop::collection::vec(words, 1..9)).prop_map(
        |(start, length, words)| cue(start as i64, (start + length) as i64, &words.join(" ")),
    );
    prop::collection::vec(single, 1..8)
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
fn sync_with_reference_points_in_descending_order_is_the_identity_they_describe() {
    // Regression for the Rational::checked_div sign bug: with the reference
    // points given in descending order (from_b < from_a and to_b < to_a the
    // pair is (30 s -> 30 s, 10 s -> 10 s)) the correct slope is
    // (10 - 30) / (10 - 30) = +1 (hand-derived), i.e. the identity. The old
    // checked_div flipped the sign because the dividend was negative, so
    // every cue was mirrored around 30 s: a cue at 30-31 s came out as
    // "00:00:30,000 --> 00:00:29,000" — end before start.
    let mut cues = vec![cue(5_000, 6_000, "a"), cue(30_000, 31_000, "b")];
    sync(
        &mut cues,
        Time::from_millis(30_000),
        Time::from_millis(30_000),
        Time::from_millis(10_000),
        Time::from_millis(10_000),
    )
    .unwrap();
    assert_eq!(cues[0].start, Time::from_millis(5_000));
    assert_eq!(cues[0].end, Time::from_millis(6_000));
    assert_eq!(cues[1].start, Time::from_millis(30_000));
    assert_eq!(cues[1].end, Time::from_millis(31_000));
}

#[test]
fn shift_drops_or_clamps_cues_that_leave_the_timeline() {
    // Review finding: shifting into negative time used to write timestamps
    // like "-00:00:05,000" that legenda itself refuses to parse back.
    // Policy (mirrors what every writer can represent): cues that end
    // before zero are dropped; cues that straddle zero are clamped to it.
    // Hand-derived: shifting -12 s moves the three cues below by -12 s.
    let mut cues = vec![
        cue(5_000, 7_000, "dropped"),  // ends at -5 s: discarded
        cue(8_000, 12_000, "clamped"), // starts at -4 s: clamped to 0
        cue(13_000, 15_000, "kept"),   // 1 s to 3 s: untouched
    ];
    let warnings = shift(&mut cues, Time::from_millis(-12_000)).unwrap();
    assert_eq!(cues.len(), 2, "one cue is dropped");
    assert_eq!(cues[0].text, "clamped");
    assert_eq!(cues[0].start, Time::ZERO);
    assert_eq!(cues[0].end, Time::from_millis(1_000));
    assert_eq!(cues[1].start, Time::from_millis(1_000));
    assert_eq!(cues[1].end, Time::from_millis(3_000));
    assert_eq!(warnings.len(), 2, "one drop + one clamp warning: {warnings:?}");
    assert!(warnings.iter().any(|warning| warning.contains("dropped")), "{warnings:?}");
    assert!(warnings.iter().any(|warning| warning.contains("clamped")), "{warnings:?}");
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
    fn shift_then_unshift_is_identity(
        mut cues in clamped_cue_set(),
        millis in -2_000i64..3_600_000,
    ) {
        // Cue starts are at least 2 s and shifts never go below -2 s, so no
        // timestamp ever leaves the non-negative timeline. Shifting below
        // zero is intentionally lossy: the operation clamps to zero (and
        // drops cues that end before it), so exact identity only holds while
        // the whole shifted timeline stays non-negative.
        let delta = Time::from_millis(millis);
        let inverse = Time::from_millis(-millis);
        let original = cues.clone();
        shift(&mut cues, delta)?;
        shift(&mut cues, inverse)?;
        prop_assert_eq!(cues, original);
    }

    #[test]
    fn shift_never_produces_negative_timestamps(
        mut cues in cue_set(),
        millis in -3_600_000i64..3_600_000,
    ) {
        shift(&mut cues, Time::from_millis(millis))?;
        for cue in &cues {
            prop_assert!(cue.start >= Time::ZERO);
            prop_assert!(cue.end > Time::ZERO);
        }
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
        // Reference points chosen so no cue ever maps below zero: since the
        // operations clamp negative results to zero (see
        // `shift_drops_or_clamps_cues_that_leave_the_timeline`), an identity
        // round trip is only guaranteed while the mapped timeline stays
        // non-negative.
        let original = cues.clone();
        let a = Time::from_millis(1_000);
        let b = Time::from_millis(5_000);
        let mapped_a = Time::from_millis(5_000);
        let mapped_b = Time::from_millis(20_000);
        sync(&mut cues, a, mapped_a, b, mapped_b)?;
        sync(&mut cues, mapped_a, a, mapped_b, b)?;
        prop_assert_eq!(cues, original);
    }

    /// Two-point sync must map each reference point exactly, no matter which
    /// of the two comes first in the source or the target timeline. The old
    /// Rational::checked_div had a sign error for negative dividends, so any
    /// pair whose target times ran in descending order mirrored the whole
    /// timeline.
    #[test]
    fn sync_maps_reference_points_exactly_in_any_order(
        from_a in 0i64..60_000,
        from_gap in -30_000i64..30_000,
        to_a in 0i64..60_000,
        to_gap in -30_000i64..30_000,
    ) {
        prop_assume!(from_gap != 0);
        prop_assume!(to_a + to_gap >= 0);
        let from_b = from_a + from_gap;
        let to_b = to_a + to_gap;
        // A descending target timeline has a negative slope; a reference
        // cue's mapped *end* can then fall below zero and be (correctly)
        // dropped by the timeline clamp. Restrict this property to mappings
        // where both 100 ms reference cues stay on the representable timeline.
        let slope = (to_b - to_a) as f64 / from_gap as f64;
        prop_assume!(to_a as f64 + slope * 100.0 >= 0.0);
        prop_assume!(to_b as f64 + slope * 100.0 >= 0.0);
        let mut cues = vec![
            cue(from_a, from_a + 100, "reference a"),
            cue(from_b, from_b + 100, "reference b"),
        ];
        sync(
            &mut cues,
            Time::from_millis(from_a),
            Time::from_millis(to_a),
            Time::from_millis(from_b),
            Time::from_millis(to_b),
        )?;
        prop_assert_eq!(cues[0].start, Time::from_millis(to_a));
        prop_assert_eq!(cues[1].start, Time::from_millis(to_b));
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
        let config = LintConfig::netflix_ptbr(FrameRate::FPS_24, false)?;
        // Physically impossible inputs (a 150-character cue lasting 1 ms with
        // another cue 84 ms later) cannot be made clean without either moving
        // the later cue or dropping words. For those, reflow keeps the hard
        // line/duration limits and leaves timing violations to `lint`.
        let feasible = LintReport::new(&cues, &config)
            .issues
            .iter()
            .all(|issue| issue.rule == LintRule::Cps || issue.rule == LintRule::TooShort);

        reflow::reflow(&mut cues, &netflix_options())?;
        for detected in LintReport::new(&cues, &config).issues {
            // Line, duration and line-count limits are never negotiable: the
            // review finding is precisely that reflow used to overflow them.
            prop_assert!(!matches!(
                detected.rule,
                LintRule::LineLength | LintRule::TooManyLines | LintRule::TooLong
            ));
            // Design finding: a CPS extension may use only the gap before the
            // next source cue. When that gap is physically too small, later
            // cues must not be moved and `lint` reports the violation instead
            // of a seconds-long desync. The old assertion banned CPS errors
            // unconditionally and thereby encoded the desynchronising bug.
            if feasible {
                prop_assert!(!matches!(
                    detected.rule,
                    LintRule::Overlap | LintRule::Gap
                ));
            }
        }
        // The exact "never move a later source cue" assertion lives in the
        // dedicated `cps_extension_never_moves_a_later_cue_start` regression.
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
