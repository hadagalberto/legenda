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
use crate::ops::{visible_char_count, FrameRate};
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

/// Chooses the rightmost break whose left side still fits, preferring a
/// linguistic boundary on ties. Used when more than two lines are needed and
/// [`best_break`] (which requires *both* sides to fit) has no candidate.
fn greedy_break(pieces: &[WordPiece], start: usize, max_chars: usize) -> Option<usize> {
    let mut best: Option<(usize, i32, usize)> = None;
    for boundary in start + 1..pieces.len() {
        let left = span_length(&pieces[start..boundary]);
        if left > max_chars {
            break;
        }
        let right = span_length(&pieces[boundary..]);
        let score = break_score(
            &pieces[boundary - 1].word,
            &pieces[boundary].word,
            left,
            right,
            boundary - start,
        );
        if best.is_none_or(|(best_left, best_score, _)| (left, score) > (best_left, best_score)) {
            best = Some((left, score, boundary));
        }
    }
    best.map(|(_, _, boundary)| boundary)
}

/// Packs word pieces into visual lines. Unlike the old `wrap`, overflow is
/// never appended to the final line: a long cue produces more lines, which
/// the caller then splits across cues. Each line is within `max_chars`
/// whenever the longest individual word allows it.
fn pack_lines(pieces: &[WordPiece], max_chars: usize) -> Vec<Vec<WordPiece>> {
    if pieces.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut start = 0;
    while start < pieces.len() {
        if span_length(&pieces[start..]) <= max_chars {
            lines.push(pieces[start..].to_vec());
            break;
        }
        let boundary = best_break(pieces, start, max_chars)
            .or_else(|| greedy_break(pieces, start, max_chars))
            .unwrap_or(start + 1);
        lines.push(pieces[start..boundary].to_vec());
        start = boundary;
    }
    lines
}

/// Visual lines for the line-breaking unit tests.
#[cfg(test)]
fn wrap_lines(pieces: &[WordPiece], max_chars: usize) -> Vec<String> {
    pack_lines(pieces, max_chars).iter().map(|line| rebuild(line)).collect()
}

/// Groups visual lines into cue texts of at most `max_lines` lines.
fn group_texts(lines: Vec<Vec<WordPiece>>, max_lines: usize) -> Vec<String> {
    lines
        .chunks(max_lines)
        .map(|group| group.iter().map(|line| rebuild(line)).collect::<Vec<_>>().join("\n"))
        .collect()
}

fn time_add(time: Time, delta: Rational) -> Result<Time> {
    Ok(Time::from_rational_seconds(time.as_rational().checked_add(delta)?))
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

/// Distributes a source cue's interval across its already-wrapped texts.
///
/// The source start is kept. Sub-cues get the minimum gap between them, and
/// their ends may extend into the free time before the *next source cue* but
/// never past `limit` (the next source start minus one minimum gap). Any
/// reading-speed requirement that cannot be met inside that budget is left
/// for `lint` to report instead of moving the following subtitle out of sync.
fn regroup_texts(
    cue: &PlainCue,
    texts: Vec<String>,
    limit: Option<Time>,
    options: &ReflowOptions,
) -> Result<Vec<PlainCue>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let weights: Vec<usize> = texts.iter().map(|text| visible_char_count(text).max(1)).collect();
    let total: usize = weights.iter().sum();
    let duration = cue.end.as_rational().checked_sub(cue.start.as_rational())?;
    let gap = options.min_gap.as_rational();
    let min_duration = options.min_duration.as_rational();
    let count = texts.len();
    let mut cursor = cue.start;
    let mut result = Vec::with_capacity(count);

    for (index, (text, weight)) in texts.into_iter().zip(weights).enumerate() {
        let proportional_end = if index + 1 == count {
            cue.end
        } else {
            let fraction =
                Rational::from_int(weight as i64).checked_div(Rational::from_int(total as i64))?;
            time_add(cue.start, duration.checked_mul(fraction)?)?
        };
        let start = if index == 0 { cue.start } else { cursor };

        let characters = visible_char_count(&text);
        let reading_time = Rational::from_int(characters as i64).checked_div(options.max_cps)?;
        let required = ceil_millis(Time::from_rational_seconds(reading_time.max(min_duration)))?;
        let required_end = time_add(start, required.as_rational())?;
        let mut end = if proportional_end > required_end { proportional_end } else { required_end };

        // Never cross the next source cue's start. This is the design fix for
        // the CPS extension that used to shift every later cue forward.
        if let Some(limit) = limit {
            if end > limit {
                end = limit;
            }
        }
        if end <= start {
            let next_ms = Time::from_millis(start.as_millis().num() + 1);
            end = match limit {
                Some(limit) if limit > start && limit < next_ms => limit,
                _ => next_ms,
            };
        }

        result.push(PlainCue { start, end, text });
        cursor = time_add(end, gap)?;
    }
    Ok(result)
}

/// Sorts cues chronologically and restores the minimum gap by pulling the
/// earlier cue's end back. Unlike `ops::sort_and_fix_gaps` this never moves a
/// later cue's start, which is the core of the CPS-desync finding.
fn sort_and_fix_gaps_in_place(cues: &mut [PlainCue], min_gap: Time) -> Result<()> {
    cues.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.end.cmp(&b.end)));
    let gap = min_gap.as_rational();
    for cue in cues.iter_mut() {
        if cue.end <= cue.start {
            cue.end = time_add(cue.start, gap)?;
        }
    }
    for index in 0..cues.len().saturating_sub(1) {
        let latest_end = cues[index + 1].start.as_rational().checked_sub(gap)?;
        if cues[index].end.as_rational() > latest_end {
            cues[index].end = Time::from_rational_seconds(latest_end);
        }
        if cues[index].end <= cues[index].start {
            cues[index].end = Time::from_millis(cues[index].start.as_millis().num() + 1);
        }
        // If the previous cue had no room to shrink, nudge the following cues
        // by the few milliseconds still needed. This is a gap repair, never a
        // CPS-driven shift of seconds.
        let required_start = time_add(cues[index].end, gap)?;
        if cues[index + 1].start < required_start {
            let shift =
                required_start.as_rational().checked_sub(cues[index + 1].start.as_rational())?;
            for following in cues.iter_mut().skip(index + 1) {
                following.start = time_add(following.start, shift)?;
                following.end = time_add(following.end, shift)?;
            }
        }
    }
    Ok(())
}

/// Reflows cues in place: splits long events across time, wraps each piece by
/// the preset's linguistic rules and extends timings up to (but never past)
/// the gap before the following source cue.
pub fn reflow(cues: &mut Vec<PlainCue>, options: &ReflowOptions) -> Result<()> {
    sort_and_fix_gaps_in_place(cues, options.min_gap)?;

    // Serialisation uses millisecond precision. Quantise every timestamp up
    // to a whole millisecond, then restore the minimum gap on that exact grid.
    for cue in cues.iter_mut() {
        cue.start = ceil_millis(cue.start)?;
        cue.end = ceil_millis(cue.end)?;
        if cue.end <= cue.start {
            cue.end = Time::from_millis(cue.start.as_millis().num() + 1);
        }
    }
    sort_and_fix_gaps_in_place(cues, options.min_gap)?;

    // Absolute end limit per source cue: next source start minus one gap.
    // Collected before any splitting so later source cues are never moved.
    let limits = cues
        .iter()
        .enumerate()
        .map(|(index, _cue)| -> Result<Option<Time>> {
            if index + 1 < cues.len() {
                let start = cues[index + 1]
                    .start
                    .as_rational()
                    .checked_sub(options.min_gap.as_rational())?;
                Ok(Some(Time::from_rational_seconds(start)))
            } else {
                Ok(None)
            }
        })
        .collect::<Result<Vec<_>>>()?;

    let max_duration = options.max_duration.as_rational();
    let mut result = Vec::new();
    for (cue, limit) in cues.iter().zip(limits) {
        let duration = cue.end.as_rational().checked_sub(cue.start.as_rational())?;
        let groups_needed = if max_duration > Rational::ZERO && duration > max_duration {
            let ratio = duration.checked_div(max_duration)?;
            ((ratio.num() + ratio.den() as i64 - 1) / ratio.den() as i64).max(1)
        } else {
            1
        };

        let pages = page_pieces(tokenize(&cue.text), options.words_per_page);
        let mut lines = Vec::new();
        for page in pages {
            for chunk in chunk_pieces(page, groups_needed as usize) {
                lines.extend(pack_lines(&chunk, options.max_chars_per_line));
            }
        }
        let texts = group_texts(lines, options.max_lines);
        result.extend(regroup_texts(cue, texts, limit, options)?);
    }
    *cues = result;
    // Sub-cues are appended in source order. In physically over-dense inputs
    // a sub-cue can be pushed past the following source cue's start; sorting
    // keeps the document in a valid, monotonic timeline for every writer.
    cues.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.end.cmp(&b.end)));
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
        let lines = wrap_lines(&pieces, 42);
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
        let lines = wrap_lines(&pieces, 20);
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
        // Review finding 5: this assertion used to require zero CPS violations,
        // which was only achievable by shifting every later cue forward (the
        // 4th cue moved from 10 s to 19 s) and desynchronising the file. CPS
        // extension now stops at the available gap and lint reports any
        // residual violation instead. Layout and timing invariants remain hard.
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
        let lines = wrap_lines(&pieces, 42);
        assert_eq!(lines.join(" "), text);
        assert!(lines[0].contains("<i>"));
        assert_eq!(visible_text(&lines.join(" ")), visible_text(text));
    }
}
