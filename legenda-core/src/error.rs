use std::fmt;

/// Errors produced by the subtitle library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A rational number was constructed with a zero denominator.
    #[error("a rational number cannot have a zero denominator")]
    ZeroDenominator,

    /// An exact arithmetic operation exceeded the representable range.
    #[error("rational arithmetic overflow")]
    ArithmeticOverflow,

    /// A timestamp string could not be parsed.
    #[error("invalid {format} time {input:?}: {reason}")]
    InvalidTime { format: &'static str, input: String, reason: &'static str },

    /// A subtitle document could not be parsed.
    #[error("failed to parse {format}: {message}")]
    Parse { format: &'static str, message: String },

    /// The subtitle format is not known.
    #[error("unsupported subtitle format: {0}")]
    UnsupportedFormat(String),

    /// A field required for an operation is missing from an ASS line.
    #[error("missing ASS field {field:?} in {kind} line")]
    MissingAssField { field: &'static str, kind: String },

    /// A merge was requested without any input documents.
    #[error("merge requires at least one input document")]
    EmptyMerge,

    /// An operation expected a different number of cues than the document has.
    #[error("cue count mismatch: expected {expected}, found {actual}")]
    CueCountMismatch { expected: usize, actual: usize },

    /// A frame-rate name or ratio could not be interpreted.
    #[error("invalid frame rate: {0}")]
    InvalidFrameRate(String),

    /// The two reference points of a two-point sync are identical.
    #[error("two-point sync requires two distinct source timestamps")]
    DegenerateSync,

    /// A free-form CLI time expression could not be interpreted.
    #[error("invalid time expression {input:?}: {reason}")]
    InvalidTimeExpression { input: String, reason: &'static str },

    /// A reflow preset name is unknown.
    #[error("unknown reflow preset: {0}")]
    UnknownPreset(String),

    /// A split boundary lies outside the subtitle's cue range.
    #[error("split boundary out of range: index {index}, cue count {count}")]
    SplitOutOfRange { index: usize, count: usize },

    /// JSON serialisation failed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Wraps an underlying I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Convenience result type for the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Builds a timestamp parse error without repeating boilerplate.
pub(crate) fn invalid_time(format: &'static str, input: &str, reason: &'static str) -> Error {
    Error::InvalidTime { format, input: input.to_owned(), reason }
}

/// Builds a document parse error.
pub(crate) fn parse_error(format: &'static str, message: impl fmt::Display) -> Error {
    Error::Parse { format, message: message.to_string() }
}
