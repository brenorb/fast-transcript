---
name: verify-fscript-chunking
description: Check audio range planning, inference iteration, overlap merging, and completion when changing fscript chunking.
---

# Verify chunk planning and inference

Every sample must appear in at least one planned range before transcription starts. Advancing the next range too far can silently omit part of the recording.

Run with Java 11 or later and `TLA2TOOLS` pointing to the official [TLC 1.7.4 JAR](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4).

```sh
sh scripts/check-cli-state-machine.sh .cursor/skills/verify-fscript-chunking/Chunks.tla
cargo test --locked --lib inference::tests
cargo test --locked --lib transcript::tests
```

## Coverage

Reviewed at `7562797c4462045c2a3edc2ceaf9595ed6a5d626` and tag `v1.1.3` at `751f294296a0034360231d6acfe9cf3c700cb0a8`. The cited release's `src/transcribe.rs` uses the same range advance and chunk offset and merge steps.

| Transition | Source |
| --- | --- |
| Round the configured lengths to samples, reject zero size or nonpositive advance, and plan half-open ranges | `src/inference.rs`, `build_chunk_ranges` |
| Choose a whole-audio range or the chunk plan, reuse an engine, update progress and handle failure | `src/inference.rs`, `transcribe_samples`; `src/progress.rs`, `ChunkProgressReporter` |
| Offset chunk timestamps, merge text and cues, and return the completed transcript | `transcribe_samples`; `src/transcript.rs`, `merge_chunk_texts`, `merge_transcript_segments` and `transcript_segments_from_transcription` |

The model bounds zero to six samples and chunk and overlap lengths of zero to three samples after rounding. It represents coverage as a set of sample positions, and successful inference and merge as one step per range. Actual inference, progress output and content-level merge rules are abstracted.

TLC checked `InferencePlanCoversInput` in 1,066 states. Supply `Chunks-mutant.cfg` as the second argument to see an off-by-one advance skip a sample. Use pstack's `/statechart` to interpret the failing path. In a disposable checkout, add one to `start += step` and run `cargo test --locked --lib inference::tests::build_chunk_ranges_splits_audio`. The current test catches the gap. Restore the advance and rerun before advancing the reviewed commit.
