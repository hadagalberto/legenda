//! Tolerant SubRip parser and strict writer.

use crate::error::{parse_error, Result};
use crate::text_util::{clean_lines, normalize_newlines, split_blocks};
use crate::time::Time;

/// A single SRT cue. The index is always present: the tolerant parser assigns
/// sequential numbers to cues whose index was omitted in the source file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SrtCue {
    pub index: u32,
    pub start: Time,
    pub end: Time,
    /// Text including any inline markup such as `<i>`, `<b>` or `<font>`.
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SrtDocument {
    pub cues: Vec<SrtCue>,
}

/// Parses SRT text.
///
/// Tolerated quirks: UTF-8 BOM, LF/CRLF/CR line endings, missing indices,
/// `,` or `.` millisecond separators, extra blank lines, and coordinates
/// after the end timestamp. Inline markup is preserved verbatim.
pub fn parse(input: &str) -> Result<SrtDocument> {
    let normalised = normalize_newlines(input);
    let mut cues = Vec::new();

    for (block_number, block) in split_blocks(&normalised).iter().enumerate() {
        let lines = clean_lines(block);
        let Some(timestamp_index) = lines.iter().position(|line| line.contains("-->")) else {
            return Err(parse_error(
                "SRT",
                format!("block {} has no '-->' timestamp line", block_number + 1),
            ));
        };

        let head = &lines[..timestamp_index];
        let (index, leading_text) = if head.len() == 1
            && !head[0].is_empty()
            && head[0].bytes().all(|byte| byte.is_ascii_digit())
        {
            (head[0].parse::<u32>().ok(), Vec::new())
        } else {
            (None, head.to_vec())
        };

        let (start, end) = parse_timestamp_line(&lines[timestamp_index])?;

        let mut text_lines = leading_text;
        text_lines.extend_from_slice(&lines[timestamp_index + 1..]);
        while text_lines.first().is_some_and(|line| line.trim().is_empty()) {
            text_lines.remove(0);
        }
        while text_lines.last().is_some_and(|line| line.trim().is_empty()) {
            text_lines.pop();
        }

        cues.push(SrtCue {
            index: index.unwrap_or(cues.len() as u32 + 1),
            start,
            end,
            text: text_lines.join("\n"),
        });
    }

    Ok(SrtDocument { cues })
}

fn parse_timestamp_line(line: &str) -> Result<(Time, Time)> {
    let (start_text, rest) =
        line.split_once("-->").ok_or_else(|| parse_error("SRT", "timestamp line has no '-->'"))?;
    // Anything after the end timestamp (SRT coordinates) is ignored.
    let end_text = rest.split_whitespace().next().unwrap_or("");
    Ok((Time::parse_srt(start_text)?, Time::parse_srt(end_text)?))
}

/// Serialises an SRT document using LF line endings and canonical timestamps.
pub fn write(document: &SrtDocument) -> String {
    let mut output = String::new();
    for (position, cue) in document.cues.iter().enumerate() {
        if position > 0 {
            output.push('\n');
        }
        output.push_str(&cue.index.to_string());
        output.push('\n');
        output.push_str(&cue.start.format_srt());
        output.push_str(" --> ");
        output.push_str(&cue.end.format_srt());
        output.push('\n');
        output.push_str(&cue.text);
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_srt() {
        let document = parse("1\n00:00:01,000 --> 00:00:02,500\nOlá, mundo\n").unwrap();
        assert_eq!(document.cues.len(), 1);
        assert_eq!(document.cues[0].index, 1);
        assert_eq!(document.cues[0].start, Time::from_millis(1_000));
        assert_eq!(document.cues[0].text, "Olá, mundo");
    }

    #[test]
    fn tolerates_missing_indices_dot_separator_and_crlf() {
        let input = "00:00:01.000 --> 00:00:02.000\r\n<i>Primeira</i>\r\n\r\n\
                     00:00:03,000 --> 00:00:04,000\r\nSegunda linha 1\r\nSegunda linha 2\r\n";
        let document = parse(input).unwrap();
        assert_eq!(document.cues.len(), 2);
        assert_eq!(document.cues[0].index, 1);
        assert_eq!(document.cues[0].text, "<i>Primeira</i>");
        assert_eq!(document.cues[1].index, 2);
        assert_eq!(document.cues[1].text, "Segunda linha 1\nSegunda linha 2");
    }

    #[test]
    fn tolerates_extra_blank_lines_and_coordinates() {
        let input = "\n\n7\n00:00:01,000 --> 00:00:02,000  X1:10 X2:20\nTexto\n\n\n\n";
        let document = parse(input).unwrap();
        assert_eq!(document.cues.len(), 1);
        assert_eq!(document.cues[0].index, 7);
        assert_eq!(document.cues[0].text, "Texto");
    }

    #[test]
    fn rejects_block_without_timestamp() {
        let error = parse("1\nthis is not a timestamp\n").unwrap_err();
        assert!(error.to_string().contains("no '-->' timestamp line"));
    }

    #[test]
    fn round_trip_is_lossless() {
        let input = "1\n00:00:00,000 --> 00:00:01,200\n<b>Negrito</b> e <font color=\"#fff\">cor</font>\n\n\
                     4\n00:01:00,000 --> 00:01:01,000\nLinha 1\nLinha 2\n";
        let document = parse(input).unwrap();
        let written = write(&document);
        assert_eq!(parse(&written).unwrap(), document);
    }
}
