# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **Maintainer fix pass (milestone 7)**
  - `legenda burn` now probes `ffmpeg -hide_banner -filters` before spawning
    ffmpeg and fails fast with actionable, per-OS libass install instructions
    when neither the `subtitles` nor the `ass` filter exists, instead of
    surfacing ffmpeg's exit 8 `Filter not found` on macOS.
  - Probe parsing is unit-tested with known-valid and adversarial listings:
    descriptions and lookalike filter names no longer count as libass support.
  - macOS CI installs the keg-only `ffmpeg-full` formula (which includes
    libass) ahead of the default ffmpeg on `PATH`; a dedicated workflow step
    verifies the filters on every OS and fails the job loudly if absent. Burn
    e2e tests never skip in CI; locally they skip with a printed reason only
    when the probe proves libass is missing.
  - README/README.pt-BR install notes explain how to check for and install a
    libass-enabled ffmpeg on Linux, macOS and Windows.
- **Maintainer fix pass (milestone 6)**
  - `legenda burn` no longer puts user caption paths in the ffmpeg
    filtergraph. The ASS file is copied to a uniquely named ASCII-only
    temporary file and referenced with `subtitles=filename=...`, so commas,
    brackets, semicolons, apostrophes, spaces, accents and drive letters can
    no longer break two-level filtergraph escaping (the macOS CI failure).
    Video and output paths remain verbatim argv elements.
  - Consecutive karaoke pages closer than 300 ms (`--page-gap-ms`) are now
    extended to the next page's start, removing the caption blink between
    pages produced by width-aware layout splitting.
- **Karaoke caption layout and highlighting (milestone 5)**
  - Pages are now packed against the estimated glyph width and the safe area
    (`width - safe_left - safe_right`), capped at two lines and 3-4 words per
    default `hormozi` page, so long lines no longer run off the frame.
  - ASS `\t` transform times are written in milliseconds relative to the
    Dialogue start (the old centisecond values made highlights fire for 8 ms).
    The active colour is held for the whole word with an 80 ms scale pop;
    `\k`/`\kf` remain centiseconds, as libass specifies.
  - Cumulative ASS override tags are reset per word, so highlighting one word
    no longer paints every following word in the line.
  - Added a pixel-level ffmpeg e2e test that burns a black video, samples raw
    RGB frames at every word midpoint and checks both the highlight colour and
    the left/right 2% safe-area edges.

## [0.1.0] — 2026-11-20

First public release: a single-binary CLI and library for creators' subtitles.

### Added

- **Parsing, writing and conversion (milestone 1)**
  - Tolerant SRT parser (UTF-8 BOM, LF/CRLF/CR, missing indices, `,` or `.`
    millisecond separators, coordinates, extra blank lines, inline markup) and
    strict canonical writer.
  - WebVTT parser/writer preserving header metadata, cue ids, cue settings and
    `NOTE`/`STYLE`/`REGION` blocks; `MM:SS.mmm` timestamps accepted.
  - ASS/SSA field-by-field parser and libass-compatible writer, preserving
    `[Fonts]`, `[Graphics]`, `[Aegisub Project Garbage]`, comments and
    non-dialogue events; reordered `Format:` fields and SSA v4 supported.
  - In-house encoding detection: UTF-8 (BOM), UTF-16 LE/BE (BOM) and a
    windows-1252 fallback for legacy pt-BR files.
  - Lossless round trips and a documented cross-format styling mapping
    (italics/bold/underline/colour/newlines).
- **Operations, reflow and lint (milestone 2)**
  - `shift`, exact `scale` (23.976 ↔ 25 etc.), two-point `sync`, `clamp`,
    `merge`, `split`, `strip-hi`, `strip-speakers` and `clean`.
  - `netflix-ptbr`, `netflix-ptbr-children` and `shorts` reflow presets with
    published Netflix values, linguistic line breaking, CPS/duration fixes and
    a two-frame minimum gap. Sources cited in
    `docs/netflix-ptbr-style-guide.md`.
  - `lint` with JSON output, eight rule kinds and exit codes 0/1/2.
- **Word-level karaoke captions (milestone 3)**
  - WhisperX, whisper.cpp `-ojf` and OpenAI `verbose_json` transcript
    ingestion with interpolation of missing word timings.
  - `hormozi`, `clean`, `karaoke` and `bounce` ASS presets at 1080×1920 with
    configurable font, outline, shadow, safe area, emphasis list and canvas.
  - `legenda burn` wrapping ffmpeg with cross-platform `subtitles=` escaping
    and hardware encoder passthrough.
- **Documentation, corpus and cross-platform CI (milestone 4)**
  - `README.md` and `README.pt-BR.md` with installation, WhisperX → shorts →
    burn, Netflix-style reflow and lint-in-CI recipes.
  - `CONTRIBUTING.md` describing the quality gate, fixture conventions and
    cross-platform rules; this changelog.
  - A byte-exact fixture corpus of 57 SRT/VTT/ASS files with insta snapshots
    of both the parsed model and the rewritten output, proptest round trips
    per format and snapshots for all nine directed conversions.
  - CI matrix on Ubuntu, Windows and macOS with rustfmt, clippy
    (`-D warnings`), debug and release test suites and ffmpeg end-to-end burn.
  - `.gitattributes` keeps byte-exact fixtures (BOMs, UTF-16, windows-1252,
    CR-only) unchanged on every platform.

### Fixed

- `lint --preset shorts` now applies the shorts limits (one line, 34
  characters, 20 CPS) instead of silently using the Netflix thresholds.
- The release performance bound now enforces the SPEC target of parsing and
  writing 100 000 cues in under one second; debug builds retain a generous
  regression bound.

[Unreleased]: https://github.com/HadagalbertoJunior/legenda/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/HadagalbertoJunior/legenda/releases/tag/v0.1.0
