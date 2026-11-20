//! ffmpeg invocation helpers for burning ASS captions into video.
//!
//! The `subtitles=` filter takes its filename through two parsing layers
//! (the filtergraph tokenizer and the filter's option parser). Colons —
//! unavoidable in Windows drive letters — therefore need two backslashes,
//! while the other filtergraph metacharacters need one. Paths are normalised
//! to forward slashes so Windows backslashes never enter the expression.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::error::{parse_error, Error, Result};

/// A ready-to-use `subtitles=...` filter expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubtitlesFilter {
    /// The original caption path.
    pub path: PathBuf,
    /// The escaped ffmpeg filter expression.
    pub expression: String,
}

impl SubtitlesFilter {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        Ok(Self {
            path: path.to_path_buf(),
            expression: format!("subtitles={}", escape_filter_path(path)?),
        })
    }

    pub fn as_str(&self) -> &str {
        &self.expression
    }
}

impl fmt::Display for SubtitlesFilter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.expression)
    }
}

/// Options for a burn invocation. The encoder name and its options are passed
/// straight through, allowing hardware encoders such as `h264_nvenc`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BurnOptions {
    pub ffmpeg: PathBuf,
    pub encoder: String,
    pub encoder_options: Vec<(String, String)>,
}

impl Default for BurnOptions {
    fn default() -> Self {
        Self {
            ffmpeg: PathBuf::from("ffmpeg"),
            encoder: "libx264".to_owned(),
            encoder_options: Vec::new(),
        }
    }
}

impl BurnOptions {
    /// Parses an encoder option in `key=value` form into an ffmpeg flag pair.
    pub fn parse_encoder_option(input: &str) -> Result<(String, String)> {
        let (key, value) = input.split_once('=').ok_or_else(|| {
            parse_error("encoder option", format!("expected key=value, got {input:?}"))
        })?;
        if key.is_empty()
            || !key.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(parse_error("encoder option", format!("invalid option name {key:?}")));
        }
        Ok((key.to_owned(), value.to_owned()))
    }
}

/// Builds the full ffmpeg argument vector for a burn.
pub fn build_args(
    video: &Path,
    captions: &Path,
    output: &Path,
    options: &BurnOptions,
) -> Result<Vec<OsString>> {
    let filter = SubtitlesFilter::new(captions)?;
    let mut args: Vec<OsString> = vec![
        OsString::from("-hide_banner"),
        OsString::from("-y"),
        OsString::from("-i"),
        video.as_os_str().to_owned(),
        OsString::from("-map"),
        OsString::from("0:v:0"),
        OsString::from("-map"),
        OsString::from("0:a?"),
        OsString::from("-vf"),
        OsString::from(filter.as_str()),
        OsString::from("-c:v"),
        OsString::from(&options.encoder),
    ];
    for (key, value) in &options.encoder_options {
        args.push(OsString::from(format!("-{key}")));
        args.push(OsString::from(value));
    }
    args.push(OsString::from("-c:a"));
    args.push(OsString::from("copy"));
    args.push(OsString::from("-movflags"));
    args.push(OsString::from("+faststart"));
    args.push(output.as_os_str().to_owned());
    Ok(args)
}

/// Escapes a filesystem path for use as the value of a filtergraph option.
///
/// * Windows separators become `/`, removing the need to escape them.
/// * `:` becomes `\\:` (two levels of option parsing).
/// * `,`, `;`, `[` and `]` become `\,`, `\;`, `\[` and `\]`.
/// * `'` becomes `\\\'` — the quote survives both parsing layers.
/// * A literal backslash on Unix becomes `\\\\` (one per parsing level).
pub fn escape_filter_path(path: &Path) -> Result<String> {
    let text = path.to_str().ok_or_else(|| Error::NonUtf8Path(path.to_path_buf()))?;
    let mut output = String::with_capacity(text.len() + 8);
    for character in text.chars() {
        match character {
            ':' => output.push_str("\\\\:"),
            '\\' if cfg!(windows) => output.push('/'),
            '\\' => output.push_str("\\\\\\\\"),
            ',' => output.push_str("\\,"),
            ';' => output.push_str("\\;"),
            '[' => output.push_str("\\["),
            ']' => output.push_str("\\]"),
            '\'' => output.push_str("\\\\\\'"),
            _ => output.push(character),
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_drive_colons_and_spaces() {
        // A drive letter keeps its colon on every platform; on Windows the
        // native separators are normalised to forward slashes as well.
        let path = Path::new("C:/Users/me/my subs/captions.ass");
        assert_eq!(
            SubtitlesFilter::new(path).unwrap().as_str(),
            r"subtitles=C\\:/Users/me/my subs/captions.ass"
        );
        if cfg!(windows) {
            assert_eq!(
                escape_filter_path(Path::new(r"C:\Users\me\captions.ass")).unwrap(),
                r"C\\:/Users/me/captions.ass"
            );
        }
    }

    #[test]
    fn escapes_unix_filtergraph_metacharacters() {
        let path = Path::new("/tmp/a,b;[c]'d/e.ass");
        assert_eq!(escape_filter_path(path).unwrap(), r"/tmp/a\,b\;\[c\]\\\'d/e.ass");
    }

    #[cfg(not(windows))]
    #[test]
    fn escapes_literal_backslashes_on_unix() {
        assert_eq!(escape_filter_path(Path::new(r"/tmp/a\b.ass")).unwrap(), r"/tmp/a\\\\b.ass");
    }

    #[test]
    fn builds_complete_arg_vector() {
        let options = BurnOptions {
            encoder: "h264_nvenc".to_owned(),
            encoder_options: vec![("preset".to_owned(), "p4".to_owned())],
            ..BurnOptions::default()
        };
        let args =
            build_args(Path::new("in.mp4"), Path::new("cap.ass"), Path::new("out.mp4"), &options)
                .unwrap();
        let rendered: Vec<String> =
            args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(rendered[rendered.len() - 1], "out.mp4");
        assert!(rendered.contains(&"subtitles=cap.ass".to_owned()));
        assert!(rendered.contains(&"-preset".to_owned()));
        assert!(rendered.contains(&"p4".to_owned()));
        assert!(rendered.contains(&"h264_nvenc".to_owned()));
    }

    #[test]
    fn rejects_malformed_encoder_options() {
        assert!(BurnOptions::parse_encoder_option("preset").is_err());
        assert!(BurnOptions::parse_encoder_option("=p4").is_err());
        assert_eq!(
            BurnOptions::parse_encoder_option("preset=p4").unwrap(),
            ("preset".into(), "p4".into())
        );
    }
}
