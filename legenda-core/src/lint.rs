//! Rule-based subtitle linting with text and JSON reports.
//!
//! Default thresholds come from the official Netflix style guides cited in
//! `docs/netflix-ptbr-style-guide.md`: 42 characters per line, two lines
//! maximum, 17 CPS for adults (13 for children), a 5/6 s minimum and a 7 s
//! maximum duration. The two-frame minimum gap comes from `SPEC.md`.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::error::Result;
use crate::model::PlainCue;
use crate::ops::{visible_char_count, FrameRate};
use crate::time::{Rational, Time};

/// A single lint rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LintRule {
    InvalidTiming,
    Cps,
    Overlap,
    Gap,
    TooShort,
    TooLong,
    LineLength,
    TooManyLines,
}

impl LintRule {
    pub const fn label(self) -> &'static str {
        match self {
            Self::InvalidTiming => "invalid_timing",
            Self::Cps => "cps",
            Self::Overlap => "overlap",
            Self::Gap => "gap",
            Self::TooShort => "too_short",
            Self::TooLong => "too_long",
            Self::LineLength => "line_length",
            Self::TooManyLines => "too_many_lines",
        }
    }
}

/// One detected violation. `value` and `limit` are expressed in the unit of
/// the rule (CPS, seconds or characters) for machine-readable reports.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LintIssue {
    /// One-based cue number, matching the SRT index in the source file.
    pub cue: usize,
    pub rule: LintRule,
    pub message: String,
    pub value: f64,
    pub limit: f64,
}

/// Thresholds used by [`lint`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LintConfig {
    pub fps: FrameRate,
    pub max_chars_per_line: usize,
    pub max_lines: usize,
    pub max_cps: Rational,
    pub min_duration: Time,
    pub max_duration: Time,
    pub min_gap: Time,
}

impl LintConfig {
    /// Netflix Brazilian Portuguese values (General Requirements §1 plus
    /// language guide I.2/I.11/I.14). The gap is two frames at `fps`.
    pub fn netflix_ptbr(fps: FrameRate, children: bool) -> Result<Self> {
        Ok(Self {
            fps,
            max_chars_per_line: 42,
            max_lines: 2,
            max_cps: Rational::new(if children { 13 } else { 17 }, 1)?,
            min_duration: Rational::new(5, 6).map(Time::from_rational_seconds)?,
            max_duration: Time::from_rational_seconds(Rational::from_int(7)),
            min_gap: fps.frame_duration(2)?,
        })
    }

    /// Vertical-shorts values matching the `shorts` reflow preset: one line,
    /// at most 34 characters and 20 CPS. Duration limits and the two-frame gap
    /// stay at the Netflix baseline so the same checks apply to both workflows.
    pub fn shorts(fps: FrameRate) -> Result<Self> {
        Ok(Self {
            fps,
            max_chars_per_line: 34,
            max_lines: 1,
            max_cps: Rational::new(20, 1)?,
            min_duration: Rational::new(5, 6).map(Time::from_rational_seconds)?,
            max_duration: Time::from_rational_seconds(Rational::from_int(7)),
            min_gap: fps.frame_duration(2)?,
        })
    }
}

fn issue(
    cue: usize,
    rule: LintRule,
    message: impl Into<String>,
    value: f64,
    limit: f64,
) -> LintIssue {
    LintIssue { cue, rule, message: message.into(), value, limit }
}

/// Lints cues in document order and returns all violations.
pub fn lint(cues: &[PlainCue], config: &LintConfig) -> Vec<LintIssue> {
    let mut issues = Vec::new();
    let mut previous_end: Option<Time> = None;

    for (position, cue) in cues.iter().enumerate() {
        let number = position + 1;
        if cue.end <= cue.start {
            issues.push(issue(
                number,
                LintRule::InvalidTiming,
                "cue ends before or when it starts",
                cue.end.as_millis().to_f64(),
                cue.start.as_millis().to_f64(),
            ));
            continue;
        }

        let duration = cue.end.as_rational().checked_sub(cue.start.as_rational());
        let Ok(duration) = duration else {
            continue;
        };
        let seconds = duration.to_f64();
        if duration < config.min_duration.as_rational() {
            issues.push(issue(
                number,
                LintRule::TooShort,
                "cue is shorter than the minimum duration",
                seconds,
                config.min_duration.as_secs_f64(),
            ));
        }
        if duration > config.max_duration.as_rational() {
            issues.push(issue(
                number,
                LintRule::TooLong,
                "cue is longer than the maximum duration",
                seconds,
                config.max_duration.as_secs_f64(),
            ));
        }

        let characters = visible_char_count(&cue.text);
        if let Ok(cps) = Rational::from_int(characters as i64).checked_div(duration) {
            if cps > config.max_cps {
                issues.push(issue(
                    number,
                    LintRule::Cps,
                    "reading speed exceeds the CPS limit",
                    cps.to_f64(),
                    config.max_cps.to_f64(),
                ));
            }
        }

        let lines: Vec<&str> = cue.text.lines().collect();
        if lines.len() > config.max_lines {
            issues.push(issue(
                number,
                LintRule::TooManyLines,
                "cue has more lines than allowed",
                lines.len() as f64,
                config.max_lines as f64,
            ));
        }
        for line in lines {
            let length = visible_char_count(line);
            if length > config.max_chars_per_line {
                issues.push(issue(
                    number,
                    LintRule::LineLength,
                    "line is longer than the character limit",
                    length as f64,
                    config.max_chars_per_line as f64,
                ));
            }
        }

        if let Some(end) = previous_end {
            if cue.start < end {
                issues.push(issue(
                    number,
                    LintRule::Overlap,
                    "cue overlaps the previous cue",
                    cue.start.as_millis().to_f64(),
                    end.as_millis().to_f64(),
                ));
            } else if let Ok(gap) = cue.start.as_rational().checked_sub(end.as_rational()) {
                if gap < config.min_gap.as_rational() {
                    issues.push(issue(
                        number,
                        LintRule::Gap,
                        "gap to the previous cue is shorter than two frames",
                        gap.to_f64(),
                        config.min_gap.as_rational().to_f64(),
                    ));
                }
            }
        }
        previous_end = Some(cue.end);
    }

    issues
}

/// Aggregated lint result, serialisable to JSON.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LintReport {
    pub cues: usize,
    pub violations: usize,
    pub by_rule: BTreeMap<LintRule, usize>,
    pub issues: Vec<LintIssue>,
}

impl LintReport {
    pub fn new(cues: &[PlainCue], config: &LintConfig) -> Self {
        let issues = lint(cues, config);
        let mut by_rule = BTreeMap::new();
        for detected in &issues {
            *by_rule.entry(detected.rule).or_insert(0) += 1;
        }
        Self { cues: cues.len(), violations: issues.len(), by_rule, issues }
    }

    pub fn has_violations(&self) -> bool {
        self.violations > 0
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Renders a compact human-readable report.
    pub fn to_text(&self) -> String {
        let mut output = format!("cues: {}\nviolations: {}\n", self.cues, self.violations);
        for detected in &self.issues {
            output.push_str(&format!(
                "cue {}: {} — {} (value: {:.3}, limit: {:.3})\n",
                detected.cue,
                detected.rule.label(),
                detected.message,
                detected.value,
                detected.limit
            ));
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(start: i64, end: i64, text: &str) -> PlainCue {
        PlainCue {
            start: Time::from_millis(start),
            end: Time::from_millis(end),
            text: text.to_owned(),
        }
    }

    fn config() -> LintConfig {
        LintConfig::netflix_ptbr(FrameRate::FPS_24, false).unwrap()
    }

    #[test]
    fn clean_cue_has_no_violations() {
        let report = LintReport::new(&[cue(1_000, 4_000, "Olá, mundo!")], &config());
        assert!(!report.has_violations());
        assert_eq!(report.to_text(), "cues: 1\nviolations: 0\n");
    }

    #[test]
    fn detects_every_rule_with_known_vectors() {
        let long_line = "a".repeat(43);
        let cues = vec![
            cue(0, 100, &long_line),                // short, CPS, line length
            cue(90, 500, "Sobreposto"),             // overlap + gap
            cue(8_100, 9_000, "a\nb\nc"),           // too many lines
            cue(10_000, 20_000, "Texto bem longo"), // too long
            cue(21_000, 21_000, "vazio"),           // invalid timing
        ];
        let report = LintReport::new(&cues, &config());
        let rules: Vec<LintRule> = report.issues.iter().map(|item| item.rule).collect();
        assert!(rules.contains(&LintRule::Cps));
        assert!(rules.contains(&LintRule::TooShort));
        assert!(rules.contains(&LintRule::LineLength));
        assert!(rules.contains(&LintRule::Overlap));
        assert!(rules.contains(&LintRule::TooManyLines));
        assert!(rules.contains(&LintRule::TooLong));
        assert!(rules.contains(&LintRule::InvalidTiming));
    }

    #[test]
    fn json_report_is_valid_and_machine_readable() {
        let long = "a".repeat(50);
        let report = LintReport::new(&[cue(0, 100, &long)], &config());
        let json = report.to_json().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["cues"], 1);
        assert!(parsed["violations"].as_u64().unwrap() > 0);
        assert_eq!(parsed["by_rule"]["line_length"], 1);
        assert!(parsed["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["rule"] == "line_length"));
    }

    #[test]
    fn shorts_config_matches_the_shorts_reflow_preset() {
        let config = LintConfig::shorts(FrameRate::FPS_24).unwrap();
        assert_eq!(config.max_lines, 1);
        assert_eq!(config.max_chars_per_line, 34);
        assert_eq!(config.max_cps, Rational::new(20, 1).unwrap());
        assert_eq!(config.min_gap, FrameRate::FPS_24.frame_duration(2).unwrap());

        let two_lines = LintReport::new(&[cue(1_000, 4_000, "linha um\nlinha dois")], &config);
        assert!(two_lines.issues.iter().any(|item| item.rule == LintRule::TooManyLines));
    }

    #[test]
    fn gap_rule_uses_two_frames() {
        let fps = FrameRate::FPS_24;
        let config = LintConfig::netflix_ptbr(fps, false).unwrap();
        let cues = vec![cue(1_000, 2_000, "Primeira"), cue(2_050, 3_000, "Segunda")];
        let report = LintReport::new(&cues, &config);
        assert!(report.issues.iter().any(|item| item.rule == LintRule::Gap));
    }
}
