---
name: verify-fscript-benchmark-runner
description: Check ordered cold and warm worker results and safe resume when changing the canonical fscript ASR benchmark runner.
---

# Verify canonical benchmark execution

Each fresh benchmark worker produces one cold and one warm result. The parent accepts the pair after a successful exit. Resume reuses saved repetitions only when the recorded fingerprint matches the current fingerprint.

Use Java 11 or later with `TLA2TOOLS` pointing to [TLC 1.7.4](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4). The worker replay uses macOS BSD time.

```sh
skill=.cursor/skills/verify-fscript-benchmark-runner
sh scripts/check-cli-state-machine.sh "$skill/WorkerRun.tla"
sh scripts/check-cli-state-machine.sh "$skill/Resume.tla"
python3 "$skill/references/check-benchmark-worker.py"
python3 "$skill/references/check-benchmark-resume.py"
```

## Coverage

Reviewed main `7562797c4462045c2a3edc2ceaf9595ed6a5d626`. The canonical runner and its Rust worker were added after the cited `v1.1.3` tag at `751f294296a0034360231d6acfe9cf3c700cb0a8`.

| Transition | Source |
| --- | --- |
| Validate parent or worker arguments | `scripts/benchmark_asr.py`, `main` |
| Load a model, run cold then warm phases, and emit each complete result | `benchmark_asr.py`, `worker`; `examples/benchmark_asr_onnx.rs`, `main` |
| Start worker and deadline timer, collect JSON, reap process, require ordered results | `benchmark_asr.py`, `execute` |
| Fingerprint inputs and compare saved metadata before considering an existing result | `benchmark_asr.py`, `benchmark` |
| Validate audio checksums and worker result shape, score, and save fresh measurements | `benchmark_asr.py`, `benchmark` |

`WorkerRun.tla` bounds one worker and recognizes a cold result, a warm result, and any unexpected record. The model abstracts timeouts, malformed JSON and termination to failure. `Resume.tla` bounds one candidate repetition and abstracts the fingerprint to a comparison result. The fingerprint consists of the paths enumerated in `benchmark`. Model files, runtime environment and validation of the saved JSON are outside the equivalence claim. Historical runner commands have separate execution flows.

TLC checked 22 worker states and 48 resume states. The mutant configs skip the ordered-result check or fingerprint equality. The worker replay runs real local child processes with normal, missing, reversed, and failed output. The resume replay uses the actual runner with temporary manifests and metadata to exercise the early rejection. In a disposable checkout, accept any successful pair of records in `execute`, or bypass the fingerprint equality assertion in `benchmark`. Each change fails its matching source replay. Restore and rerun before updating the reviewed refs.

Use pstack’s `/statechart` to inspect the shortest path for a failing rule or visualize the checked flow. When the source changes, update the reviewed revision after rechecking the model and source fixture.
