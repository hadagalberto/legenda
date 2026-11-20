# Contributing to legenda

Thanks for helping improve `legenda`. This document describes the quality bar
every change must meet. The authoritative feature list is [SPEC.md](SPEC.md);
the Netflix values are sourced in
[docs/netflix-ptbr-style-guide.md](docs/netflix-ptbr-style-guide.md).

## Development setup

```sh
git clone https://github.com/HadagalbertoJunior/legenda.git
cd legenda
cargo build --workspace
```

Rust stable with MSRV 1.85 is required. Components: `rustfmt` and `clippy`.
No system library is needed for the library or CLI; install `ffmpeg` and
`ffprobe` to run the `burn` end-to-end test (CI installs them on every matrix
platform).

## Quality gate — run before every commit

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo test --workspace --release
```

The same commands run in CI. Warnings are errors (`#![deny(warnings)]` in both
crates plus `-D warnings`). Never silence a warning by weakening a test,
adding `#[allow(...)]` around real code, or deleting coverage. Fix the root
cause instead.

## Tests

- No stubs, `todo!()`, `unimplemented!()`, ignored tests or placeholders.
- Every test must assert real behaviour using known-valid **and** known-invalid
  vectors. A snapshot without an explicit assertion is not enough on its own.
- Unit tests live next to the code they cover; integration tests live in
  `legenda-core/tests/` and `legenda/tests/`.
- The parsing layer has a byte-exact corpus of 57 fixtures under
  `legenda-core/tests/fixtures/`. `tests/corpus.rs` generates four tests per
  fixture: parsed-model snapshot with known-vector assertions, rewritten-output
  snapshot, lossless round trip and byte-stable second write. Add new edge
  cases there, with the matching snapshots in `tests/snapshots/` and a
  hand-verified expected cue count, start, end and first cue text.
- Generate or refresh snapshots deliberately:

  ```sh
  INSTA_UPDATE=always cargo test -p legenda-core --test corpus
  ```

  Then review the diff before committing. On PowerShell set the variable with
  `$env:INSTA_UPDATE='always'` first.
- Property tests cover shift/unshift identity, exact scale round trips,
  sync/inverse identity, sorted non-overlapping results after fix operations,
  reflow word preservation and line/CPS limits, page grouping and per-format
  parse→write→parse round trips.

## Cross-platform rules

The CI matrix runs Ubuntu, Windows and macOS. Every step in
`.github/workflows/ci.yml` must work on all three. Keep these lessons in mind:

- `.gitattributes` declares `* text=auto eol=lf` and marks byte-exact fixtures
  as `binary`. Formatter output is LF (`rustfmt.toml` sets
  `newline_style = "Unix"`). Never commit CRLF in source or snapshot files.
- Never locate `cargo`, `rustc`, `npm` or any other toolchain through the
  language runtime's binary path. CI installs Rust with
  `dtolnay/rust-toolchain`; integration tests that need the CLI use Cargo's
  `env!("CARGO_BIN_EXE_legenda")`, and the workflow builds before testing.
- Tests must not depend on OS- or ICU-specific behaviour: no locale/culture
  formatting, no `TextDecoder`-style label tables, no host timezone, and no
  assumptions about path separators or line endings. Normalise input in the
  parser (`text_util::normalize_newlines`) and use `/` in ffmpeg filter paths.
- Paths in tests are built with `Path::join`; UTF-8 paths are used for ffmpeg
  filter expressions and metacharacters are escaped by `burn::escape_filter_path`.
- The e2e burn test generates its video with ffmpeg `lavfi` and asserts the
  output duration with ffprobe; it never assumes a pre-existing media file.

## Commits

Use Conventional Commits, for example:

```text
docs: add bilingual README and CI lint recipes
test: expand tolerant-parser fixture corpus with snapshots
fix(lint): apply shorts limits when --preset shorts is selected
```

Keep commits local. **Never** run `git push`, `gh`, `cargo publish` or any
other publishing command; release and publication are out of scope.

## License

By contributing you agree that your contributions are licensed under the MIT
License, as found in [LICENSE](LICENSE).
