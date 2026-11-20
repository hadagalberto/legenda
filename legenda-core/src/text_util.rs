//! Shared helpers for the tolerant text parsers.

/// Normalises CRLF and CR line endings to LF and removes a leading BOM.
pub(crate) fn normalize_newlines(input: &str) -> String {
    let normalised = input.replace("\r\n", "\n").replace('\r', "\n");
    normalised.strip_prefix('\u{feff}').unwrap_or(&normalised).to_owned()
}

/// Splits a document into cue blocks, treating blank (or whitespace-only)
/// lines as separators and collapsing runs of them into one.
pub(crate) fn split_blocks(input: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = String::new();

    for line in input.lines() {
        if line.trim().is_empty() {
            if !current.trim().is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
        } else {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }
    if !current.trim().is_empty() {
        blocks.push(current);
    }
    blocks
}

/// Removes leading and trailing blank lines from a block and trims trailing
/// whitespace from every remaining line.
pub(crate) fn clean_lines(block: &str) -> Vec<String> {
    let mut lines: Vec<String> = block.lines().map(|line| line.trim_end().to_owned()).collect();
    while lines.first().is_some_and(|line| line.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    lines
}
