//! Subtitle parsing, writing, conversion and exact timing primitives.
//!
//! `legenda-core` is the library behind the `legenda` CLI. It deliberately
//! implements encoding detection in-house and stores all timings as exact
//! rationals so that format conversions and framerate rescaling never lose
//! information to floating point rounding.

#![deny(warnings)]

pub mod ass;
pub mod burn;
pub mod convert;
pub mod encoding;
pub mod error;
pub mod karaoke;
pub mod lint;
pub mod model;
pub mod ops;
pub mod reflow;
pub mod srt;
pub mod time;
pub mod transcript;
pub mod vtt;
pub mod warnings;

mod text_util;

pub use burn::{BurnOptions, LibassFilters, SubtitlesFilter};
pub use error::{Error, Result};
pub use karaoke::{KaraokeOptions, KaraokePreset};
pub use lint::{LintConfig, LintIssue, LintReport, LintRule};
pub use model::{Format, PlainCue, Subtitle};
pub use ops::FrameRate;
pub use reflow::Preset as ReflowPreset;
pub use time::{Rational, Time};
pub use transcript::{PageOptions, Transcript, TranscriptFormat, Word};
