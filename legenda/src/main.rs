//! The `legenda` command line interface.

#![deny(warnings)]

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use legenda_core::encoding;
use legenda_core::error::Result;
use legenda_core::{Format, Subtitle};

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
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
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
                None => Format::detect(&[], &output)?,
            };
            let subtitle = Subtitle::decode(&bytes, source_format)?;
            let converted = legenda_core::convert::convert(&subtitle, target_format)?;
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
            let (_, detected_encoding) = encoding::decode(&bytes);
            let subtitle = Subtitle::decode(&bytes, detected)?;
            let cues = subtitle.plain_cues()?;
            let first = cues.first().map(|cue| cue.start.to_string()).unwrap_or_default();
            let last = cues.last().map(|cue| cue.end.to_string()).unwrap_or_default();
            println!("format: {}", detected.name());
            println!("encoding: {detected_encoding}");
            println!("cues: {}", cues.len());
            println!("first start: {first}");
            println!("last end: {last}");
        }
    }
    Ok(())
}

fn parse_format_name(name: &str) -> Result<Format> {
    Format::from_name(name).ok_or_else(|| legenda_core::Error::UnsupportedFormat(name.to_owned()))
}
