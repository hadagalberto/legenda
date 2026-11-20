//! The `legenda` command line interface.

#![deny(warnings)]

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use clap::{Parser, Subcommand};
use legenda_core::burn::{self, BurnOptions};
use legenda_core::encoding;
use legenda_core::error::Result;
use legenda_core::karaoke::{self, KaraokeOptions, KaraokePreset};
use legenda_core::lint::{LintConfig, LintReport};
use legenda_core::ops::{self, FrameRate};
use legenda_core::reflow::{self, Preset};
use legenda_core::time::Time;
use legenda_core::transcript;
use legenda_core::{ass, convert, Error, Format, PlainCue, Subtitle};

#[derive(Debug, Parser)]
#[command(name = "legenda", version, about = "Fast subtitle parsing, writing and conversion")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Convert a subtitle file to another format.
    Convert {
        /// Input subtitle file.
        input: PathBuf,
        /// Output subtitle file.
        output: PathBuf,
        /// Override the input format (inferred from the extension by default).
        #[arg(short = 'f', long)]
        from: Option<String>,
        /// Override the output format (inferred from the extension by default).
        #[arg(short = 't', long)]
        to: Option<String>,
    },
    /// Show format, encoding, cue count and timing range for a subtitle file.
    Info {
        /// Input subtitle file.
        input: PathBuf,
        /// Override the input format.
        #[arg(short = 'f', long)]
        format: Option<String>,
    },
    /// Shift every cue by a signed amount (e.g. `1.5s`, `-250ms`).
    Shift {
        input: PathBuf,
        output: PathBuf,
        /// Signed time offset.
        #[arg(short = 'b', long)]
        by: String,
    },
    /// Convert timings exactly between frame rates (e.g. 23.976 -> 25).
    Scale {
        input: PathBuf,
        output: PathBuf,
        /// Source frame rate.
        #[arg(short = 'f', long)]
        from: String,
        /// Target frame rate.
        #[arg(short = 't', long)]
        to: String,
    },
    /// Apply an affine two-point synchronisation to all cues.
    Sync {
        input: PathBuf,
        output: PathBuf,
        /// Source timestamp of the first reference point.
        #[arg(long)]
        from_a: String,
        /// Target timestamp of the first reference point.
        #[arg(long)]
        to_a: String,
        /// Source timestamp of the second reference point.
        #[arg(long)]
        from_b: String,
        /// Target timestamp of the second reference point.
        #[arg(long)]
        to_b: String,
    },
    /// Keep only cues within a time range.
    Clamp {
        input: PathBuf,
        output: PathBuf,
        /// Lower bound (inclusive).
        #[arg(long)]
        start: String,
        /// Upper bound (inclusive).
        #[arg(long)]
        end: String,
    },
    /// Merge several subtitle files chronologically into one.
    Merge {
        /// Input files, in any order. With no `--output`, the last path is
        /// treated as the output file.
        #[arg(required = true, num_args = 1..)]
        inputs: Vec<PathBuf>,
        /// Output file.
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
    /// Split one file into two at a timestamp.
    Split {
        input: PathBuf,
        /// Output file for cues starting before the split point.
        first: PathBuf,
        /// Output file for the remaining cues.
        second: PathBuf,
        /// Split timestamp.
        #[arg(long)]
        at: String,
    },
    /// Remove hearing-impaired annotations and/or speaker labels.
    Clean {
        input: PathBuf,
        output: PathBuf,
        /// Remove hearing-impaired annotations only.
        #[arg(long)]
        hi: bool,
        /// Remove speaker labels only.
        #[arg(long)]
        speakers: bool,
    },
    /// Remove hearing-impaired annotations such as `[MÚSICA]` or `(risos)`.
    #[command(alias = "remove-hi", alias = "strip_hi")]
    StripHi { input: PathBuf, output: PathBuf },
    /// Remove leading speaker labels such as `JOÃO:`.
    #[command(alias = "remove-speakers", alias = "strip_speakers")]
    StripSpeakers { input: PathBuf, output: PathBuf },
    /// Report guideline violations. Exit 0 when clean, 1 when violations exist.
    Lint {
        input: PathBuf,
        /// Emit a machine-readable JSON report.
        #[arg(long)]
        json: bool,
        /// Guideline preset used for the thresholds.
        #[arg(long, default_value = "netflix-ptbr")]
        preset: String,
        /// Frame rate used for the two-frame gap check.
        #[arg(long)]
        fps: Option<String>,
        /// Use children's reading-speed limits.
        #[arg(long)]
        children: bool,
    },
    /// Reflow lines and timings to a guideline preset.
    Reflow {
        input: PathBuf,
        output: PathBuf,
        /// Guideline preset: `netflix-ptbr`, `netflix-ptbr-children` or `shorts`.
        #[arg(long, default_value = "netflix-ptbr")]
        preset: String,
        /// Frame rate used for the minimum gap.
        #[arg(long)]
        fps: Option<String>,
        /// Use children's reading-speed limits.
        #[arg(long)]
        children: bool,
    },
    /// Turn a word-level ASR transcript into animated karaoke ASS captions.
    Karaoke {
        /// WhisperX, whisper.cpp `-ojf` or OpenAI `verbose_json` transcript.
        input: PathBuf,
        /// Output ASS file.
        #[arg(short = 'o', long)]
        output: PathBuf,
        /// Visual preset: `hormozi`, `clean`, `karaoke` or `bounce`.
        #[arg(long, default_value = "hormozi")]
        preset: String,
        /// Font family written to the ASS style.
        #[arg(long)]
        font: Option<String>,
        /// Font size in script pixels.
        #[arg(long)]
        font_size: Option<u32>,
        /// Outline width in script pixels.
        #[arg(long)]
        outline: Option<u32>,
        /// Shadow distance in script pixels.
        #[arg(long)]
        shadow: Option<u32>,
        /// Left safe-area margin in pixels.
        #[arg(long)]
        safe_left: Option<u32>,
        /// Right safe-area margin in pixels.
        #[arg(long)]
        safe_right: Option<u32>,
        /// Top safe-area margin in pixels.
        #[arg(long)]
        safe_top: Option<u32>,
        /// Bottom safe-area margin in pixels.
        #[arg(long)]
        safe_bottom: Option<u32>,
        /// Canvas width in pixels (default 1080).
        #[arg(long)]
        width: Option<u32>,
        /// Canvas height in pixels (default 1920).
        #[arg(long)]
        height: Option<u32>,
        /// Words rendered in the emphasis colour (repeatable).
        #[arg(long)]
        emphasis: Vec<String>,
        /// Maximum words per caption page.
        #[arg(long)]
        words_per_page: Option<usize>,
        /// Maximum characters per caption page.
        #[arg(long)]
        max_chars: Option<usize>,
        /// Pause in milliseconds that starts a new page (default 300).
        #[arg(long)]
        pause_ms: Option<u64>,
    },
    /// Burn ASS captions into a video with ffmpeg.
    Burn {
        /// Input video file.
        video: PathBuf,
        /// ASS caption file.
        captions: PathBuf,
        /// Output video file.
        #[arg(short = 'o', long)]
        output: PathBuf,
        /// Video encoder, e.g. `libx264` or `h264_nvenc`.
        #[arg(long, default_value = "libx264")]
        encoder: String,
        /// Encoder option in `key=value` form (repeatable).
        #[arg(long)]
        encoder_option: Vec<String>,
        /// Path to the ffmpeg executable (default `ffmpeg` on PATH).
        #[arg(long)]
        ffmpeg: Option<PathBuf>,
    },
}

fn main() {
    let code = run();
    if code != 0 {
        std::process::exit(code);
    }
}

/// Exit codes: 0 success/clean, 1 lint violations, 2 operational error.
fn run() -> i32 {
    match execute() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            2
        }
    }
}

fn execute() -> Result<i32> {
    let cli = Cli::parse();
    match cli.command {
        Command::Convert { input, output, from, to } => {
            let bytes = std::fs::read(&input)?;
            let source_format = match from {
                Some(name) => parse_format_name(&name)?,
                None => Format::detect(&bytes, &input)?,
            };
            let target_format = match to {
                Some(name) => parse_format_name(&name)?,
                None => ops::output_format_from_path(&source_format, &output),
            };
            let subtitle = Subtitle::decode(&bytes, source_format)?;
            let converted = convert::convert(&subtitle, target_format)?;
            std::fs::write(&output, converted.encode()?)?;
            println!(
                "converted {} -> {} ({} cues)",
                source_format.name(),
                target_format.name(),
                subtitle.plain_cues()?.len()
            );
        }
        Command::Info { input, format } => {
            let bytes = std::fs::read(&input)?;
            let detected = match format {
                Some(name) => parse_format_name(&name)?,
                None => Format::detect(&bytes, &input)?,
            };
            let (_, encoding_name) = encoding::decode(&bytes);
            let subtitle = Subtitle::decode(&bytes, detected)?;
            let cues = subtitle.plain_cues()?;
            let first = cues.first().map(|cue| cue.start.to_string()).unwrap_or_default();
            let last = cues.last().map(|cue| cue.end.to_string()).unwrap_or_default();
            println!("format: {}", detected.name());
            println!("encoding: {encoding_name}");
            println!("cues: {}", cues.len());
            println!("first start: {first}");
            println!("last end: {last}");
        }
        Command::Shift { input, output, by } => {
            let delta = ops::parse_time(&by)?;
            edit(&input, &output, |cues| ops::shift(cues, delta))?;
            println!("shifted cues by {by}");
        }
        Command::Scale { input, output, from, to } => {
            let source = FrameRate::parse(&from)?;
            let target = FrameRate::parse(&to)?;
            edit(&input, &output, |cues| ops::scale(cues, source, target))?;
            println!("scaled cues from {source} fps to {target} fps");
        }
        Command::Sync { input, output, from_a, to_a, from_b, to_b } => {
            let (source_a, target_a, source_b, target_b) = (
                ops::parse_time(&from_a)?,
                ops::parse_time(&to_a)?,
                ops::parse_time(&from_b)?,
                ops::parse_time(&to_b)?,
            );
            edit(&input, &output, |cues| ops::sync(cues, source_a, target_a, source_b, target_b))?;
            println!("synchronised cues using two reference points");
        }
        Command::Clamp { input, output, start, end } => {
            let lower = ops::parse_time(&start)?;
            let upper = ops::parse_time(&end)?;
            edit(&input, &output, |cues| ops::clamp(cues, lower, upper))?;
            println!("clamped cues to [{start}, {end}]");
        }
        Command::Merge { inputs, output } => {
            let (inputs, output) = match output {
                Some(path) => (inputs, path),
                None if inputs.len() > 1 => {
                    let output = inputs.last().expect("checked above").clone();
                    (inputs[..inputs.len() - 1].to_vec(), output)
                }
                None => return Err(Error::EmptyMerge),
            };
            let mut documents = Vec::with_capacity(inputs.len());
            for path in &inputs {
                documents.push(read_subtitle(path, None)?.0);
            }
            let merged = ops::merge(&documents)?;
            let count = merged.plain_cues()?.len();
            write_subtitle(&output, &merged)?;
            println!("merged {} files into {count} cues", inputs.len());
        }
        Command::Split { input, first, second, at } => {
            let boundary = ops::parse_time(&at)?;
            let subtitle = read_subtitle(&input, None)?.0;
            let (head, tail) = ops::split_at_time(&subtitle, boundary)?;
            write_subtitle(&first, &head)?;
            write_subtitle(&second, &tail)?;
            println!(
                "split at {at}: {} / {} cues",
                head.plain_cues()?.len(),
                tail.plain_cues()?.len()
            );
        }
        Command::Clean { input, output, hi, speakers } => {
            let remove_hi = hi || !speakers;
            let remove_speakers = speakers || !hi;
            edit(&input, &output, |cues| {
                if remove_hi {
                    ops::strip_hearing_impaired(cues);
                }
                if remove_speakers {
                    ops::strip_speaker_labels(cues);
                }
                Ok(())
            })?;
            println!("cleaned cues");
        }
        Command::StripHi { input, output } => {
            edit(&input, &output, |cues| {
                ops::strip_hearing_impaired(cues);
                Ok(())
            })?;
            println!("removed hearing-impaired annotations");
        }
        Command::StripSpeakers { input, output } => {
            edit(&input, &output, |cues| {
                ops::strip_speaker_labels(cues);
                Ok(())
            })?;
            println!("removed speaker labels");
        }
        Command::Lint { input, json, preset, fps, children } => {
            let (subtitle, _) = read_subtitle(&input, None)?;
            let frame_rate = match fps {
                Some(value) => FrameRate::parse(&value)?,
                None => FrameRate::FPS_24,
            };
            let selected = Preset::parse(&preset)?;
            let children = children || selected == Preset::NetflixPtBrChildren;
            let config = if selected == Preset::Shorts {
                LintConfig::shorts(frame_rate)?
            } else {
                LintConfig::netflix_ptbr(frame_rate, children)?
            };
            let report = LintReport::new(&subtitle.plain_cues()?, &config);
            if json {
                println!("{}", report.to_json()?);
            } else {
                print!("{}", report.to_text());
            }
            return Ok(i32::from(report.has_violations()));
        }
        Command::Reflow { input, output, preset, fps, children } => {
            let (subtitle, _) = read_subtitle(&input, None)?;
            let selected =
                if children { Preset::NetflixPtBrChildren } else { Preset::parse(&preset)? };
            let frame_rate = match fps {
                Some(value) => FrameRate::parse(&value)?,
                None => FrameRate::FPS_24,
            };
            let options = reflow::ReflowOptions::new(selected, frame_rate)?;
            let mut cues = subtitle.plain_cues()?;
            reflow::reflow(&mut cues, &options)?;
            let result = ops::with_cues(&subtitle, &cues)?;
            write_subtitle(&output, &result)?;
            println!("reflowed {} cues with preset {}", cues.len(), selected.name());
        }
        Command::Karaoke {
            input,
            output,
            preset,
            font,
            font_size,
            outline,
            shadow,
            safe_left,
            safe_right,
            safe_top,
            safe_bottom,
            width,
            height,
            emphasis,
            words_per_page,
            max_chars,
            pause_ms,
        } => {
            let selected = KaraokePreset::parse(&preset)?;
            let mut options = KaraokeOptions::new(selected);
            if let Some(font) = font {
                options.font = font;
            }
            if let Some(size) = font_size {
                if size == 0 {
                    return Err(Error::Parse {
                        format: "karaoke",
                        message: "font size must be greater than zero".to_owned(),
                    });
                }
                options.font_size = size;
            }
            if let Some(outline) = outline {
                options.outline = outline;
            }
            if let Some(shadow) = shadow {
                options.shadow = shadow;
            }
            if let Some(margin) = safe_left {
                options.safe_area.left = margin;
            }
            if let Some(margin) = safe_right {
                options.safe_area.right = margin;
            }
            if let Some(margin) = safe_top {
                options.safe_area.top = margin;
            }
            if let Some(margin) = safe_bottom {
                options.safe_area.bottom = margin;
            }
            if let Some(width) = width {
                options.width = width;
            }
            if let Some(height) = height {
                options.height = height;
            }
            if let Some(words) = words_per_page {
                options.page.max_words = words;
            }
            if let Some(chars) = max_chars {
                options.page.max_chars = chars;
            }
            if let Some(millis) = pause_ms {
                options.page.pause_threshold = Time::from_millis(millis as i64);
            }
            options.emphasis = emphasis;
            let json = std::fs::read_to_string(&input)?;
            let transcript = transcript::parse_transcript(&json)?;
            let document = karaoke::render(&transcript.words, &options)?;
            let pages = document.dialogues().len();
            std::fs::write(&output, ass::write(&document)?)?;
            println!(
                "wrote {pages} karaoke pages ({}) from {} words to {}",
                selected.name(),
                transcript.words.len(),
                output.display()
            );
        }
        Command::Burn { video, captions, output, encoder, encoder_option, ffmpeg } => {
            let mut options = BurnOptions { encoder, ..BurnOptions::default() };
            for option in &encoder_option {
                options.encoder_options.push(BurnOptions::parse_encoder_option(option)?);
            }
            if let Some(ffmpeg) = ffmpeg {
                options.ffmpeg = ffmpeg;
            }
            let arguments = burn::build_args(&video, &captions, &output, &options)?;
            let status = StdCommand::new(&options.ffmpeg).args(&arguments).status()?;
            if !status.success() {
                return Err(Error::FfmpegFailed(status.to_string()));
            }
            println!("burned captions into {}", output.display());
        }
    }
    Ok(0)
}

fn read_subtitle(path: &Path, format: Option<&str>) -> Result<(Subtitle, Format)> {
    let bytes = std::fs::read(path)?;
    let detected = match format {
        Some(name) => parse_format_name(name)?,
        None => Format::detect(&bytes, path)?,
    };
    Ok((Subtitle::decode(&bytes, detected)?, detected))
}

fn write_subtitle(path: &Path, subtitle: &Subtitle) -> Result<()> {
    let target = ops::output_format(subtitle, path);
    let converted = convert::convert(subtitle, target)?;
    std::fs::write(path, converted.encode()?)?;
    Ok(())
}

fn edit<F>(input: &Path, output: &Path, change: F) -> Result<()>
where
    F: FnOnce(&mut Vec<PlainCue>) -> Result<()>,
{
    let (subtitle, _) = read_subtitle(input, None)?;
    let mut cues = subtitle.plain_cues()?;
    change(&mut cues)?;
    let result = ops::with_cues(&subtitle, &cues)?;
    write_subtitle(output, &result)
}

fn parse_format_name(name: &str) -> Result<Format> {
    Format::from_name(name).ok_or_else(|| Error::UnsupportedFormat(name.to_owned()))
}
