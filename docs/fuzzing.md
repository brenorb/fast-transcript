# Fuzzing

This repo includes a `cargo-fuzz` target focused on CLI argv parsing.
That is the highest-value surface for cheap fuzzing here because bugs like `-D` being treated as an output path show up before any audio/model work starts.

## Tooling

Install the required pieces:

```bash
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz --locked
```

Run a short parser session:

```bash
rustup run nightly cargo fuzz run cli_args --sanitizer address -- -max_total_time=60
```

If your shell resolves `cargo` or `rustc` from Homebrew instead of `rustup`, make sure the rustup-managed binaries are the ones being used for fuzzing. `cargo-fuzz` needs nightly sanitizer support.

## Corpus

The checked-in corpus lives in `fuzz/corpus/cli_args/` and intentionally keeps only human-readable seed files named `seed_*.txt`.
Each file is one CLI argument per line.

The seeds cover:

- default local transcription
- `--stdout` and positional `-`
- `-D` / `--no-diarization`
- `--diarize coreml` and `--diarize lseend-dihard3`
- `--output` and positional output paths
- `--text`, `--json`, `--srt`, `--vtt`
- chunk/overlap overrides
- remote URLs with `--local`
- hyphen-prefixed explicit paths such as `./-D`
- invalid combinations such as `--output` with `--stdout`, `-d` with `-D`, and unknown short flags

Generated corpus growth, artifacts, and build outputs are ignored and should not be committed by default.

## Reading Results

Useful lines from `libFuzzer` output:

- `N files found`: how many seed inputs were loaded from the starting corpus
- `cov: X`: current coverage score from instrumented execution paths
- `ft: Y`: feature count used internally by libFuzzer to distinguish interesting inputs
- `corp: A/Bb`: corpus size in files and total bytes

Example interpretation:

- starting at a higher `cov` means the seed corpus already exercises real parser paths
- ending with a higher `cov` after mutation means the fuzzer discovered additional execution paths
- no crash, panic, or sanitizer report means the run did not find an obvious memory-safety or parser-stability failure in that time window

Coverage numbers are not percentages. They are best used to compare one fuzz session against another.
