# Streaming validation

Measured on 2026-11-26 on this development Mac: Apple M5 Max, arm64. Speech
inference ran locally on CPU using the cached `parakeet-v3-int8` ONNX model.
No GUI or remote service performed inference.
An export of the exact staged source, excluding pre-existing SDK/runtime work,
passed 176 library tests, 17 CLI contract tests, nine CLI I/O tests, and all 20
streaming/partial-transcription E2E tests. The format/man-page checks and package
dry run passed. The additional SIGHUP/current-PID cancellation check also passed.

## Real model differential check

Input: `experiments/transcription_benchmarks/audio/audio_15s_16k_mono.wav` (15 seconds).
Both jobs used `--model onnx --no-diarization --chunk 5 --overlap 1 --raw --json`.
The incremental run added `--stream`; the batch run added `--stdout`.
They ran in separate fresh CLI processes. Streamed PCM normalization produced
identical final text, segments, timestamps, audio duration, and chunk count to the
batch path. Each streaming job reused one loaded model across all four chunks.

| Measurement | Observed seconds | Scope |
| --- | ---: | --- |
| First nonempty text observed by the calling Python process | 1.079 | Wall time from launching the CLI, including supervisor/worker startup, cached-model loading, media decoding, and first chunk inference. No compilation or download. |
| Worker to model-ready | 0.686 | Worker startup through decoder launch and model loading; excludes the outer supervisor launch. |
| Worker to first nonempty text | 1.066 | Worker startup through first recognition result, before event serialization/flush. |
| Worker through final result | 1.447 | Full streamed decoding and four inference calls; no diarization. |
| First chunk inference | 0.183 | First call on the newly loaded model; five seconds of audio. |
| Later chunk inference | 0.192 / 0.206 / 0.145 | Same loaded model, different overlapping audio windows (5 / 5 / 3 seconds). |

These are correctness-run timings, not a statistically controlled cold/warm
benchmark. Weights were cached on disk, the model was newly loaded in each process,
and filesystem caches were not cleared. Later calls preserve the loaded model but
use different audio, so their times do not establish a cold-versus-warm speedup.
No runtime memory, allocator usage, model RAM requirement, or on-disk size comparison
is claimed here. The two-second default opening is covered separately by E2E tests;
the table deliberately uses explicit matching chunk settings for differential
comparison.

## Process-level coverage

`tests/test_streaming_cli.py` exercises the actual CLI, supervisor, FFmpeg, and
output renderer. A deterministic model worker reports a digest of the PCM samples
it actually receives. It checks twelve workflows:

- Matched batch/incremental chunk results, one worker startup per job, and an atomic
  saved JSON document equal to the completed event.
- Two-second default opening and append-only human live text.
- Short opening windows continuing through empty recognition results until text appears.
- WAV/FLAC/MP3/M4A decoding parity and empty-input failure.
- Selected intervals preserving source timestamps in provisional and final data.
- Caption filtering without a model and `--last` for piped media.
- Explicit final diarization using only selected audio, with provisional unlabeled
  updates followed by the final labeled result.
- A stdin update while the input pipe is still open and audio remains unsent.
- Actual yt-dlp resolution and HTTP decoding while the test server deliberately
  withholds the remaining audio until the first update arrives.
- Failed inference producing an error event, no successful terminal event, and no
  replacement of an existing destination.
- SIGINT, SIGTERM, and SIGHUP stopping the decoder, model worker, and descendants, including
  blocked inference, with exit 130 and removal of job scratch files.
- Closed stdout ending successfully and cleaning up test job processes.

The HTTP test runs entirely on loopback and does not depend on a public site's
authentication or current extractor policy. It proves incremental transport with
the real downloader and decoder; it does not prove every media host/container is
seekable or streamable. The worker fixture proves process/audio contracts, while
the real ONNX comparison above separately validates local inference.

## Property and core differential checks

The shared chunker checks 3,000 reproducible randomized combinations of audio
length, chunk length, overlap, and arrival boundaries against batch chunk ranges,
including every emitted sample's original position. Another 1,000 randomized PCM
byte-fragmentation cases verify signed sample reconstruction. Core differential
tests compare assembled text and timestamps, not just chunk counts. Existing
partial-transcription, CLI, model-worker, and packaging tests remain applicable.

## Remaining scope

Desktop controls, microphone/system capture, persistent dictation model reuse
between utterances, external-agent notes, and calendar/call detection are later
roadmap milestones. No GUI E2E or live meeting quality is claimed by these checks.
