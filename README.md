# legenda

A fast, single-binary Rust CLI and library for creators' subtitles.

[Português (Brasil)](README.pt-BR.md)

`legenda` parses and writes SRT, WebVTT and ASS/SSA with tolerant, byte-exact
input handling; shifts, rescales, synchronises, reflows and lints cues with
exact rational timing; and turns WhisperX / whisper.cpp / OpenAI word-level
transcripts into animated karaoke-style ASS captions for vertical shorts,
ready to burn in with ffmpeg.

- **No external encoding crates**: UTF-8 (with BOM), UTF-16 LE/BE (BOM) and a
  windows-1252 fallback for legacy Brazilian Portuguese files are detected in
  house.
- **No floating-point timing decisions**: every timestamp is an exact
  rational, so 23.976 ↔ 25 fps conversions and shift/unshift round trips are
  lossless.
- **Guideline-aware**: the `netflix-ptbr` preset uses the published Netflix
  values (42 chars/line, 2 lines, 17 CPS adults / 13 CPS children, 5/6 s–7 s).
  Sources are cited in [docs/netflix-ptbr-style-guide.md](docs/netflix-ptbr-style-guide.md).

MIT licensed. Author: Hadagalberto Junior.

## Install

Requires Rust stable (MSRV 1.85). No system libraries are needed to build the
CLI; ffmpeg is only required for the `burn` command and the end-to-end test.

```sh
git clone https://github.com/HadagalbertoJunior/legenda.git
cd legenda
cargo build --release
# binary: target/release/legenda  (target\release\legenda.exe on Windows)
```

To install the binary into your cargo bin directory:

```sh
cargo install --path legenda
legenda --help
```

From a checkout, run the whole quality gate with:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --release
```

## Command overview

| Command | Purpose |
| --- | --- |
| `convert IN OUT` | Convert between SRT, VTT and ASS (format from extension; `-f`/`-t` override). |
| `info IN` | Report format, detected encoding, cue count and timing range. |
| `shift IN OUT --by 1.5s` | Shift every cue; signed seconds/ms or a timestamp. |
| `scale IN OUT --from 23.976 --to 25` | Exact framerate conversion. |
| `sync IN OUT --from-a … --to-a … --from-b … --to-b …` | Two-point affine resync. |
| `clamp IN OUT --start … --end …` | Keep only cues overlapping a range. |
| `merge A B -o OUT` / `split IN A B --at …` | Concatenate chronologically / split. |
| `strip-hi`, `strip-speakers`, `clean` | Remove `[música]`, `(risos)`, `♪…♪` and/or `JOÃO:` labels. |
| `lint IN --json [--preset …] [--fps …]` | Report violations; exit 1 when dirty, 2 on error. |
| `reflow IN OUT --preset netflix-ptbr\|shorts` | Linguistic line breaks, CPS and duration fixes. |
| `karaoke TRANSCRIPT.json -o OUT.ass --preset …` | Word-level animated ASS captions. |
| `burn VIDEO CAPTIONS.ass -o OUT.mp4` | Burn ASS into video with ffmpeg. |

Run `legenda <command> --help` for every flag.

## Recipe: WhisperX → shorts captions → burn

Assume `video.mp4` (vertical, 1080×1920) and a WhisperX JSON export with
`segments[].words[]` containing `word`, `start`, `end` and `score`.

```sh
# 1. Generate animated ASS pages (one page per ~8 words / punctuation / pause).
legenda karaoke whisperx.json -o shorts.ass --preset hormozi

# 2. Optional: tune typography and the platform safe area.
legenda karaoke whisperx.json -o shorts.ass --preset bounce \
  --font Anton --font-size 96 --outline 6 \
  --safe-top 180 --safe-bottom 420 \
  --emphasis nunca --emphasis jamais

# 3. Burn with ffmpeg (software encoder by default).
legenda burn video.mp4 shorts.ass -o reel.mp4

# Hardware encoder passthrough (flags forwarded verbatim):
legenda burn video.mp4 shorts.ass -o reel.mp4 \
  --encoder h264_nvenc --encoder-option preset=p4 --encoder-option crf=28
```

Other transcript shapes work with the same command: whisper.cpp `-ojf` JSON
(`transcription[].tokens[].offsets`, milliseconds) and OpenAI
`verbose_json` (`words[]`, seconds). Missing word timings are interpolated
within their segment. Presets: `hormozi` (bold uppercase + accent pop),
`clean` (soft reveal on a subtle box), `karaoke` (`\k` fill) and `bounce`
(overshoot-and-settle). Emoji pass through untouched; braces are escaped.

The caption file is copied to a temporary ASCII-only name before burning, so
filtergraph metacharacters in your paths (`, ; [ ] '`), spaces and accents never
reach ffmpeg's two-level escaping. Video and output paths are passed verbatim.
Works on Linux, macOS and Windows.

## Recipe: Netflix-style reflow

```sh
# Reflow to the published pt-BR style guide at 25 fps.
legenda reflow legendas.srt legendas-reflow.srt \
  --preset netflix-ptbr --fps 25

# Children's reading speed uses 13 CPS instead of 17.
legenda reflow legendas.srt legendas-criancas.srt \
  --preset netflix-ptbr-children --fps 25

# Check the result; exit 0 means clean, 1 means violations, 2 means error.
legenda lint legendas-reflow.srt --preset netflix-ptbr --fps 25 --json
```

Reflow breaks after punctuation and before conjunctions/prepositions, never
splits an article from its noun, balances lines bottom-heavy, bounds each cue
to 5/6 s–7 s, enforces 17/13 CPS by extending events and shifting later cues,
and keeps a two-frame minimum gap. Overlong events are split at word
boundaries; no words are lost. The `shorts` preset uses one line per page of
roughly four words.

## Recipe: lint in CI

`lint` is machine-readable and uses the exit code to fail a build:

```yaml
- name: Check subtitle timing
  shell: bash
  run: |
    legenda lint captions/pt-BR.srt --json > lint-report.json || code=$?
    cat lint-report.json
    # 0 = clean, 1 = violations (fail the job), 2 = usage/IO error
    test "${code:-0}" -le 1
    test "${code:-0}" -ne 1
```

A simpler step for a single file:

```yaml
- name: Lint captions
  run: cargo run --quiet --bin legenda -- lint captions/pt-BR.srt --fps 25
```

The JSON report contains `cues`, `violations`, `by_rule` and `issues`. Rules:
`invalid_timing`, `cps`, `overlap`, `gap`, `too_short`, `too_long`,
`line_length`, `too_many_lines`.

## Library

```rust
use legenda_core::{Format, Subtitle};

let bytes = std::fs::read("captions.srt")?;
let subtitles = Subtitle::decode(&bytes, Format::Srt)?;   // encoding auto-detected
let cues = subtitles.plain_cues()?;
let vtt = legenda_core::convert::convert(&subtitles, Format::Vtt)?;
std::fs::write("captions.vtt", vtt.encode()?)?;
```

## Project layout

- `legenda-core/` — library: parsers, writers, conversions, timing, encoding,
  operations, reflow, lint, transcript ingestion, karaoke rendering and ffmpeg
  argument building.
- `legenda/` — clap-derived CLI.
- `legenda-core/tests/fixtures/` — byte-exact fixture corpus (57 files across
  SRT/VTT/ASS). Parsed-model and rewritten-output insta snapshots live in
  `legenda-core/tests/snapshots/`.

See [CONTRIBUTING.md](CONTRIBUTING.md) for development and testing rules and
[CHANGELOG.md](CHANGELOG.md) for release history.

## License

MIT — see [LICENSE](LICENSE).
