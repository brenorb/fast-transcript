# Changelog

## 1.0.3 - 2026-06-16

- fix the release workflow artifact naming so the PyPI publish job downloads the macOS wheel artifact produced by the build matrix
- keep the release archive naming aligned with Homebrew while restoring a successful PyPI publish path

## 1.0.2 - 2026-06-16

- document local video files as valid `fscript` inputs alongside audio files and remote media URLs
- switch help and README usage examples from `<audio-or-url>` to `<media-or-url>` so the CLI surface matches real behavior
- add release artifact checks so the built binary and the published PyPI wheel must expose the same version and help output
- restore architecture-specific GitHub release archive names so Homebrew and the release workflow target the same asset names

## 1.0.1 - 2026-06-09

- clarify `--speakers` help to document the explicit `timestamps` alias
- tighten the `fscript --help` layout around the default invocation and grouped option sections
- default to `coreml` diarization when `fluidaudiocli` is available, with descriptive warnings before falling back to plain transcription
- sanitize user-specific home directory paths in help output

## 1.0.0 - 2026-05-31

- redesign the CLI around a v1 default flow: `fscript <audio>` now writes a cleaned, timestamped, speaker-aware transcript next to the input
- add first-class output modes for `--speakers`, `--text`, `--json`, `--srt`, and `--vtt`
- make timestamps the default for human-readable text outputs, with `=plain` as the explicit opt-out
- enable local diarization by default with `coreml`, while supporting `--backend=lseend-dihard3` and `--backend=none`
- add `--raw`, `--local`, `--chunk`, `--overlap`, `-n/--num-speakers`, and `-t/--threshold`, with clearer validation for conflicting flags
- default `lseend-dihard3` runs to `--threshold 0.3`
- add optional transcript cleanup that applies consistently across JSON, speakers, text, SRT, and VTT outputs
- support remote inputs with local fallback when platform subtitles are missing or insufficient
- split the old monolithic binary into focused modules for CLI parsing, audio prep, model handling, output rendering, remote handling, progress reporting, and transcription orchestration
- validate the release line with Rust tests, clippy, benchmark reruns, and real transcript comparisons against published site artifacts
- rename the Homebrew tap path to `brenorb/tap`
- users migrating from the old tap should switch from `brenorb/fast-transcript` to `brenorb/tap`

## 0.2.9 - 2026-05-26

- print the final absolute transcript path to `stdout` when writing to a file
- keep progress and completion status on `stderr` so `fscript` stays shell-friendly
- cover the new file-output contract with Rust tests
