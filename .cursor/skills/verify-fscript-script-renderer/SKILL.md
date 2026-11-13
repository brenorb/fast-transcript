---
name: verify-fscript-script-renderer
description: Check skipped segments, consecutive speaker grouping, and final-turn emission when changing the standalone diarized transcript renderer.
---

# Verify diarized transcript rendering

The renderer keeps a pending speaker turn. A different normalized label or disabled merging flushes the pending turn. The final pending turn must be emitted at the end of the input.

Use Java 11 or later with `TLA2TOOLS` pointing to [TLC 1.7.4](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4).

```sh
skill=.cursor/skills/verify-fscript-script-renderer
sh scripts/check-cli-state-machine.sh "$skill/Turns.tla"
sh scripts/check-cli-state-machine.sh "$skill/Turns.tla" "$skill/Turns-unmerged.cfg"
python3 -m unittest discover -s tests -p test_render_diarized_transcript_script.py
```

## Coverage

`scripts/render_diarized_transcript_script.py` is identical at main `7562797c4462045c2a3edc2ceaf9595ed6a5d626` and tag `v1.1.3` at `751f294296a0034360231d6acfe9cf3c700cb0a8`.

| Transition | Source in the renderer |
| --- | --- |
| Read and validate the transcript and select output path and options | `parse_args`, `main`, `default_output_path` |
| Skip unusable segments and normalize labels | `render_script_lines`, `normalize_speaker_label` |
| Accumulate consecutive turns or flush a pending line | `render_script_lines`, nested `flush` |
| Flush the final turn and write output | `render_script_lines`, `main` |

The model enumerates up to three input segments. It abstracts labels to two speakers and one unknown label, and maps blank or invalid segments to skip. It tracks output labels and abstracts the exact text and filesystem writes. The source tests verify emitted text and the optional unknown speaker prefix.

TLC checked 212 states with merging and 260 without merging. Supply `Turns-mutant.cfg` to see the final turn disappear. In a disposable checkout, remove the final call to `flush()` before `return lines`. Four source tests then fail. All six pass with the source restored. Rerun both configs and the tests before updating the reviewed refs.

Use pstack’s `/statechart` to inspect the shortest path for a failing rule or visualize the checked flow. When the source changes, update the reviewed revision after rechecking the model and source fixture.
