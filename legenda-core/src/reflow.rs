//! Guideline-driven reflow: linguistic line breaks, presets and timing fixes.
//!
//! Preset thresholds are the exact published Netflix values documented in
//! `docs/netflix-ptbr-style-guide.md`:
//!
//! * Portuguese (Brazil) guide I.2: 42 characters per line.
//! * I.11: two lines maximum, prefer one line, bottom-heavy balance, break
//!   after punctuation and before conjunctions/prepositions, never split an
//!   article from its noun, a verb from its subject pronoun, etc.
//! * I.14: 17 CPS adult / 13 CPS children.
//! * General Requirements §1: 5/6 s minimum and 7 s maximum duration.
//!
//! Sources:
//! <https://partnerhelp.netflixstudios.com/hc/en-us/articles/215600497-Portuguese-Brazil-Timed-Text-Style-Guide>
//! <https://partnerhelp.netflixstudios.com/hc/en-us/articles/215758617-Timed-Text-Style-Guide-General-Requirements>
//!
//! The two-frame minimum gap is a `SPEC.md` requirement, not a Netflix value.

use crate::error::Result;
use crate::lint::LintConfig;
use crate::model::PlainCue;
use crate::ops::{sort_and_fix_gaps, visible_char_count, FrameRate};
use crate::time::{Rational, Time};

/// A reflow preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// Netflix Brazilian Portuguese, adult reading speed (17 CPS).
    NetflixPtBr,
    /// Netflix Brazilian Portuguese, children's reading speed (13 CPS).
    NetflixPtBrChildren,
    /// Vertical shorts: one short line per page, about four words.
    Shorts,
}

impl Preset {
    pub fn parse(input: &str) -> Result<Self> {
        match input.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "netflix-ptbr" | "netflix" | "netflix-pt-br" => Ok(Self::NetflixPtBr),
            "netflix-ptbr-children" | "netflix-children" | "children" => {
                Ok(Self::NetflixPtBrChildren)
            }
            "shorts" => Ok(Self::Shorts),
            _ => Err(crate::error::Error::UnknownPreset(input.to_owned())),
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::NetflixPtBr => "netflix-ptbr",
            Self::NetflixPtBrChildren => "netflix-ptbr-children",
            Self::Shorts => "shorts",
        }
    }
}

/// Fully resolved thresholds for a reflow run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReflowOptions {
    pub preset: Preset,
    pub fps: FrameRate,
    pub max_chars_per_line: usize,
    pub max_lines: usize,
    pub max_cps: Rational,
    pub min_duration: Time,
    pub max_duration: Time,
    pub min_gap: Time,
    /// Soft page size for the `shorts` preset; zero disables page splitting.
    pub words_per_page: usize,
}

impl ReflowOptions {
    pub fn new(preset: Preset, fps: FrameRate) -> Result<Self> {
        let children = preset == Preset::NetflixPtBrChildren;
        let lint = if preset == Preset::Shorts {
            LintConfig::shorts(fps)?
        } else {
            LintConfig::netflix_ptbr(fps, children)?
        };
        let (max_lines, max_cps, words_per_page) = match preset {
            Preset::NetflixPtBr | Preset::NetflixPtBrChildren => (2, lint.max_cps, 0),
            Preset::Shorts => (1, Rational::new(20, 1)?, 4),
        };
        Ok(Self {
            preset,
            fps,
            max_chars_per_line: lint.max_chars_per_line,
            max_lines,
            max_cps,
            min_duration: lint.min_duration,
            max_duration: lint.max_duration,
            // SRT/WebVTT writers emit milliseconds: round the two-frame gap
            // up so the serialized file still satisfies the requirement.
            min_gap: ceil_millis(lint.min_gap)?,
            words_per_page,
        })
    }
}

/// Rounds a time up to the next whole millisecond, so writing to SRT/WebVTT
/// never shortens a duration or a gap by sub-millisecond rounding.
pub fn ceil_millis(time: Time) -> Result<Time> {
    let millis = time.as_millis();
    let denominator = millis.den() as i64;
    let ceiled = if millis.num() >= 0 {
        (millis.num() + denominator - 1) / denominator
    } else {
        millis.num() / denominator
    };
    Ok(Time::from_millis(ceiled))
}

/// A word with the inline markup that belongs immediately before and after it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct WordPiece {
    before: String,
    word: String,
    after: String,
}

fn flush_word(pieces: &mut Vec<WordPiece>, pending: &mut String, current: &mut String) -> bool {
    if current.is_empty() {
        false
    } else {
        pieces.push(WordPiece {
            before: std::mem::take(pending),
            word: std::mem::take(current),
            after: String::new(),
        });
        true
    }
}

fn tokenize(text: &str) -> Vec<WordPiece> {
    let mut pieces: Vec<WordPiece> = Vec::new();
    let mut pending = String::new();
    let mut after_word = false;
    let mut current = String::new();

    let characters: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < characters.len() {
        let character = characters[index];
        let tag_end = match character {
            '<' => Some('>'),
            '{' => Some('}'),
            _ => None,
        };
        if let Some(closing) = tag_end {
            if flush_word(&mut pieces, &mut pending, &mut current) {
                after_word = true;
            }
            if let Some(end) =
                (index + 1..characters.len()).find(|&position| characters[position] == closing)
            {
                let tag: String = characters[index..=end].iter().collect();
                if after_word && pending.is_empty() {
                    if let Some(last) = pieces.last_mut() {
                        last.after.push_str(&tag);
                    }
                    after_word = false;
                } else {
                    pending.push_str(&tag);
                }
                index = end + 1;
                continue;
            }
        }
        if character.is_whitespace() {
            if flush_word(&mut pieces, &mut pending, &mut current) {
                after_word = true;
            }
        } else {
            current.push(character);
        }
        index += 1;
    }
    flush_word(&mut pieces, &mut pending, &mut current);
    if !pending.is_empty() {
        if let Some(last) = pieces.last_mut() {
            last.after.push_str(&pending);
        }
    }
    pieces
}

fn rebuild(pieces: &[WordPiece]) -> String {
    let mut output = String::new();
    for (position, piece) in pieces.iter().enumerate() {
        if position > 0 {
            output.push(' ');
        }
        output.push_str(&piece.before);
        output.push_str(&piece.word);
        output.push_str(&piece.after);
    }
    output
}

fn span_length(pieces: &[WordPiece]) -> usize {
    pieces.iter().map(|piece| visible_char_count(&piece.word)).sum::<usize>()
        + pieces.len().saturating_sub(1)
}

fn bare(word: &str) -> String {
    word.trim_matches(|character: char| !character.is_alphanumeric()).to_lowercase()
}

fn in_list(word: &str, list: &[&str]) -> bool {
    list.contains(&word)
}

const PREPOSITIONS: &[&str] = &[
    "de", "em", "por", "para", "com", "sem", "sobre", "sob", "até", "desde", "como", "ante",
    "após", "contra", "dentre", "entre", "perante", "segundo", "via", "ao", "aos", "à", "às", "do",
    "dos", "da", "das", "no", "nos", "na", "nas", "num", "numa", "nuns", "numas",
];
const CONJUNCTIONS: &[&str] = &[
    "e", "ou", "mas", "nem", "que", "porque", "pois", "porém", "contudo", "todavia", "então", "se",
    "quando", "enquanto", "embora", "logo", "assim", "pois",
];
const ARTICLES: &[&str] = &[
    "o", "a", "os", "as", "um", "uma", "uns", "umas", "este", "esta", "estes", "estas", "esse",
    "essa", "esses", "essas", "aquele", "aquela", "aqueles", "aquelas",
];
const AUXILIARIES: &[&str] = &[
    "não", "se", "si", "já", "ainda", "também", "mais", "muito", "ser", "estar", "ter", "haver",
    "poder", "dever", "querer", "ir", "vou", "vai", "vamos", "era", "foi", "foram", "são",
];

fn break_score(previous: &str, next: &str, left: usize, right: usize, words_on_top: usize) -> i32 {
    let previous_bare = bare(previous);
    let next_bare = bare(next);
    let mut score = 0;

    if previous
        .trim_end()
        .ends_with(['.', ',', ';', ':', '!', '?', '…', '"', '\u{201d}', '\u{2019}', '\u{2026}'])
    {
        score += 6;
    }
    if in_list(&next_bare, PREPOSITIONS) || in_list(&next_bare, CONJUNCTIONS) {
        score += 4;
    }
    // Never split a noun from its article/adjective, a verb from its subject
    // pronoun, or a verb from an auxiliary/reflexive pronoun/negation.
    if in_list(&previous_bare, ARTICLES)
        || in_list(&previous_bare, PREPOSITIONS)
        || in_list(&previous_bare, AUXILIARIES)
    {
        score -= 12;
    }
    // Prefer bottom-heavy (pyramid) subtitles and balanced line lengths.
    if left <= right {
        score += 2;
    } else {
        score -= 2;
    }
    score -= i32::try_from(left.abs_diff(right) / 4).unwrap_or(i32::MAX);
    if words_on_top <= 2 {
        score -= 3;
    }
    score
}

fn best_break(pieces: &[WordPiece], start: usize, max_chars: usize) -> Option<usize> {
    let mut best: Option<(usize, i32)> = None;
    for boundary in start + 1..pieces.len() {
        let left = span_length(&pieces[start..boundary]);
        let right = span_length(&pieces[boundary..]);
        if left > max_chars || right > max_chars {
            continue;
        }
        let score = break_score(
            &pieces[boundary - 1].word,
            &pieces[boundary].word,
            left,
            right,
            boundary - start,
        );
        if best.is_none_or(|(_, previous)| score > previous) {
            best = Some((boundary, score));
        }
    }
    best.map(|(boundary, _)| boundary)
}

fn fallback_break(pieces: &[WordPiece], start: usize) -> usize {
    let total = span_length(&pieces[start..]);
    let mut best = start + 1;
    let mut best_difference = usize::MAX;
    for boundary in start + 1..pieces.len() {
        let left = span_length(&pieces[start..boundary]);
        let right = total.saturating_sub(left + 1);
        let difference = left.abs_diff(right);
        if difference < best_difference {
            best_difference = difference;
            best = boundary;
        }
    }
    best
}

/// Wraps word pieces into at most `max_lines` lines.
fn wrap(pieces: &[WordPiece], max_chars: usize, max_lines: usize) -> Vec<String> {
    if pieces.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut start = 0;
    while start < pieces.len() && lines.len() < max_lines {
        if lines.len() + 1 == max_lines || span_length(&pieces[start..]) <= max_chars {
            lines.push(rebuild(&pieces[start..]));
            start = pieces.len();
            break;
        }
        let boundary =
            best_break(pieces, start, max_chars).unwrap_or_else(|| fallback_break(pieces, start));
        lines.push(rebuild(&pieces[start..boundary]));
        start = boundary;
    }
    if start < pieces.len() {
        if let Some(last) = lines.last_mut() {
            last.push(' ');
            last.push_str(&rebuild(&pieces[start..]));
        } else {
            lines.push(rebuild(&pieces[start..]));
        }
    }
    lines
}

fn time_add(time: Time, delta: Rational) -> Result<Time> {
    Ok(Time::from_rational_seconds(time.as_rational().checked_add(delta)?))
}

/// Splits a cue's word pieces into `groups`, distributing the cue's duration
/// proportionally to the visible character count of each group.
fn regroup(cue: &PlainCue, groups: Vec<Vec<WordPiece>>) -> Result<Vec<PlainCue>> {
    if groups.is_empty() {
        return Ok(Vec::new());
    }
    let lengths: Vec<usize> = groups.iter().map(|group| span_length(group)).collect();
    let total: usize = lengths.iter().sum();
    let group_count = lengths.len();
    let duration = cue.end.as_rational().checked_sub(cue.start.as_rational())?;
    let mut cursor = cue.start;
    let mut result = Vec::with_capacity(group_count);
    for (position, (group, length)) in groups.into_iter().zip(lengths).enumerate() {
        let end = if position + 1 == group_count {
            cue.end
        } else if total == 0 {
            let share = duration.checked_div(Rational::from_int(group_count as i64))?;
            time_add(cursor, share)?
        } else {
            let fraction =
                Rational::from_int(length as i64).checked_div(Rational::from_int(total as i64))?;
            time_add(cursor, duration.checked_mul(fraction)?)?
        };
        result.push(PlainCue { start: cursor, end, text: rebuild(&group) });
        cursor = end;
    }
    Ok(result)
}

fn chunk_pieces(pieces: Vec<WordPiece>, group_count: usize) -> Vec<Vec<WordPiece>> {
    let count = group_count.min(pieces.len()).max(1);
    let base = pieces.len() / count;
    let remainder = pieces.len() % count;
    let mut groups = Vec::with_capacity(count);
    let mut iterator = pieces.into_iter();
    for index in 0..count {
        let size = base + usize::from(index < remainder);
        groups.push(iterator.by_ref().take(size).collect());
    }
    groups
}

fn page_pieces(pieces: Vec<WordPiece>, words_per_page: usize) -> Vec<Vec<WordPiece>> {
    if words_per_page == 0 || pieces.len() <= words_per_page {
        return vec![pieces];
    }
    pieces.chunks(words_per_page).map(|chunk| chunk.to_vec()).collect()
}

/// Reflows cues in place: pages, wraps lines by the preset's linguistic rules,
/// splits over-long events, and extends timings until reading speed and
/// minimum duration are within limits. Later cues are shifted forward by the
/// same amount whenever an event is extended, preserving gaps and ordering.
pub fn reflow(cues: &mut Vec<PlainCue>, options: &ReflowOptions) -> Result<()> {
    sort_and_fix_gaps(cues, options.min_gap)?;

    // Shorts: break long events into pages of roughly four words.
    let mut paged = Vec::new();
    for cue in cues.iter() {
        let pieces = tokenize(&cue.text);
        paged.extend(regroup(cue, page_pieces(pieces, options.words_per_page))?);
    }
    *cues = paged;

    // Netflix: an event longer than the maximum duration is split at word
    // boundaries into the smallest number of events that fit.
    let mut duration_split = Vec::new();
    for cue in cues.iter() {
        let duration = cue.end.as_rational().checked_sub(cue.start.as_rational())?;
        let limit = options.max_duration.as_rational();
        if duration <= limit || limit <= Rational::ZERO {
            duration_split.push(cue.clone());
            continue;
        }
        let ratio = duration.checked_div(limit)?;
        let groups_needed = (ratio.num() + ratio.den() as i64 - 1) / ratio.den() as i64;
        let groups = chunk_pieces(tokenize(&cue.text), groups_needed as usize);
        duration_split.extend(regroup(cue, groups)?);
    }
    *cues = duration_split;
    sort_and_fix_gaps(cues, options.min_gap)?;

    // Serialisation uses millisecond precision. Quantise every timestamp up
    // to a whole millisecond, then restore the minimum gap on that exact
    // grid so the written file keeps the invariants verified here.
    for cue in cues.iter_mut() {
        cue.start = ceil_millis(cue.start)?;
        cue.end = ceil_millis(cue.end)?;
        if cue.end <= cue.start {
            cue.end = Time::from_millis(cue.start.as_millis().num() + 1);
        }
    }
    sort_and_fix_gaps(cues, options.min_gap)?;

    // Wrap text according to the preset's line-break rules.
    for cue in cues.iter_mut() {
        let pieces = tokenize(&cue.text);
        let lines = wrap(&pieces, options.max_chars_per_line, options.max_lines);
        cue.text = lines.join("\n");
    }

    // CPS and minimum-duration enforcement. Requirement is
    // max(min_duration, visible_characters / max_cps); extending the current
    // event and shifting all following events keeps gaps and ordering intact.
    for index in 0..cues.len() {
        let characters = visible_char_count(&cues[index].text);
        let reading_time = Rational::from_int(characters as i64).checked_div(options.max_cps)?;
        let required = if reading_time > options.min_duration.as_rational() {
            ceil_millis(Time::from_rational_seconds(reading_time))?
        } else {
            ceil_millis(options.min_duration)?
        };
        let duration =
            cues[index].end.as_rational().checked_sub(cues[index].start.as_rational())?;
        if duration < required.as_rational() {
            let delta = required.as_rational().checked_sub(duration)?;
            cues[index].end = time_add(cues[index].end, delta)?;
            for following in cues.iter_mut().skip(index + 1) {
                following.start = time_add(following.start, delta)?;
                following.end = time_add(following.end, delta)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{visible_text, word_tokens};
    use crate::time::Time;

    fn cue(start: i64, end: i64, text: &str) -> PlainCue {
        PlainCue {
            start: Time::from_millis(start),
            end: Time::from_millis(end),
            text: text.to_owned(),
        }
    }

    fn options(preset: Preset) -> ReflowOptions {
        ReflowOptions::new(preset, FrameRate::FPS_24).unwrap()
    }

    #[test]
    fn preset_uses_exact_published_values() {
        let adult = options(Preset::NetflixPtBr);
        assert_eq!(adult.max_chars_per_line, 42);
        assert_eq!(adult.max_lines, 2);
        assert_eq!(adult.max_cps, Rational::new(17, 1).unwrap());
        assert_eq!(adult.min_duration, Time::from_rational_seconds(Rational::new(5, 6).unwrap()));
        assert_eq!(adult.max_duration, Time::from_rational_seconds(Rational::from_int(7)));
        assert_eq!(adult.min_gap, Time::from_millis(84));

        let children = options(Preset::NetflixPtBrChildren);
        assert_eq!(children.max_cps, Rational::new(13, 1).unwrap());

        let shorts = options(Preset::Shorts);
        assert_eq!(shorts.max_lines, 1);
        assert_eq!(shorts.words_per_page, 4);
        assert!(Preset::parse("netflix-ptbr").is_ok());
        assert!(Preset::parse("bogus").is_err());
    }

    #[test]
    fn wraps_after_punctuation_and_before_preposition() {
        let text = "Eu queria falar com vocês. Sobre isso, amanhã.";
        let pieces = tokenize(text);
        let lines = wrap(&pieces, 42, 2);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines.join(" "), text);
        assert!(lines.iter().all(|line| visible_char_count(line) <= 42));
        // The break lands after the full stop rather than inside "com vocês".
        assert!(lines[0].ends_with('.'));
    }

    #[test]
    fn never_splits_article_from_noun_when_alternative_exists() {
        let text = "Ela viu o filme ontem e gostou muito";
        let pieces = tokenize(text);
        let lines = wrap(&pieces, 20, 2);
        assert_eq!(lines.len(), 2);
        assert!(!lines[0].trim_end().ends_with("o"));
        assert!(!lines[1].starts_with("filme"));
    }

    #[test]
    fn reflow_preserves_words_and_enforces_limits() {
        let mut cues = vec![
            cue(
                2_000,
                2_500,
                "Muitos acreditam que só encontramos um amor verdadeiro em nossas vidas.",
            ),
            cue(2_550, 2_800, "O amor é uma boa razão para que tudo fracasse."),
        ];
        let before: Vec<String> = cues.iter().flat_map(|cue| word_tokens(&cue.text)).collect();
        let config = options(Preset::NetflixPtBr);
        reflow(&mut cues, &config).unwrap();
        let after: Vec<String> = cues.iter().flat_map(|cue| word_tokens(&cue.text)).collect();
        assert_eq!(before, after);

        let lint_config = LintConfig::netflix_ptbr(FrameRate::FPS_24, false).unwrap();
        let report = crate::lint::LintReport::new(&cues, &lint_config);
        assert!(!report.issues.iter().any(|item| item.rule == crate::lint::LintRule::Cps));
        assert!(!report.issues.iter().any(|item| item.rule == crate::lint::LintRule::LineLength));
        assert!(!report.issues.iter().any(|item| item.rule == crate::lint::LintRule::TooManyLines));
        assert!(!report.issues.iter().any(|item| item.rule == crate::lint::LintRule::Overlap));
        assert!(!report.issues.iter().any(|item| item.rule == crate::lint::LintRule::Gap));
    }

    #[test]
    fn shorts_pages_limit_words_per_page() {
        let mut cues = vec![cue(1_000, 6_000, "um dois três quatro cinco seis sete oito")];
        reflow(&mut cues, &options(Preset::Shorts)).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(word_tokens(&cues[0].text), ["um", "dois", "três", "quatro"]);
        assert_eq!(word_tokens(&cues[1].text), ["cinco", "seis", "sete", "oito"]);
        assert!(cues.iter().all(|cue| !cue.text.contains('\n')));
    }

    #[test]
    fn markup_survives_wrapping() {
        let text = "<i>Primeira frase com ênfase.</i> Segunda parte bem longa aqui.";
        let pieces = tokenize(text);
        let lines = wrap(&pieces, 42, 2);
        assert_eq!(lines.join(" "), text);
        assert!(lines[0].contains("<i>"));
        assert_eq!(visible_text(&lines.join(" ")), visible_text(text));
    }
}
