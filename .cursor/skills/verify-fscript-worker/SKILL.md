---
name: verify-fscript-worker
description: Check Python ASR worker readiness, request/reply order, temporary audio lifetime, and teardown when changing the Photon or Phonon engine.
---

# Verify the model worker lifecycle

The CLI must validate the worker's startup message before it sends audio. Otherwise a runtime that reports it has no loaded model can receive a transcription request.

Run with Java 11 or later and `TLA2TOOLS` pointing to the official [TLC 1.7.4 JAR](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4).

```sh
sh scripts/check-cli-state-machine.sh .cursor/skills/verify-fscript-worker/Worker.tla
cargo build --locked --bin fscript
python3 .cursor/skills/verify-fscript-worker/references/check-worker-ready.py target/debug/fscript
```

## Coverage

Reviewed at `7562797c4462045c2a3edc2ceaf9595ed6a5d626`. The cited `v1.1.3` at `751f294296a0034360231d6acfe9cf3c700cb0a8` predates the Photon and Phonon worker.

| Transition | Source |
| --- | --- |
| Select native or SDK engine, start the worker and check the ready reply | `src/engine.rs`, `load_engine` and `PythonEngine::load` |
| Load the model, report readiness, process requests until input closes | `src/python_worker.py`, `main` |
| Write temporary WAV, send one JSON request, read and decode one reply, then release the file | `PythonEngine::transcribe`, `read_response`, `worker_audio` |
| Close stdin, attempt kill and wait on drop | `PythonEngine::drop` and the worker release in `src/transcribe.rs` |

The model checks one worker and at most two sequential audio requests. It abstracts readiness to a valid or invalid reply and groups child I/O, EOF and protocol errors. A blocked read can wait for the worker. The invariant concerns whether audio is sent and makes no timing guarantee. SDK setup and actual model outputs are outside the abstraction.

TLC checked `SendRequiresValidStartup` in 21 states. Supply `Worker-mutant.cfg` as the second argument and use pstack's `/statechart` to interpret the failing path. In a disposable checkout, remove only `!info.ready ||` from the startup guard and rebuild. The local fixture records a request sent to a worker that replied `ready: false`. Restore the check and rerun before advancing the reviewed commit.
