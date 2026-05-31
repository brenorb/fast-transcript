# Subtitle Experiments Plan

Date: 2026-05-31

## Goal

Keep subtitle timing and rendering experiments separate from the stable CLI/output work until subtitle behavior is validated well enough for merge.

## Current Branch Split

- Stable staging line:
  - worktree: `/Users/breno/Documents/code/PROJECTS/fast-transcript-staging`
  - branch: `staging/text-subtitle-base`
  - base feature commit: `8574b54` `feat: add text and subtitle output formats`
  - additional stable commit: `f904337` `feat: add opt-in transcript cleanup`
- Experimental subtitle line:
  - worktree: `/Users/breno/Documents/code/PROJECTS/fast-transcript`
  - branch: `feat/fscript-2-diarization`
  - experimental subtitle commits currently on top:
    - `aecabfd` `fix: shorten overlong subtitle cues`
    - `7ef6fff` `fix: split oversized subtitle fallback segments`
  - there is also an uncommitted local renderer adjustment in `src/main.rs`

## What Is Considered Stable

- JSON output
- diarized JSON output
- diarized script output
- raw text output
- timestamped raw text output
- SRT/VTT output as a feature surface
- opt-in transcript cleanup via `--clean`

## What Is Still Experimental

- subtitle cue end-time shortening heuristics
- subtitle cue splitting/spreading heuristics for coarse ASR segments
- visual subtitle QA against real films
- any change whose purpose is to improve subtitle timing rather than add/maintain format support

## Rules For The Next Session

1. Do not merge subtitle timing experiments into the staging branch by default.
2. Treat `staging/text-subtitle-base` as the merge candidate for `main`.
3. If subtitle work continues, do it on the experimental branch or a fresh branch forked from it.
4. If a subtitle improvement is proposed for merge, require:
   - regression tests
   - validation on at least one real subtitle file
   - explicit confirmation that it does not reduce speech coverage

## Immediate Cleanup Tasks On Staging

1. Fix `cargo clippy --all-targets -- -D warnings`
   - current issue: dead code in `parse_vtt_text`
2. Add missing tests for stable staging behavior:
   - `-c` alias parsing
   - cleaned script output
   - cleaned SRT output
   - optionally cleaned VTT output
3. Re-run:
   - `cargo fmt --check`
   - `cargo test`
   - `cargo clippy --all-targets -- -D warnings`

## Test And Validation Notes

- Current staging status during review:
  - `cargo fmt --check` passed
  - `cargo test` passed
  - `cargo clippy --all-targets -- -D warnings` failed only because `parse_vtt_text` is dead code
- Mutation testing is not wired in yet:
  - `cargo-mutants` was not installed in the environment during review
  - do not block merge on mutation testing
  - if added later, focus it on text cleaning and chunk-merging helpers first
- Realistic validation priority:
  1. unit tests for format behavior
  2. lint-clean build
  3. one or two real-file smoke tests
  4. only then optional deeper mutation/property testing

## Coverage Gaps To Close

- no explicit test for short alias `-c`
- no explicit test proving `--clean` affects `--script`
- no explicit test proving `--clean` affects `--srt`
- no explicit test proving `--clean` affects `--vtt`
- remote manual-subtitle path could use one integration-style test if we decide to harden that flow further

## Complexity Review

- `src/main.rs` is still too large and mixes too many responsibilities
- main complexity hotspots identified during review:
  - `parse_args`
  - `main`
  - `transcribe_audio_input`
- current smell to remove later:
  - remote subtitle flow builds a full benchmark struct just to reuse rendering
- merge is still reasonable after cleanup, but this is not yet a pleasant codebase shape for repeated extension

## Follow-Up Refactor Tasks

These are not required before merge, but they are the right next cleanup branch:

1. Move CLI parsing out of `src/main.rs`
2. Move remote subtitle/audio download flow out of `src/main.rs`
3. Move output rendering into its own module
4. Introduce a smaller transcript/output DTO so rendering does not depend on the full benchmark struct
5. Split stable transcript-format logic from subtitle-quality heuristics so future subtitle experiments cannot spill into the merge candidate branch by accident

## Merge Strategy

Preferred order:

1. Finish staging cleanup
2. Test `staging/text-subtitle-base`
3. Merge stable staging line into `main`
4. Resume subtitle experiments separately

## Decision Record

- We explicitly prefer lingering subtitles over losing spoken content.
- We do not yet trust subtitle timing heuristics enough to merge them into the stable line.
- Subtitle experiments should be judged as subtitle-quality work, not as core transcript-format work.
