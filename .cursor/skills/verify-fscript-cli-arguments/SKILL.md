---
name: verify-fscript-cli-arguments
description: Check fscript end-of-options handling and exclusive format and destination choices when changing argument normalization.
---

# Verify fscript argument dispatch

Main at `7562797` normalizes spaced optional values in `src/cli.rs::parse_command` up to `--`, then Clap assigns positional and named arguments. `parse_args_with_diarization_status_and_default` rejects conflicts among output modes, positional and named destinations, stdout and a file path, raw and clean, model selectors, and incompatible diarization options. `src/app.rs::run` handles informational and settings paths before media processing. The cited `v1.1.3` uses a manual argument scanner. It checks repeated destination sources and exclusive format flags as it scans.

Use Java 11 or later and set `TLA2TOOLS` to the absolute path of the official [TLC 1.7.4 JAR](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4).

```sh
dir=.cursor/skills/verify-fscript-cli-arguments
sh scripts/check-cli-state-machine.sh "$dir/Arguments.tla"
sh scripts/check-cli-state-machine.sh "$dir/Arguments.tla" "$dir/Arguments-mutant.cfg"
cargo build --locked --bin fscript
python3 "$dir/references/check-arguments.py" target/debug/fscript
cargo test --locked --test cli_contract
```

The model uses representative streams of at most three tokens, including repeated and conflicting format flags, a destination conflict, and literals after the option boundary. It checks that accepted output choices are unambiguous and that the representative suffix is literal. The conflict mutant must fail. The source fixture executes the production CLI on fixed local invocations. In a disposable checkout, bypass the `formats.len() > 1` guard and rebuild. The first fixture case must fail its expected diagnostic. Clap internals, the semantics of individual values, and media processing after accepted arguments are outside the finite model.

Use pstack’s `/statechart` to inspect the shortest path for a failing rule or visualize the checked flow. When the source changes, update the reviewed revision after rechecking the model and source fixture.
