# Partial transcription validation

Checked on the development Mac on 2026-11-26. Runtime inference ran locally using
the cached `parakeet-v3-int8` ONNX model on CPU; no GUI was involved.

## Automated coverage

`cargo test --locked` covers the parser, range invariants, existing CLI contracts,
output modes, and inference/diarization orchestration. The range module runs 10,000
deterministic randomized last-duration cases and 4,000 clock-notation/caption
intersection cases. These are property checks with reproducible seeds.
An export of the exact staged files, excluding pre-existing SDK/runtime changes,
passed the Rust and media-selection E2E suites. The generated man page check also
passed. Checks were repeated after integrating concurrent CLI guideline fixes.

After building fscript, run the process-level suite:

```bash
FSCRIPT_TEST_BINARY=target/debug/fscript python3 -m unittest tests/test_media_range_cli.py
```

It requires FFmpeg, ffprobe, Python, yt-dlp, and permission to bind a loopback HTTP
fixture. CI installs the dependencies explicitly. Eight tests cover:

- WAV start/end, start-only, end-only, final duration, a final duration longer than
  the source, and clamping an end beyond known duration.
- Piped WAV and lossless FLAC selection.
- Selection of an option-like filename protected with `--`, including the duration probe.
- Remote section arguments and avoiding a second seek into downloaded clips.
- Filtering manual captions without audio download or inference; exclusive ends
  and original source timestamps.
- Invalid/conflicting selections and unknown-duration final selection failing
  before inference/download.
- Speaker assignment using the selected audio before source timestamps are shifted.
- Actual yt-dlp section download from controlled HTTP media supporting byte ranges.
  A fixed absolute byte cutoff before the selected interval ends must fail before
  inference. The cutoff applies to every retry/range request. Halving each response
  was insufficient: a seek could still obtain the complete selected interval.

The worker fixture reports a digest of the PCM samples it actually receives.
Compare those bytes with an independent FFmpeg `atrim` filter, rather than another
call through the application's seeking code. Diarization's captured WAV must
match the ASR worker's selected WAV. These fixtures test process contracts, not
recognition accuracy or diarization model quality.

## Real model differential check

Input: `experiments/transcription_benchmarks/audio/audio_15s_16k_mono.wav`.
Transcribing its final five seconds (`--last 5 --model onnx`) processed exactly
five seconds starting at source time 10. An independent FFmpeg `atrim` crop,
transcribed through the same cached real model as a standalone file, produced
identical text:

> Os mitos das receitas que são divulgadas na internet ou nas redes sociais.
> Para isso, eu vou utilizar

The selected run's chunk starts at 10; the standalone crop starts at 0. Model load
and inference were exercised in separate processes. No cold/warm latency or memory
comparison is claimed by this correctness check.

## Limits

This does not guarantee every media host honors byte-range seeking. Servers can
transfer extra bytes, and codecs can require earlier fragments. Manual captions
retain whole boundary-cue text because reliable word timestamps are unavailable.
Native GUI controls, live capture, and recording detection belong to later roadmap
milestones and have not been validated here.
