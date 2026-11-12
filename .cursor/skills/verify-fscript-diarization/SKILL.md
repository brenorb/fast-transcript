---
name: verify-fscript-diarization
description: Check optional speaker detection, subprocess outcomes, no-speech fallback, and speaker assignment when changing fscript diarization.
---

# Verify speaker detection and fallback

When the helper reports no speech, fscript must retain the transcript it already produced. Turning this result into a fatal error can discard a usable transcript.

Run with Java 11 or later and `TLA2TOOLS` pointing to the official [TLC 1.7.4 JAR](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4).

```sh
sh scripts/check-cli-state-machine.sh .cursor/skills/verify-fscript-diarization/Speakers.tla
cargo test --locked --lib diarization::tests
```

## Coverage

Reviewed at `7562797c4462045c2a3edc2ceaf9595ed6a5d626` and tag `v1.1.3` at `751f294296a0034360231d6acfe9cf3c700cb0a8`. The no-speech fallback and overlap-based merge are present at both refs.

| Transition | Source |
| --- | --- |
| Resolve helper availability and whether speaker detection is requested | `src/cli.rs`, `parse_args_with_diarization_status_and_default`; `src/diarization.rs`, `fluidaudio_binary_status` |
| Run the selected helper and parse its temporary output | `FluidAudioDiarizer::diarize`, `parse_diarization_output` |
| Return the unmodified transcript when skipped or after no speech, propagate other failures, or merge speakers | `maybe_diarize_segments`, `should_skip_diarization_error`, `merge_speakers_into_segments` |
| Feed the final speaker metadata and segments into delivery | `src/transcribe.rs`, `transcribe_audio_input` |

The model bounds one transcript and at most one helper invocation. It starts after CLI option resolution and abstracts the helper output to success, no speech or another failure. On success, assignment sums positive overlap by speaker for each transcript segment. Helper internals, exact timestamps and file rendering are outside the model.

TLC checked `NoSpeechKeepsTranscript` in eight states. Supply `Speakers-mutant.cfg` as the second argument to see a no-speech response become fatal. Use pstack's `/statechart` to read the failing path. In a disposable checkout, disable the `should_skip_diarization_error` guard in `maybe_diarize_segments`. The test `maybe_diarize_segments_skips_no_speech_backend_failures` catches the regression. Restore and rerun before advancing the reviewed commit.
