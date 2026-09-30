# ASR v2 — comparable cold and warm benchmarks

Measured 2026-09-30 on Apple M5 Max / macOS 27.0 / 128 GiB.

Within each table, every model uses the same input, shared audio partitions and normalization. Processes are benchmark workers using native SDK/library APIs, not production CLI timings. Values are medians of **three fresh-worker repetitions**. Each worker transcribes once from startup, then repeats the same input with its model resident.

- **New process → result:** process launch to receipt of the complete first transcript, including startup/imports/model loading. Excludes downloads, audio preparation and teardown; OS caches are populated.
- **Loaded model:** full second-pass transcription, including WAV reads and GPU synchronization. Excludes startup/loading and result serialization.
- **WER:** word edit distance against the reference, after lowercasing and Unicode punctuation removal. Numbers and spelling variants remain distinct. Lower is better.

[Protocol and reproduction](../../README.md) · [Machine-readable summary](summary.json) · [Raw error alignments](error-alignments.json) · [Independent validation](validation.json)

Cold and warm WER are identical for all configurations.

## Portuguese TEDx — Aprendendo a Aprender, 15m24s, 1,986 automatic-caption words

Source: [Aprendendo a Aprender — Yvonne Bezerra de Mello, TEDxRioED](https://www.youtube.com/watch?v=K__5PBtLrgM), published by TEDx Talks. This is Brazilian Portuguese. The benchmark covers 6.190–930.006 s of the 15m30s video. The selected YouTube `pt-orig` track is automatic; YouTube reported no publisher-authored subtitle tracks. Bracketed non-speech labels were removed. Treat WER as agreement with machine-generated captions, not as a human-ground-truth accuracy score.

| Model / runtime | New process → result (s) | Loaded model (s) | Loaded realtime | Peak RAM RSS (MiB) | WER |
| --- | ---: | ---: | ---: | ---: | ---: |
| ONNX CPU | 19.896 | 19.663 | 47.0× | 2921 [2909–2924] | 20.49% |
| Redux CPU | 17.803 | 17.691 | 52.2× | 1883 [1879–1883] | 20.75% |
| Redux GPU · Metal/MPS | 8.020 | 5.747 | 160.7× | 912 [902–914] | 20.80% |
| Ultra CPU | 20.931 | 18.819 | 49.1× | 5013 [5013–5023] | 21.65% |
| Ultra GPU · Metal/MPS | 6.121 | 4.246 | 217.6× | 922 [921–923] | 21.70% |
| Phonon-2 CPU | 33.118 | 24.114 | 38.3× | 2188 [2179–2194] | 24.37% |
| Phonon-2 GPU · Metal/MLX | 8.511 | 2.808 | 328.9× | 2946 [2936–2952] | 24.82% |

## Memory and disk footprint

Peak RAM is the median maximum resident set size across the three fresh worker processes, with the observed min–max range beside it. `/usr/bin/time -l` measures RSS for the complete two-pass worker, including model loading; this is process RAM and may not equal total system-wide unified-memory pressure.

Resource snapshot captured 2026-09-30T13:52:50.640167+00:00: the machine had 128 GiB physical RAM and 1501.6 GiB free of 1.8 TiB on the benchmark volume. These are machine-level snapshots, not per-model allocations.

Persistent model files already present in the local cache (counting symlink targets and deduplicating file inodes within each family):

| Model family | On-disk model cache |
| --- | ---: |
| onnx-parakeet-tdt-0.6b-v3-int8 | 639.6 MiB |
| parakeet-redux | 170.7 MiB |
| parakeet-ultra | 1198.3 MiB |
| phonon-2 | 325.4 MiB |

The benchmark's prepared dataset folder occupied 68.0 MiB at that snapshot. The run-results folder currently occupies 1.5 MiB, including per-repetition JSON and `/usr/bin/time` logs.

## Scope and validation

TED publisher captions can edit repetitions, fillers and spelling. The Portuguese TEDx reference here is an automatic YouTube transcript, so WER describes agreement with that machine reference rather than transcription accuracy against human ground truth. Per-model insertion/deletion/substitution counts and exact alignments are saved. No general language or model ranking follows from this recording.

Shared partitioning also differs from historical runs: boundaries are chosen from audio energy between 25–30 s, the last remainder is at most 31 s, and every PCM sample appears exactly once. Models receive the same parts. Therefore compare rows **within this report**, not against older runs with different timing/chunking protocols.

Independent jiwer validation checked **21 process repetitions, 42 corpus passes and 42 utterance transcripts**, plus input hashes, sample-exact reconstruction of all 33 audio parts, coverage, timings, medians and ranges.

Exact transcripts are stable across cold/warm phases and all repetitions in 7/7 configurations. See raw results for any variation. Process resource logs cover both passes and are not a third competing latency metric.
