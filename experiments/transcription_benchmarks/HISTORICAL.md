# Historical benchmark protocols

These runs are preserved for provenance. Their timing scopes, segmentation and hardware differ. Use [the canonical report](README.md) for current comparisons; do not combine numbers across these historical tables.

# Transcription benchmarks

This directory brings the reusable audio and ONNX reference runs from
`vtsd-fluxo-transcript` into this repository and adds a direct comparison with
Moondream's `moondream/parakeet-redux`, `moondream/parakeet-ultra`, and
Fermion's `FermionResearch/Phonon-2`.

The two shared inputs are a 15-second Portuguese clip with a human reference
and a 5-minute Portuguese lecture. The original 5-minute reference is an ONNX
transcript; a separate user-reviewed reference was added on 2026-09-30.
Historical scores remain distance-from-baseline metrics. Scores against the
human review are explicitly identified below. The English comparison uses
50 LibriSpeech utterances with independent dataset references.

Run the comparison with a Python environment that has Moondream 2.4 and its
Photon dependencies installed:

```bash
python3 scripts/benchmark_transcription_engines.py \
  --redux-python /path/to/moondream-python
```

By default the runner measures `fscript` ONNX and Redux on CPU and MPS.
`--models` also accepts Ultra and Phonon-2; Phonon's Apple GPU path is MLX.
It writes one JSON file per run, `results.csv`, and median `summary.json`.

## English LibriSpeech on Apple M5 Max (2026-09-30)

The same first 50 `clean/validation` rows used by the historical English
report were selected again: **232.005 seconds, 789 normalized reference words**.
The duration matches the old summary; that summary did not retain individual
IDs or transcripts. The new [manifest](datasets/librispeech-validation-clean-50/manifest.json)
records all IDs, original dataset references, WAV/FLAC hashes and dataset
revision `71cacbfb7e2354c4226d01e70d77d5fca3d04ba1` from
[LibriSpeech on Hugging Face](https://huggingface.co/datasets/openslr/librispeech_asr).
These references are independent of the models under test.

All **seven configurations** ran sequentially on the M5 Max / 128 GiB, with
one resident model per process. A complete 50-file warmup was discarded,
followed by three measured passes over the 50 files. The table reports the
median total call time per corpus pass: WAV reading and transcription are
included, initialization and warmup are excluded. MPS calls synchronize before
stopping the timer. No batching, diarization, hotwords or thread tuning was used.
The ONNX worker uses the same `transcribe-rs 0.3.8` int8 model and parameters
as `fscript`, kept resident through a small Rust example; these are **not CLI
startup latency measurements**. Downloads occurred before measured passes.

WER is the sum of per-utterance word edit distances divided by the 789
reference words, not an average of utterance percentages. Normalization
lowercases, removes characters other than ASCII letters/digits/whitespace,
and splits whitespace; spelling and numeral variants are not equated.

| engine | device | resident call total s | realtime | word edits | WER | peak process RSS MiB |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| onnx | cpu | 6.008 | 38.62× | 11/789 | 1.39% | 2,086 |
| parakeet-redux | cpu | 3.651 | 63.54× | 8/789 | 1.01% | 1,240 |
| parakeet-redux | mps | 1.831 | 126.72× | 8/789 | 1.01% | 1,030 |
| parakeet-ultra | cpu | 5.682 | 40.83× | 6/789 | 0.76% | 4,290 |
| parakeet-ultra | mps | 1.303 | 178.00× | 6/789 | 0.76% | 1,047 |
| phonon-2 | cpu | 4.107 | 56.49× | 7/789 | 0.89% | 1,674 |
| phonon-2 | mlx | 0.978 | 237.33× | 7/789 | 0.89% | 2,937 |

**Phonon-2 MLX is fastest on this sample:** 0.978 s versus Ultra MPS at
1.303 s, or **1.33× the throughput** (25% less time). Ultra has the lowest
WER: 6 edits versus 7 for Phonon, 8 for Redux and 11 for ONNX.
On CPU, Redux is fastest at 3.651 s; Phonon takes 4.107 s.
All transcripts were stable across three repetitions, and CPU/GPU scores
match within each model. No Phonon output was truncated.

This is **one speaker (2277), two chapters, clean read English, under four
minutes**. A one-word gap does not establish a general quality ranking, and
these results say nothing about accents, noisy speech or long conversations.
Three timing repetitions do not increase the number of independent speech
samples. The narrow sample does confirm that Phonon behaves much better on
its declared English language than on the Portuguese lecture above/below.
Do not compare resident times here with the fresh-process Portuguese timings
or the historical M1 timings.

[Full results](runs/2026-09-30-m5-max-english/summary.json),
[per-utterance errors](runs/2026-09-30-m5-max-english/errors.json), and all
1,050 measured transcripts are saved in the run directory. WER was checked
independently with `jiwer 4.0.0` for every transcript and all 21 corpus passes;
[validation](runs/2026-09-30-m5-max-english/validation.json) also checks dataset
hashes, durations, coverage and output stability. Per-pass times, process wall
times, warmup times and internal Phonon decode timers remain in the raw JSON.
Process wall covers warmup plus all three passes, so it is not a single-pass
latency. RSS covers the whole worker process and is not total GPU memory.
Python initialization includes imports; Rust's load timer covers model load
only, so those fields are not directly comparable. Runtime/model versions
are the same as the Portuguese run, with snapshots and hashes saved again.

Reproduce from the repository root, using the two environments installed
from the pinned requirements described in the Phonon section:

```bash
cargo build --release --locked --example benchmark_resident_onnx
.venv/bin/python scripts/benchmark_english_transcription.py --prepare
.venv/bin/python scripts/benchmark_english_transcription.py \
  --repetitions 3 --out-dir /tmp/english-comparison
```

Audio downloads are cached locally and excluded from Git. The preparation
step checks the Dataset Viewer revision before accepting new rows.

## Phonon-2 on Apple M5 Max (2026-09-30)

[Phonon-2](https://huggingface.co/FermionResearch/Phonon-2) is the model in
the [announcement supplied by the user](https://x.com/yoitsmanan/status/2104990913886031993).
Its model card declares **English**; the Portuguese clips here test behavior
outside that declared language coverage, not the published English benchmark.
The package is `fermion-research 0.2.3`, with MLX 0.32.3. The package pins the
model archive to SHA-256
`98125795b6dda72f5c6eee9ba33d19815df65dcb18b50a357bf9f73c9935309e`.

All seven configurations were measured again on the same M5 Max / 128 GiB.
Each clip/configuration has a discarded warmup and three sequential fresh
process measurements; downloads are outside the measured runs. Default
chunking/thread settings and no hotword hints were used. Phonon ran with
`FERMION_DEVICE=cpu` and `FERMION_DEVICE=mlx`, and complete output was required.
Wall time includes startup/loading/teardown for every engine. The 5-minute
scores use the user's 988-word human review described below. The 15-second
scores retain the original human reference.

Reproduce (with Rust, `uv`, and `ffmpeg` installed):

```bash
uv venv .venv --python 3.12
uv pip install --python .venv/bin/python \
  -r experiments/transcription_benchmarks/runs/2026-09-30-m5-max/requirements-photon.txt
uv venv .venv/phonon2 --python 3.12
uv pip install --python .venv/phonon2/bin/python \
  -r experiments/transcription_benchmarks/runs/2026-09-30-m5-max/requirements-phonon.txt
cargo build --release
.venv/bin/python scripts/benchmark_transcription_engines.py \
  --photon-python .venv/bin/python --fermion .venv/phonon2/bin/fermion \
  --fscript target/release/fscript \
  --models parakeet-redux parakeet-ultra phonon-2 \
  --five-minute-reference experiments/transcription_benchmarks/references/5min-human.json \
  --repetitions 3 --warmup --out-dir /tmp/phonon-comparison
```

Per-run transcripts, package versions and medians are in
[runs/2026-09-30-m5-max](runs/2026-09-30-m5-max/).

| engine | device | 15s wall s | 15s WER | 5min wall s | 5min WER | 5min peak RSS MiB |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| onnx | cpu | 0.83 | 0.00% | 8.46 | 9.41% | 3,572 |
| parakeet-redux | cpu | 3.05 | 13.51% | 8.32 | 7.59% | 2,229 |
| parakeet-redux | mps | 3.14 | 13.51% | 4.60 | 7.29% | 753 |
| parakeet-ultra | cpu | 2.73 | 2.70% | 8.61 | 2.43% | 5,373 |
| parakeet-ultra | mps | 2.55 | 2.70% | 4.21 | 2.43% | 773 |
| phonon-2 | cpu | 10.00 | 13.51% | 19.54 | 15.38% | 2,219 |
| phonon-2 | mlx | 6.97 | 13.51% | 7.65 | 15.28% | 2,935 |

Times and memory are medians of three processes. WER is identical across the three repetitions for each configuration. RSS is process resident memory, not total GPU/unified-memory consumption. Timing ranges are in `summary.json`.

On this Portuguese lecture, Phonon-2 MLX takes **7.65 s**, versus **4.21 s** for Ultra MPS and **8.46 s** for ONNX. Its normalized WER is **15.28% (151/988 word edits)**, versus **2.43% (24/988)** for Ultra. Phonon CPU scores **15.38% (152/988)** and takes **19.54 s**.

Phonon reports **0.043 s** of decode for the short file and **0.691 s** for the long file on MLX (medians). These are its internal decode timers, not end-to-end latency: fresh CLI wall times are **6.97 s** and **7.65 s**. The JSON stores `decode_seconds` separately from the other engines' `transcribe_seconds`; their timing scopes should not be equated. The publisher's headline throughput is not a claim about fresh-process CLI latency.

Phonon is fast in its decode path but is not an improvement over Ultra for this Portuguese workload. No broad claim about its English quality follows from these samples. The long-clip reference began as an Ultra draft and was edited/confirmed by the user, which can favor that model; the independent English sample above adds evidence, but a broader multilingual evaluation remains necessary for a general ranking.

## Human review of the 5-minute clip (2026-09-30)

The user listened to the full audio and confirmed all ten review sections,
correcting the Ultra draft. The exact submitted text and section boundaries
are in [references/5min-human.json](references/5min-human.json), separate from
the untouched historical [ONNX reference](references/5min-onnx.json).
This is a human-reviewed model draft, **not a blind independent transcription**.
The review retained the user's wording; no additional automatic corrections
were applied. The normalized reference has **988 words**.

The 39 saved long-clip transcripts (18 historical and 21 from the new run) were rescored without rerunning inference
or replacing historical metrics. For the September 22 M5 Max runs, all three
repetitions have the same score:

| engine | device | word edits / 988 | normalized WER vs human review |
| --- | --- | ---: | ---: |
| ONNX | CPU | 93 | 9.41% |
| Parakeet Redux | CPU | 75 | 7.59% |
| Parakeet Redux | MPS | 72 | 7.29% |
| Parakeet Ultra | CPU | 24 | 2.43% |
| Parakeet Ultra | MPS | 24 | 2.43% |

The historical M1 ONNX transcript scores 94/988 (9.51%); its Redux CPU/MPS
transcripts score 75/988 and 72/988. Full per-run scores, raw WER and source
checksums are in [the rescore report](evaluations/2026-09-30-human-5min/scores.json).
No transcript is retained for Bark in its aggregate summary, so its WER cannot
be recomputed from that summary.

```bash
python3 scripts/rescore_transcription_benchmarks.py \
  --reference experiments/transcription_benchmarks/references/5min-human.json \
  --runs-dir experiments/transcription_benchmarks/runs \
  --out-dir /tmp/human-reference-scores
```

WER lowercases and removes Unicode punctuation, keeping accents and numerals;
it does not equate `cinco` with `5`. Ultra has the lowest error on this reviewed
clip. Because the review started from Ultra's output and covers only one
lecture, this is not enough to establish a general accuracy ranking.

## Parakeet Ultra comparison on Apple M5 Max (2026-09-22)

The Ultra comparison reruns ONNX, Redux and Ultra on the same Apple M5 Max
with 128 GiB unified memory and macOS 26.5.1. Do not compare these timings
directly with the historical M1 results below.

Reproduce from the repository root (requires `uv`, Rust and `ffmpeg`):

```bash
uv venv .venv --python 3.12
uv pip install --python .venv/bin/python \
  -r experiments/transcription_benchmarks/runs/2026-09-22-m5-max/requirements.txt
cargo build --release
.venv/bin/python scripts/benchmark_transcription_engines.py \
  --photon-python .venv/bin/python \
  --fscript target/release/fscript \
  --models parakeet-redux parakeet-ultra \
  --repetitions 3 --warmup \
  --out-dir /tmp/parakeet-comparison
```

Each configuration runs sequentially in three fresh processes after a
discarded warmup. Downloads are cached before measurement. Wall time includes
Python imports, model resolution/loading, the first transcription and process
teardown; it is not persistent-model warm inference. All engines use their
default thread counts and chunking, without diarization. Photon requests
segment timestamps. Medians are reported; individual runs and timing ranges
are retained in [the run directory](runs/2026-09-22-m5-max/).

Runtime: `fscript 1.1.3`, `moondream 2.4.1`, `kestrel 0.8.1`, `torch 2.14.0`.
Photon pins Ultra to `510e6f5a1c4619f39c72b083c091476935734e65` and Redux to
`af60db939ebab3ca8b95b5983174e669599a2352`; these are recorded in every Photon
result. Ultra successfully runs on both CPU and MPS here, although its
[published benchmarks](https://huggingface.co/moondream/parakeet-ultra)
use NVIDIA GPUs.

Normalized WER below lowercases text and removes Unicode punctuation while
preserving accents and numerals. The JSON also retains the original scorer's
case- and punctuation-sensitive WER/CER. Peak RSS is process resident memory
from macOS `time -l`, in MiB; it does **not** measure all Metal allocations or
total unified-memory use.

### 15-second Portuguese clip (37 reference words)

| engine | device | wall s | wall range s | realtime | inference s¹ | peak RSS MiB² | normalized WER³ |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| onnx | cpu | 0.82 | 0.82–0.83 | 18.29× | 0.325 | 1,931 | 0.00% |
| parakeet-redux | cpu | 3.05 | 2.83–3.19 | 4.92× | 0.213 | 1,069 | 13.51% |
| parakeet-redux | mps | 3.02 | 2.75–3.22 | 4.97× | 0.148 | 755 | 13.51% |
| parakeet-ultra | cpu | 2.99 | 2.76–3.08 | 5.02× | 0.318 | 4,132 | 2.70% |
| parakeet-ultra | mps | 2.98 | 2.70–2.98 | 5.03× | 0.444 | 731 | 2.70% |

### 5-minute Portuguese lecture (distance from historical ONNX output)

| engine | device | wall s | wall range s | realtime | inference s¹ | peak RSS MiB² | normalized WER³ |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| onnx | cpu | 8.01 | 8.00–8.03 | 37.46× | 7.484 | 3,524 | 4.99% |
| parakeet-redux | cpu | 8.37 | 8.30–8.39 | 35.85× | 5.267 | 2,229 | 11.72% |
| parakeet-redux | mps | 4.87 | 4.76–5.05 | 61.61× | 1.752 | 756 | 11.82% |
| parakeet-ultra | cpu | 8.69 | 8.43–8.80 | 34.53× | 5.948 | 5,377 | 9.79% |
| parakeet-ultra | mps | 3.97 | 3.94–4.26 | 75.57× | 1.686 | 769 | 9.79% |

¹ First transcription call after loading, excluding load/imports/teardown. This is not a warmed persistent service.

² Process RSS only; MPS rows exclude some Metal memory and must not be read as total memory requirements.

³ The 5-minute reference is a previous ONNX transcript, **not human ground truth**. A lower value there means closer agreement with that baseline, not necessarily higher accuracy.

Ultra on MPS transcribes the long clip in **3.97 s (75.57× realtime)**, versus 8.01 s for ONNX: **2.02× faster** end to end and 1.23× faster than Redux on MPS. Ultra CPU takes 8.69 s. For short files, ONNX still wins on total latency: 0.82 s versus 2.98 s for Ultra MPS.

On the short human-reference clip, Ultra makes one scored substitution: `cinco` instead of `5`. Normalizing that numeral too would make Ultra and ONNX both zero-error on this clip. Redux has five word edits, including `colégio` for `colágeno` and `adulgados` for `divulgadas`. Ultra CPU and MPS produce identical text for both clips across all three repetitions.

At the time of this September 22 run, only the 37-word clip had a human reference.
The later full-clip review is reported above. Ultra MPS is a promising optional
backend for long Portuguese recordings, but both clips come from one lecture
and that September 22 run included no English or noise evaluation. Keep the default unchanged
until a broader human-labeled comparison is available.

## Result on this MacBook Pro M1

| input | engine | device | wall s | realtime | peak RSS MB | WER |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 15s | current ONNX | CPU | 1.54 | 9.74x | 1,250 | **0.000** |
| 15s | Parakeet Redux | CPU | 5.97 | 2.51x | 1,071 | 0.135 |
| 15s | Parakeet Redux | MPS | 5.40 | 2.78x | 656 | 0.135 |
| 5min | current ONNX | CPU | 24.63 | 12.18x | 2,284 | **0.082** |
| 5min | Parakeet Redux | CPU | 19.86 | 15.11x | 2,225 | 0.160 |
| 5min | Parakeet Redux | MPS | 19.23 | 15.60x | 662 | 0.161 |

Redux is faster and uses less memory on the long clip, but it loses badly on
the short ground-truth clip and is materially farther from the current ONNX
transcript on the long clip. Keep the existing ONNX model as the
`fast-transcript` standard. An ONNX conversion of Redux is not justified by
these results.

## Historical short LibriSpeech validation (M1)

The first 50 `validation.clean` utterances from LibriSpeech were run as
separate short files (232.005 seconds total). WER uses the dataset text after
lowercasing and removing punctuation. The warm column excludes model load;
the CLI wall column includes the current `fscript` process startup for every
utterance.

| engine | device | warm inference | CLI/process wall | WER |
| --- | --- | ---: | ---: | ---: |
| current ONNX | CPU | 20.61x | 4.87x | 0.0152 |
| Parakeet Redux | CPU | 25.50x | 12.82x | 0.0114 |
| Parakeet Redux | MPS | 9.95x | 8.93x | 0.0114 |

Redux wins this short-set warm-inference test by 24%, with a small WER edge.
That is not enough to replace the current standard: the ONNX model remains
more accurate on the checked-in ground-truth clip, and the MPS Redux path is
slower on this M1 for short calls.

## Underdog ASR check

Underdog's **Husky** package is an inference engine for the text-only Woof
model, so it cannot be compared as a transcription engine. The closest valid
test is Underdog's separate 4-bit MLX ASR model,
`ConwayResearch/Underdog-Bark-0.8B-1.0`.

| input | engine | wall realtime | WER |
| --- | --- | ---: | ---: |
| LibriSpeech, 50 short utterances | Bark MLX | 11.00x warm | 0.0177 |
| Portuguese, 15s | Bark MLX | 5.74x | 0.150 |
| Portuguese, 5min | Bark MLX | 7.82x | 0.185 |

Bark is slower and less accurate than the current ONNX path on both
Portuguese clips, and slower than Redux on the short English set. It does not
replace the current standard.
