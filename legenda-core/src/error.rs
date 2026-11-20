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
