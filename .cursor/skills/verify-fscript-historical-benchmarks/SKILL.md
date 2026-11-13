---
name: verify-fscript-historical-benchmarks
description: Check warmup, resident and fresh-process repetitions, and output eligibility when changing historical fscript benchmarks.
---

# Verify historical benchmark repetition and output

The historical comparison optionally warms filesystem and model caches with a discarded fresh process. The resident English comparison loads one model per worker and discards its first corpus pass. The historical measurement rejects Phonon output for the wrong backend or marked truncated before writing a result.

Use Java 11 or later with `TLA2TOOLS` pointing to [TLC 1.7.4](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4).

```sh
skill=.cursor/skills/verify-fscript-historical-benchmarks
sh scripts/check-cli-state-machine.sh "$skill/Warmup.tla"
sh scripts/check-cli-state-machine.sh "$skill/Measurement.tla"
python3 "$skill/references/check-historical-worker.py"
python3 "$skill/references/check-historical-measure.py"
cargo check --locked --example benchmark_resident_onnx
```

## Coverage

Reviewed main `7562797c4462045c2a3edc2ceaf9595ed6a5d626`. Both historical scripts and the resident Rust example were added after `v1.1.3` at `751f294296a0034360231d6acfe9cf3c700cb0a8`.

| Transition | Source |
| --- | --- |
| Validate modes, prepare pinned LibriSpeech rows, reuse or convert local audio, and write manifest | `scripts/benchmark_english_transcription.py`, `main`, `prepare` |
| Load resident model, discard first pass, accumulate requested repetitions and emit | `benchmark_english_transcription.py`, `worker`; `examples/benchmark_resident_onnx.rs`, `main` |
| Launch worker, validate coverage and score resident passes | `benchmark_english_transcription.py`, `benchmark` |
| Optionally warm caches with a fresh process and run requested repetitions | `scripts/benchmark_transcription_engines.py`, `main`, `run` |
| Validate worker output, calculate metrics and write raw records and summaries | `benchmark_transcription_engines.py`, `measure`, `main` |

The warmup model bounds one configuration and one to three measured passes. It abstracts each corpus pass to a completion or failure and distinguishes resident from fresh processes by their warmup policy. The measurement model bounds one result and abstracts backend, truncation, and process status. They omit exact timing, real models, download and conversion failures, and data-dependent score arithmetic. The resident replay uses the real Python worker with a local fake model. The Rust example is compile-checked and its repetition loop is inspected. Full historical inference requires the original assets and environments.

TLC checked the warmup in 78 states. The mutant counts the first pass. In a disposable checkout, replace the repetition condition before appending results in `worker` with true. The source fixture detects the extra rows. The other mutant bypasses Phonon output eligibility. Bypass the corresponding guard in `measure` and the source fixture rejects the written result. Rerun both checks when changing worker or reporting order and update the reviewed refs.

Use pstack’s `/statechart` to inspect the shortest path for a failing rule or visualize the checked flow. When the source changes, update the reviewed revision after rechecking the model and source fixture.
