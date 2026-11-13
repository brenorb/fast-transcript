---
name: verify-fscript-parser-harness
description: Check the bounded input normalization in the fscript parser test harness.
---

# Verify parser harness input normalization

The manually dispatched `.github/workflows/fuzz.yml` starts the existing `cli_args` parser test target with a time limit. In `fuzz/fuzz_targets/cli_args.rs`, `bytes_to_args` prefers a NUL separator when present, considers at most 16 chunks, truncates each raw chunk to 64 bytes before lossy UTF-8 conversion, trims and drops blank chunks, and inserts `audio.wav` if nothing remains. The harness then calls the argument parser. The target source is identical at main `7562797` and the cited `v1.1.3`.

Use Java 11 or later and set `TLA2TOOLS` to the absolute path of the official [TLC 1.7.4 JAR](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4).

```sh
dir=.cursor/skills/verify-fscript-parser-harness
sh scripts/check-cli-state-machine.sh "$dir/Harness.tla"
sh scripts/check-cli-state-machine.sh "$dir/Harness.tla" "$dir/Harness-mutant.cfg"
python3 "$dir/references/check-harness.py"
```

The model checks delimiter selection, the nonempty parser input, and raw input limits. Chunk counts and byte widths use representative boundary values. The bounds mutant must fail. The source fixture compiles the actual normalizer function with six fixed local inputs and must fail if the raw byte cap changes from 64 to 65. The fixture stops before invoking the parser or fuzz runner.

Use pstack’s `/statechart` to inspect the shortest path for a failing rule or visualize the checked flow. When the source changes, update the reviewed revision after rechecking the model and source fixture.
