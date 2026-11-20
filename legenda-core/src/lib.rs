//! Subtitle parsing, writing, conversion and exact timing primitives.
//!
//! `legenda-core` is the library behind the `legenda` CLI. It deliberately
//! implements encoding detection in-house and stores all timings as exact
//! rationals so that format conversions and framerate rescaling never lose
//! information to floating point rounding.

#![deny(warnings)]

pub mod ass;
pub mod convert;
pub mod encoding;
pub mod error;
pub mod model;
pub mod srt;
pub mod time;
pub mod vtt;

mod text_util;

pub use error::{Error, Result};
pub use model::{Format, PlainCue, Subtitle};
pub use time::{Rational, Time};
