# legenda — SPEC

**legenda**: a fast single-binary Rust CLI + library for subtitles aimed at creators: parse/write SRT, WebVTT and ASS/SSA; shift, scale, resync;
reflow lines to professional guidelines; and turn word-level transcripts (WhisperX/whisper.cpp/OpenAI verbose JSON) into **animated karaoke-style ASS captions for vertical shorts**
(highlight the current word, pop-in, emphasis colors) ready to burn in with ffmpeg. Open source, MIT, "Hadagalberto Junior".
NOT a toy: no stubs, no `todo!()`, `unimplemented!()`, ignored or placeholder tests.

## Stack
Rust stable, workspace `legenda-core` (lib) + `legenda` (CLI, clap derive). clippy -D warnings, rustfmt (with `.gitattributes` `* text=auto eol=lf`), proptest, insta snapshots,
criterion benchmark. GitHub Actions CI ubuntu/windows/macos (install ffmpeg for e2e). README.md + README.pt-BR.md, CHANGELOG, CONTRIBUTING, LICENSE.

## Formats
- **SRT**: tolerant parser (BOM, CRLF/LF/CR, missing indices, `,` or `.` millisecond separators, extra blank lines, HTML-ish tags `<i> <b> <font>`), strict writer.
- **WebVTT**: header, cues with ids, settings (position, line, align), NOTE/STYLE blocks preserved, timestamps without hours.
- **ASS/SSA**: [Script Info], [V4+ Styles] full field parsing, [Events] Dialogue with override tags preserved; writer producing files libass renders correctly.
- Encoding detection (UTF-8/UTF-16 BOM, windows-1252 fallback for legacy pt-BR SRTs) — implement in-house.
- Lossless round-trip tests for each format (parse → write → parse equal), conversions between formats with documented mapping of styling.

## Operations
- `shift ±time`, `scale` (framerate conversion 23.976↔25 etc. with exact rationals), two-point `sync` (map two reference timestamps), `clamp`, merge/split files, remove hearing-impaired text `[música]`, `(risos)`, speaker labels.
- **Reflow** to guidelines with presets: `netflix-ptbr` (max 42 chars/line, 2 lines, max 20 CPS? — implement the published Netflix Brazilian Portuguese Timed Text Style Guide values; fetch and cite the official page during work, never guess), `shorts` (1 line, ~4 words, large text). Line breaking by linguistic rules (don't split article+noun, preposition at line end avoided, balanced lines), min/max duration, min gap (2 frames), CPS enforcement by extending/merging.
- `lint`: report CPS violations, overlaps, too-short/long cues, line length, gaps < 2 frames; JSON output; exit code non-zero when violations.

## Word-level karaoke captions (the headline feature)
- Inputs: WhisperX JSON (`segments[].words[] {word,start,end,score}`), whisper.cpp JSON (`-ojf` full with tokens), OpenAI `verbose_json` with `words`. Robust to missing word timings (interpolate within segment).
- Grouping into caption "pages" by max words/chars, punctuation, pauses (> 300 ms), and never splitting a word.
- ASS generation for 1080x1920 (configurable): style presets `hormozi` (bold uppercase, current word in accent color + slight scale pop), `clean` (white, subtle box), `karaoke` (`\k` fill), `bounce`; per-word timing with `\t` transforms; emphasis words list (colored); safe-area margins for TikTok/Reels/Shorts UI; font/outline/shadow options; emoji passthrough.
- `legenda burn video.mp4 captions.ass -o out.mp4` wraps ffmpeg (`subtitles=` filter with proper escaping on Windows paths), hardware encoder passthrough.

## Tests
- Fixture corpus of tolerant-parsing edge cases; insta snapshots for every writer and every karaoke preset; proptest invariants (cues sorted, non-overlapping after fix ops, shift then unshift identity, reflow never exceeds limits and never loses words).
- E2E: generate a short video with ffmpeg lavfi, burn generated ASS, assert ffmpeg exit 0 and output duration (CI has ffmpeg).
- Benchmark: parse+write 100k cues < 1 s release (assert generous bound).

## Hard constraints
- Never run `git push`, `gh`, `cargo publish`, or publish anything. Commit locally after each milestone. Stay inside this repository.
