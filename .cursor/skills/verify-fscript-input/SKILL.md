---
name: verify-fscript-input
description: Check media preparation from file or stdin when changing fscript input routing, temporary media, audio probing, or normalization.
---

# Verify media preparation

`fscript -` rejects terminal input before reading. If the guard regresses, a command expecting piped audio can wait on the terminal instead of telling the user how to supply media.

Run with Java 11 or later and `TLA2TOOLS` pointing to the official [TLC 1.7.4 JAR](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4).

```sh
sh scripts/check-cli-state-machine.sh .cursor/skills/verify-fscript-input/Input.tla
cargo test --locked --lib input::tests
cargo test --locked --lib audio::tests
cargo test --locked --test cli_io
```

## Coverage

Reviewed at `7562797c4462045c2a3edc2ceaf9595ed6a5d626`. The Homebrew tag `v1.1.3` at `751f294296a0034360231d6acfe9cf3c700cb0a8` routes files and downloaded audio but has no stdin spool branch.

| Transition | Source |
| --- | --- |
| Recognize dash input, file paths and remote input | `src/remote.rs`, `infer_input_source` and `src/app.rs`, `run` |
| Reject a terminal, spool nonempty bytes, flush and retain the temporary file | `src/input.rs`, `spool_stdin` |
| Probe media, reuse supported input or convert into a temporary WAV | `src/audio.rs`, `probe_audio_with`, `is_supported_audio`, `normalize_audio_with` |
| Read samples and keep normalized media through transcription and diarization | `src/transcribe.rs`, `transcribe_audio_input` |
| Deliver the result and drop the stdin spool | `src/app.rs`, stdin branch and `emit_result` |

The model bounds one invocation and one instance of each owned temporary resource. It abstracts byte copying and decoding, external-tool exit outcomes, and the internals of inference. A spooling error includes empty input. Resource release on ordinary error and completion is modeled at the relevant ownership boundary. Abrupt process termination is outside the model.

TLC checked `TerminalNeverRead` in 34 states. Supply `Input-mutant.cfg` as the second argument to see a terminal reach the reading state. Use pstack's `/statechart` to read a counterexample. In a disposable checkout, remove the `if interactive` guard and run `cargo test --locked --lib input::tests::interactive_stdin_is_rejected_before_reading`. The real source test fails when its reader is called. Restore the guard and rerun the checks, then update the reviewed commit after confirming source correspondence.
