---
name: verify-fscript-rendering
description: Check the native transcript format and speaker-turn accumulator when changing src/output.rs.
---

# Verify native transcript rendering

In `src/output.rs`, `render_output` optionally cleans repeated words, then chooses JSON, speaker turns, plain or timestamped text, SRT, or VTT. Segment-driven formats require metadata unless the transcript is empty. Subtitle output skips empty and nonpositive-duration cues. `render_speaker_lines` trims and drops empty text, groups adjacent equal normalized speaker labels, flushes on a speaker change, and flushes the last pending turn on completion.

The native accumulator starts with no speaker and timestamp zero. When the first nonempty segment also has no speaker, merging treats it as part of an existing turn and skips recording its start time. Both main at `7562797` and `v1.1.3` implement this branch.

Use Java 11 or later and set `TLA2TOOLS` to the absolute path of the official [TLC 1.7.4 JAR](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4).

```sh
dir=.cursor/skills/verify-fscript-rendering
sh scripts/check-cli-state-machine.sh "$dir/Turns.tla"
sh scripts/check-cli-state-machine.sh "$dir/Turns.tla" "$dir/Turns-active.cfg"
cargo test --locked --lib output::tests
```

The baseline must fail `TimestampFromFirstSegment`. The alternative requires an active turn before merging and must pass. The model uses a few representative streams of at most three nonblank segments, with positive distinct start times. Formats outside the speaker-turn state and subtitle timing calculations are outside the model.

In a disposable source checkout, run `python3 path/to/extend-render-tests.py` from its root, then run `cargo test --locked --lib first_unlabeled_turn_keeps_its_start_time`. Expect the fixture to fail with a zero timestamp. Add `!current_parts.is_empty()` to the merge guard in that checkout and rerun to check the alternative. Discard the source changes after the replay.

Use pstack’s `/statechart` to inspect the shortest path for a failing rule or visualize the checked flow. When the source changes, update the reviewed revision after rechecking the model and source fixture.
