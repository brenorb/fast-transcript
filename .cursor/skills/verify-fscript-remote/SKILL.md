---
name: verify-fscript-remote
description: Model-check how fscript chooses manual platform subtitles or downloads audio when changing remote input handling, the local override, or diarization routing.
---

# Verify fscript remote transcript routing

When a user asks for `--local`, fscript must use the audio path even if usable platform captions exist. Otherwise the user receives a platform transcript after explicitly requesting local inference.

Run from the repository root with Java 11 or later and `TLA2TOOLS` set to the absolute path of the official [TLA+ 1.7.4 release JAR](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4).

```sh
sh scripts/check-cli-state-machine.sh .cursor/skills/verify-fscript-remote/Remote.tla
```

## Coverage

Reviewed at `7562797c4462045c2a3edc2ceaf9595ed6a5d626`. The cited Homebrew release `v1.1.3` at `751f294296a0034360231d6acfe9cf3c700cb0a8` uses the same remote routing guard and fallback in `src/lib.rs`, `handle_remote_input`.

| Model | Source on main |
| --- | --- |
| Inspect the remote media and choose manual captions only when neither local override nor diarization applies. | `src/app.rs`, `handle_remote_input`, and `src/remote.rs`, `load_remote_video_info` |
| A missing manual track, empty decoded text, download failure or parse failure leads to downloading audio. | `src/remote.rs`, `pick_manual_subtitle_language`, `download_manual_remote_transcript`, and `src/app.rs`, `handle_remote_input` |
| Downloaded audio is transcribed and rendered, or the operation fails. | `src/remote.rs`, `download_remote_audio`, and `src/app.rs`, `handle_remote_input`, `emit_result`, and `emit_result_with_status` |

The model bounds one URL and exhausts every combination of a local override, an effective diarization backend, and a reported manual track. It assumes parsing has selected the effective options and the output destination resolves. It represents subprocess outcomes symbolically and abstracts caption language, decoding, model inference, and output formatting.

The baseline exhausted 68 states. Removing the local override from the model allows subtitle fetching in two steps. Removing the corresponding condition in a scratch build made the CLI fixture catch a subtitle fetch under `--local`. The fixture passed again after rebuilding the original source.

## Verify a change

1. Compare the covered functions with the reviewed commit. Update the actions when route selection, fallbacks, or error handling changes.
2. Run the checker. Pass `.cursor/skills/verify-fscript-remote/Remote-broken.cfg` as the second argument to confirm that `ExplicitLocalSkipsSubtitles` fails without the guard. Interpret counterexamples with pstack's `/statechart`.
3. Run `cargo build --locked` and `python3 .cursor/skills/verify-fscript-remote/references/check-remote-route.py target/debug/fscript`. The fake downloader exercises the caption shortcut and rejects any `--local` run that requests subtitles. For a source mutation in a disposable checkout, remove `!args.force_local_for_remote &&` from the guard and repeat. Restore the original guard before rerunning the baseline.
4. Advance the reviewed commit only after checking that the model describes the code.
