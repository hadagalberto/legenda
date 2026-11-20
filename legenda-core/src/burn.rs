//! ffmpeg invocation helpers for burning ASS captions into video.
//!
//! # Why the caption path is copied
//!
//! The `subtitles` filter receives its filename through two escaping layers:
//! the filter-option layer (escape `\`, `'`, `:`) and the filtergraph layer
//! (escape `\`, `'`, `[`, `]`, `,`, `;`). A third layer exists only when a
//! graph is typed into a shell; `std::process::Command` passes argv elements
//! verbatim, so we never need it. See the official FFmpeg reference,
//! <https://ffmpeg.org/ffmpeg-filters.html#Notes-on-filtergraph-escaping> and
//! <https://ffmpeg.org/ffmpeg-utils.html#Quoting-and-escaping>.
//!
//! Escaping arbitrary user paths through both layers correctly is subtle and
//! has proven platform-dependent (macOS in particular). Instead of gambling
//! on it, [`build_args`] copies the caption file to a uniquely named,
//! ASCII-only temporary file and points the filter at *that*. The only
//! metacharacter that can remain is a Windows drive colon, which
//! [`escape_filter_path`] handles exactly as the two-level rules prescribe.
//!
//! The video and output paths are plain argv elements and are passed through
//! untouched — never escaped, never mangled.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{parse_error, Error, Result};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A ready-to-use `subtitles=...` filter expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubtitlesFilter {
    /// The caption path the expression points at.
    pub path: PathBuf,
    /// The escaped ffmpeg filter expression.
    pub expression: String,
}

impl SubtitlesFilter {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        Ok(Self {
            path: path.to_path_buf(),
            expression: format!("subtitles=filename={}", escape_filter_path(path)?),
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

/// Deletes a temporary directory on drop. Kept alive by [`BurnInvocation`]
/// until the ffmpeg child process has exited.
#[derive(Debug)]
struct TempCaption {
    directory: PathBuf,
}

impl Drop for TempCaption {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// A prepared ffmpeg burn.
///
/// Owns the sanitised caption copy referenced by `args`. Hold on to this
/// value for the entire lifetime of the ffmpeg process; dropping it early
/// removes the temporary caption file.
#[derive(Debug)]
pub struct BurnInvocation {
    _caption: TempCaption,
    caption: PathBuf,
    args: Vec<OsString>,
}

impl BurnInvocation {
    /// The complete ffmpeg argument vector, without the program name.
    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    /// The sanitised caption copy referenced by the filtergraph.
    pub fn caption_path(&self) -> &Path {
        &self.caption
    }
}

/// Copies a caption file into a fresh directory whose name cannot contain any
/// filtergraph metacharacters. Returns the safe path and its cleanup guard.
fn safe_caption_copy(captions: &Path) -> Result<(PathBuf, TempCaption)> {
    let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let directory = std::env::temp_dir()
        .join(format!("legenda-burn-{nanos}-{}-{sequence}", std::process::id()));
    std::fs::create_dir_all(&directory)?;
    let guard = TempCaption { directory: directory.clone() };
    let path = directory.join("captions.ass");
    if let Err(error) = std::fs::copy(captions, &path) {
        drop(guard);
        return Err(error.into());
    }
    Ok((path, guard))
}

/// Builds the full ffmpeg argument vector for a burn.
///
/// Only caption files are rewritten to a safe temporary path: `video` and
/// `output` are inserted verbatim so adversarial but legal filenames survive
/// unchanged.
pub fn build_args(
    video: &Path,
    captions: &Path,
    output: &Path,
    options: &BurnOptions,
) -> Result<BurnInvocation> {
    let (safe_captions, caption_guard) = safe_caption_copy(captions)?;
    let filter = SubtitlesFilter::new(&safe_captions)?;
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
    Ok(BurnInvocation { _caption: caption_guard, caption: safe_captions, args })
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

    fn source_caption(name: &str) -> PathBuf {
        let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!("legenda-burn-source-{sequence}"));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(name);
        std::fs::write(&path, "[Script Info]\nScriptType: v4.00+\n").unwrap();
        path
    }

    #[test]
    fn escapes_drive_colons_and_spaces() {
        // A drive letter keeps its colon on every platform; on Windows the
        // native separators are normalised to forward slashes as well.
        let path = Path::new("C:/Users/me/my subs/captions.ass");
        assert_eq!(
            SubtitlesFilter::new(path).unwrap().as_str(),
            r"subtitles=filename=C\\:/Users/me/my subs/captions.ass"
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
    fn adversarial_caption_paths_never_reach_the_filtergraph() {
        // Every filtergraph metacharacter plus an apostrophe, accents and a
        // Windows-drive-shaped component (a legal filename on Unix).
        let caption = source_caption("captions, [burn]; #1's çã.ass");
        let video = Path::new("input, [v]; #2's çã.mp4");
        let output = Path::new("output, [o]; #3's çã.mp4");
        let invocation = build_args(video, &caption, output, &BurnOptions::default()).unwrap();

        let filter = invocation
            .args()
            .iter()
            .position(|arg| arg.to_string_lossy() == "-vf")
            .and_then(|index| invocation.args().get(index + 1))
            .expect("-vf and its value");
        let filter = filter.to_string_lossy();
        assert!(filter.starts_with("subtitles=filename="), "{filter}");
        assert!(filter.contains("captions.ass"), "{filter}");
        for forbidden in [',', '[', ']', ';', '#', '\'', 'ç', 'ã'] {
            assert!(!filter.contains(forbidden), "filtergraph leaked {forbidden:?}: {filter}");
        }
        assert_ne!(Path::new(filter.trim_start_matches("subtitles=filename=")), caption);

        // Video and output are plain argv elements: byte-for-byte unchanged.
        assert_eq!(invocation.args()[3].as_os_str(), video.as_os_str());
        assert_eq!(invocation.args().last().unwrap().as_os_str(), output.as_os_str());
    }

    #[test]
    fn sanitised_copy_is_removed_when_invocation_is_dropped() {
        let caption = source_caption("cap.ass");
        let invocation = build_args(
            Path::new("in.mp4"),
            &caption,
            Path::new("out.mp4"),
            &BurnOptions::default(),
        )
        .unwrap();
        let safe_path = invocation.caption_path().to_path_buf();
        assert!(invocation.args()[9].to_string_lossy().starts_with("subtitles=filename="));
        assert!(safe_path.exists(), "temporary caption copy must exist while burning");
        assert_eq!(std::fs::read(&safe_path).unwrap(), std::fs::read(&caption).unwrap());
        drop(invocation);
        assert!(!safe_path.exists(), "temporary caption copy must be cleaned up");
    }

    #[test]
    fn builds_complete_arg_vector() {
        let caption = source_caption("cap.ass");
        let options = BurnOptions {
            encoder: "h264_nvenc".to_owned(),
            encoder_options: vec![("preset".to_owned(), "p4".to_owned())],
            ..BurnOptions::default()
        };
        let invocation =
            build_args(Path::new("in.mp4"), &caption, Path::new("out.mp4"), &options).unwrap();
        let rendered: Vec<String> =
            invocation.args().iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(rendered[rendered.len() - 1], "out.mp4");
        assert!(rendered.iter().any(|arg| arg.starts_with("subtitles=filename=")));
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
