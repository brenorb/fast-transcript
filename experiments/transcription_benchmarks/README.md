# Canonical transcription benchmark (ASR v2)

The canonical runner is `scripts/benchmark_asr.py`. All current comparisons
use the same protocol, references, audio partitions and timing columns.
Machine: Apple M5 Max, 128 GiB unified memory, macOS 27.0.
The previous reports are preserved in [HISTORICAL.md](HISTORICAL.md); their
fresh-process, resident-only and internal decoder times must not be mixed.

## Inputs and reference quality

| Case | Audio | Reference |
| --- | ---: | --- |
| Portuguese short | 15.000 s | Original 37-word human reference |
| Portuguese lecture | 300.032 s | 988 words reviewed by the user from an Ultra draft |
| English LibriSpeech | 232.005 s, 50 files | 789 words from the dataset; one speaker |
| English TED | 1,139.110 s (18m59s) | 3,106 words from publisher English captions |
| Brazilian Portuguese TEDx | 923.816 s (15m24s) | 1,986 words from YouTube automatic Portuguese captions |

TED source: [Do schools kill creativity? — Sir Ken Robinson, TED](https://www.youtube.com/watch?v=iG9CE55wbtY).
The public video is 20m03s. The benchmark uses the complete captioned talk
from 26.603 to 1165.713 seconds, including a half-second margin at each end.
This excludes the uncaptioned opening/closing sequence. Captions were taken
from YouTube's publisher `subtitles.en` track, not `automatic_captions`.
Only `(Laughter)`, `(Applause)` and the `(Audience)` speaker label were
removed; the audience's spoken words were retained. The original wording,
spelling and numbers were preserved.

Portuguese TEDx source: [Aprendendo a Aprender — Yvonne Bezerra de Mello,
TEDxRioED](https://www.youtube.com/watch?v=K__5PBtLrgM), published by TEDx Talks.
YouTube had no publisher-authored subtitle track; the selected `pt-orig`
caption track is automatic. It is retained as a reproducible machine reference,
not presented as human ground truth. Bracketed `[Aplausos]` and `[Música]`
non-speech labels were removed. The benchmark uses the available captioned
talk from 6.190 to 930.006 seconds. Raw media remains ignored by Git; the
gzip-compressed JSON3 caption track, source metadata and reference are checked
in. Local audio files remain ignored, while partition boundaries and SHA-256
hashes are preserved in the manifest.

Publisher captions are an independent reference, but can edit repetitions,
fillers and spelling. WER here is agreement with those captions, not a claim
that every difference is an ASR mistake. The Portuguese long reference began
as an Ultra draft. These recordings do not establish a general language ranking.

## One timing protocol

Each of seven engine/device configurations runs sequentially, with three
fresh-worker repetitions per input case. Each worker loads its model once,
transcribes the complete case, emits the first result, then transcribes the
same case again with the model still resident.

- **New process → first result:** parent wall timer from process launch until
  receipt of the complete first transcript. Includes interpreter startup,
  imports, model loading, WAV reading, transcription and result serialization.
  Ends at the result, before process teardown. Downloads and preprocessing
  are excluded; the filesystem/model download cache is already populated.
- **Loaded model:** complete second-pass wall timer inside the worker,
  including WAV reading, all inference calls, concatenation and GPU
  synchronization. The first pass serves as warmup. Model loading and result
  serialization are excluded.
- Tables use medians of three measurements. WER is independently reported
  for both passes in JSON; variability and exact repetitions are retained.
  Internal Phonon decode timers are diagnostic fields only.
- Each process also records `max_rss_mb` from `/usr/bin/time -l`; reports show
  the median and min–max across repetitions. This is worker RSS across model
  loading and both inference passes, not total machine-wide memory pressure.
- The run environment snapshots installed model-cache bytes, prepared dataset
  bytes, physical RAM and free/total volume space. Model storage is persistent
  cache size, not temporary download traffic; the report keeps these scopes
  separate from peak process RAM.

All engines receive identical 16-kHz mono PCM parts. Long recordings are
split at the lowest-energy 200-ms window between 25 and 30 seconds after
the previous cut; the final remainder is at most 31 seconds. This uses
only audio, without looking at reference text. Parts cover every sample
exactly once, with no overlap or dropped samples. Text is concatenated
before scoring each original recording. This shared segmentation bounds
ONNX memory and removes the different outer chunking policies of the old
benchmarks. Engine-internal processing remains at its default settings.
There are no hotwords or diarization, and no thread tuning.
The measured processes are benchmark workers using the native SDK/library APIs,
not the existing production CLIs. ONNX uses `transcribe-rs 0.3.8`, int8 weights
and segment timestamps; the same worker protocol applies to the Python models.

WER lowercases, removes Unicode punctuation and collapses whitespace,
preserving accents and numerals. It sums word edit counts across original
recordings and divides by the total reference words. It does not average
utterance percentages or silently equate number/spelling variants.

GPU labels identify the runtime: **Metal/MPS** for Moondream via PyTorch,
**Metal/MLX** for Phonon. Both use the Apple GPU.

## Results

The completed [canonical report](runs/2026-09-30-unified-v2/REPORT.md) contains
the four original input cases and seven configurations. The new
[Portuguese TEDx report](runs/2026-09-30-pt-tedx-yvonne/REPORT.md) adds the
fifth case with the same seven configurations and three fresh-worker
repetitions. All tables now include peak process RSS and a resource inventory
for model caches, prepared data, physical RAM and free disk space. The original
84 repetitions and the new 21 repetitions have independent jiwer validation;
all audio partitions were reassembled and checked byte-for-byte.

The historical Portuguese WERs also change under the shared chunking policy:
for example, Ultra's reviewed lecture WER is 4.05% in v2, compared with 2.43%
under its previous native long-file processing. Both sets are retained; this
is a different input partitioning policy, not a replacement of the reference.
Timing and quality claims should use the v2 rows together.

## Reproduction

Use the pinned Python environments documented in the
[historical setup](HISTORICAL.md#phonon-2-on-apple-m5-max-2026-09-30).
Preparation additionally requires NumPy, ffmpeg and yt-dlp. Download the
public TED audio and publisher English subtitles, preserving source metadata:

```bash
mkdir -p experiments/transcription_benchmarks/datasets/ted-ken-robinson
uvx yt-dlp==2026.08.19 --skip-download --dump-single-json --no-playlist \
  'https://www.youtube.com/watch?v=iG9CE55wbtY' > /tmp/ted-info.json
uvx yt-dlp==2026.08.19 --load-info-json /tmp/ted-info.json -f 251 \
  --write-subs --no-write-auto-subs --sub-langs en --sub-format json3 \
  -o 'experiments/transcription_benchmarks/datasets/ted-ken-robinson/source.%(ext)s'
# Fetch the pinned 50 LibriSpeech utterances if they are not cached yet:
.venv/bin/python scripts/benchmark_english_transcription.py --prepare
.venv/bin/python scripts/prepare_asr_benchmarks.py --youtube-info /tmp/ted-info.json
cargo build --release --locked --example benchmark_asr_onnx
.venv/bin/python scripts/benchmark_asr.py --out-dir /tmp/asr-v2-run
uv run --no-project --with jiwer==4.0.0 python scripts/validate_asr_benchmark.py \
  --runs-dir /tmp/asr-v2-run
.venv/bin/python scripts/report_asr_benchmark.py --runs-dir /tmp/asr-v2-run
```

`--resume` continues a partial run only when input/script/binary hashes match.
The raw audio cache is ignored by Git; the reference manifests, source
metadata, partition boundaries and SHA-256 checksums are retained.

For the Portuguese TEDx case, get the `pt-orig` automatic captions and audio
from [TEDxRioED](https://www.youtube.com/watch?v=K__5PBtLrgM), then create the
manifest, run all seven configurations and validate them with the same suite:

```bash
mkdir -p experiments/transcription_benchmarks/datasets/tedx-yvonne
uvx yt-dlp==2026.08.19 --skip-download --dump-single-json --no-playlist \
  'https://www.youtube.com/watch?v=K__5PBtLrgM' > /tmp/tedx-yvonne-info.json
uvx yt-dlp==2026.08.19 --load-info-json /tmp/tedx-yvonne-info.json -f 251 \
  --write-auto-subs --no-write-subs --sub-langs pt-orig --sub-format json3 \
  -o 'experiments/transcription_benchmarks/datasets/tedx-yvonne/source.%(ext)s'
gzip -n -c experiments/transcription_benchmarks/datasets/tedx-yvonne/source.pt-orig.json3 \
  > experiments/transcription_benchmarks/datasets/tedx-yvonne/caption_track.json3.gz
.venv/bin/python scripts/prepare_tedx_pt_benchmark.py \
  --youtube-info /tmp/tedx-yvonne-info.json \
  --audio experiments/transcription_benchmarks/datasets/tedx-yvonne/source.webm \
  --captions experiments/transcription_benchmarks/datasets/tedx-yvonne/source.pt-orig.json3
.venv/bin/python scripts/benchmark_asr.py \
  --suite experiments/transcription_benchmarks/datasets/tedx-yvonne/suite.json \
  --cases pt-tedx-yvonne \
  --out-dir /tmp/asr-pt-tedx-run
uv run --no-project --with jiwer==4.0.0 python scripts/validate_asr_benchmark.py \
  --suite experiments/transcription_benchmarks/datasets/tedx-yvonne/suite.json \
  --runs-dir /tmp/asr-pt-tedx-run
.venv/bin/python scripts/report_asr_benchmark.py --runs-dir /tmp/asr-pt-tedx-run
```
