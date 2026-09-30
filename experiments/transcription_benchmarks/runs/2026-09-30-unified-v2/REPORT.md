# ASR v2 — comparable cold and warm benchmarks

Measured 2026-09-30 on Apple M5 Max / macOS 27.0 / 128 GiB.

Within each table, every model uses the same input, shared audio partitions and normalization. Processes are benchmark workers using native SDK/library APIs, not production CLI timings. Values are medians of **three fresh-worker repetitions**. Each worker transcribes once from startup, then repeats the same input with its model resident.

- **New process → result:** process launch to receipt of the complete first transcript, including startup/imports/model loading. Excludes downloads, audio preparation and teardown; OS caches are populated.
- **Loaded model:** full second-pass transcription, including WAV reads and GPU synchronization. Excludes startup/loading and result serialization.
- **WER:** word edit distance against the reference, after lowercasing and Unicode punctuation removal. Numbers and spelling variants remain distinct. Lower is better.

[Protocol and reproduction](../../README.md) · [Machine-readable summary](summary.json) · [Raw error alignments](error-alignments.json) · [Independent validation](validation.json)

Cold and warm WER are identical for all configurations.

## English TED — 18m59s, 3,106 reference words

Source: [Sir Ken Robinson — Do schools kill creativity? (TED)](https://www.youtube.com/watch?v=iG9CE55wbtY). The video is 20m03s; the full captioned talk spans 26.603–1165.713 s including 0.5-s margins. Publisher English captions, not automatic captions, are the reference. Only non-speech annotations/speaker labels were removed.

| Model / runtime | New process → result (s) | Loaded model (s) | Loaded realtime | WER |
| --- | ---: | ---: | ---: | ---: |
| ONNX CPU | 25.139 | 25.108 | 45.4× | 12.46% |
| Redux CPU | 23.791 | 23.223 | 49.1× | 11.91% |
| Redux GPU · Metal/MPS | 10.150 | 7.981 | 142.7× | 12.11% |
| Ultra CPU | 23.748 | 22.191 | 51.3× | 9.76% |
| Ultra GPU · Metal/MPS | 7.242 | 5.244 | 217.2× | 9.72% |
| Phonon-2 CPU | 36.833 | 28.819 | 39.5× | 12.88% |
| Phonon-2 GPU · Metal/MLX | 9.045 | 3.436 | 331.5× | 12.56% |

## Portuguese lecture — 5m00s, 988 reference words

| Model / runtime | New process → result (s) | Loaded model (s) | Loaded realtime | WER |
| --- | ---: | ---: | ---: | ---: |
| ONNX CPU | 7.127 | 6.869 | 43.7× | 9.11% |
| Redux CPU | 6.851 | 5.143 | 58.3× | 7.89% |
| Redux GPU · Metal/MPS | 4.065 | 1.509 | 198.8× | 8.00% |
| Ultra CPU | 7.081 | 5.148 | 58.3× | 4.05% |
| Ultra GPU · Metal/MPS | 3.367 | 1.117 | 268.6× | 4.05% |
| Phonon-2 CPU | 15.254 | 6.717 | 44.7× | 16.19% |
| Phonon-2 GPU · Metal/MLX | 7.247 | 0.669 | 448.8× | 15.89% |

## English LibriSpeech — 50 clips / 3m52s, 789 reference words

| Model / runtime | New process → result (s) | Loaded model (s) | Loaded realtime | WER |
| --- | ---: | ---: | ---: | ---: |
| ONNX CPU | 7.532 | 6.955 | 33.4× | 1.39% |
| Redux CPU | 5.948 | 4.321 | 53.7× | 1.01% |
| Redux GPU · Metal/MPS | 4.882 | 1.937 | 119.8× | 1.01% |
| Ultra CPU | 7.509 | 5.703 | 40.7× | 0.76% |
| Ultra GPU · Metal/MPS | 4.078 | 1.240 | 187.1× | 0.76% |
| Phonon-2 CPU | 13.383 | 4.972 | 46.7× | 0.89% |
| Phonon-2 GPU · Metal/MLX | 8.278 | 0.988 | 234.9× | 0.89% |

## Portuguese short clip — 15s, 37 reference words

| Model / runtime | New process → result (s) | Loaded model (s) | Loaded realtime | WER |
| --- | ---: | ---: | ---: | ---: |
| ONNX CPU | 0.831 | 0.346 | 43.3× | 0.00% |
| Redux CPU | 2.413 | 0.203 | 73.8× | 13.51% |
| Redux GPU · Metal/MPS | 2.457 | 0.069 | 216.1× | 13.51% |
| Ultra CPU | 1.965 | 0.253 | 59.3× | 2.70% |
| Ultra GPU · Metal/MPS | 2.090 | 0.051 | 293.7× | 2.70% |
| Phonon-2 CPU | 9.680 | 0.246 | 60.9× | 13.51% |
| Phonon-2 GPU · Metal/MLX | 6.579 | 0.034 | 443.8× | 13.51% |

## Scope and validation

TED captions are edited text, not guaranteed verbatim speech. Filler words, repetitions, spelling and number formatting can contribute to WER; a mismatch is not automatically a recognition error. Per-model insertion/deletion/substitution counts and the exact alignments are saved. The Portuguese lecture reference began as an Ultra draft reviewed by the user. LibriSpeech covers one speaker. No general language or model ranking follows from these few recordings.

Shared partitioning also differs from historical runs: boundaries are chosen from audio energy between 25–30 s, the last remainder is at most 31 s, and every PCM sample appears exactly once. Models receive the same parts. Therefore compare rows **within this report**, not against older runs with different timing/chunking protocols.

Independent jiwer validation checked **84 process repetitions, 168 corpus passes and 2226 utterance transcripts**, plus input hashes, sample-exact reconstruction of all 105 audio parts, coverage, timings, medians and ranges.

Exact transcripts are stable across cold/warm phases and all repetitions in 28/28 configurations. See raw results for any variation. Process resource logs cover both passes and are not a third competing latency metric.
