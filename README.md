# fast-transcript

**`fast-transcript` is a local lecture transcription CLI built to beat the usual Apple Silicon tradeoff: either fast but flaky, or accurate but painfully slow.**

On the development machine, this project handled **30 minutes in 2!*** while staying around **2.51 GB RSS** on the long run. In the same local test set, it beat **`mlx-whisper`**, **`insanely-fast-whisper`**, and **`parakeet-mlx`**.

<sub>* Benchmark run on a MacBook Pro M1. The exact long-run measurement was **29m47s** of Portuguese lecture audio transcribed in about **2m14s** (**13.38x real-time**).</sub>

The CLI binary is called **`fscript`**:

```bash
fscript lecture.mp3
fscript lecture.mp3 -d
fscript lecture.mp3 -d lseend-dihard3 -t 0.3
fscript lecture.mp3 -d --num-speakers 2
```

That is the whole point of this project. One command. Large audio. No babysitting.

## Why this exists

I wanted a tool for **transcribing long classes and lectures quickly on a laptop while still using the computer for normal work**.

The existing options I tested had clear problems for this use case:

- **`insanely-fast-whisper`** was far too slow on this Mac once it fell back to CPU
- **`mlx-whisper`** was solid, but slower than I wanted for long lecture workflows
- **`parakeet-mlx`** had excellent memory numbers, but drifted into English on longer Portuguese segments unless heavily tuned

`fast-transcript` packages the ONNX Parakeet path that held up best in practice.

## What it does

- downloads the default **Parakeet TDT 0.6B v3 int8** model automatically if it is missing
- stores the extracted model in a persistent per-user application data directory
- keeps the downloaded tarball in the user cache directory
- accepts `mp3`, `wav`, and other audio formats supported by `ffmpeg`
- accepts remote `http(s)` video/audio URLs supported by `yt-dlp`
- prefers platform-provided manual subtitles for remote URLs when available
- falls back to downloading remote audio and transcribing locally when only auto-captions exist or no captions exist
- auto-converts unsupported audio to **16 kHz mono PCM16 WAV**
- uses **120s chunks** with **2s overlap** by default
- can run optional local speaker diarization as a second pass via `fluidaudiocli process --mode offline`
- writes `<audio>.transcript.json` next to the input unless you choose a different output path
- stays quiet by default: concise progress in the terminal, transcript JSON on disk
- shows a spinner and chunk progress bar on interactive terminals

## Install

### Requirements

- `ffmpeg`
- `ffprobe`
- `yt-dlp` for remote URLs, or `uvx yt-dlp`
- `fluidaudiocli` on `PATH` if you want local diarization (`-d`)

### Install with Homebrew

```bash
brew install brenorb/fast-transcript/fast-transcript
```

On Apple Silicon macOS, Homebrew now installs `fast-transcript` from a proper bottle.
On Linux x86_64, the formula still installs from the published release binary.

### PyPI / uv

The PyPI package name for this project is **`fscript`** so the target UX is:

```bash
uvx fscript lecture.mp3
uv tool install fscript
```

The repo already includes platform wheel builds for:

- macOS arm64
- Linux x86_64

PyPI publishing is currently enabled for:

- macOS arm64

See [`docs/pypi-publishing.md`](./docs/pypi-publishing.md) for the release workflow details.

### Install a prebuilt binary directly

Download the archive for your platform from the [GitHub Releases page](https://github.com/brenorb/fast-transcript/releases), then put `fscript` on your `PATH`.

### Build from source

```bash
cargo install --git https://github.com/brenorb/fast-transcript
```

Or from a local clone:

```bash
cargo install --path .
```

## Quick start

```bash
fscript lecture.mp3
fscript https://www.youtube.com/watch?v=QSdh8Gj0mEg
```

This will:

1. ensure the default model exists
2. normalize the audio if needed
3. transcribe with the default chunking strategy
4. write `lecture.transcript.json`
5. print the final absolute transcript path to `stdout`

For remote URLs, the default flow is:

1. inspect the URL with `yt-dlp`
2. use manual subtitles directly when the platform provides them
3. otherwise download the remote audio and run the normal local transcription pipeline

## Usage

```bash
fscript <audio-or-url> [output.json]
fscript <audio-or-url> --stdout
fscript <audio-or-url> -
fscript <audio-or-url> -d
fscript <audio-or-url> -d lseend-dihard3 -t 0.3
fscript <audio-or-url> -d --num-speakers 2
fscript --version
```

When `fscript` writes the transcript to a file, it keeps progress and human-readable status on `stderr` and prints only the final absolute transcript path on `stdout`.
That makes it easy to compose in shell scripts:

```bash
out=$(fscript lecture.mp3)
open "$out"
```

Optional overrides:

```bash
fscript lecture.wav custom-output.json
fscript lecture.wav --stdout
fscript lecture.wav -d
fscript lecture.wav -d coreml --num-speakers 2
fscript lecture.wav -d lseend-dihard3 -t 0.3
fscript lecture.wav -d --num-speakers 2
fscript lecture.wav --chunk-seconds 180 --chunk-overlap-seconds 3
fscript lecture.wav --chunk-seconds 0
fscript lecture.wav --model-dir ./models/parakeet/custom-copy
fscript lecture.wav --model-package ./models/parakeet-v3-int8.tar.gz
fscript lecture.wav --model-url https://example.com/parakeet-v3-int8.tar.gz
fscript https://www.youtube.com/watch?v=QSdh8Gj0mEg
fscript https://www.youtube.com/watch?v=QSdh8Gj0mEg --prefer-local-for-remote
```

Environment overrides:

- `FSCRIPT_MODEL_DIR`
- `FSCRIPT_MODEL_PACKAGE`
- `FSCRIPT_MODEL_URL`
- `FSCRIPT_DIARIZATION_BINARY`

## Optional diarization

`fscript` keeps the current fast path as the default.

When you pass `-d` or `--diarize`, it:

1. runs the normal Parakeet ASR flow first
2. releases the ASR model
3. runs a separate `fluidaudiocli` diarization subprocess
4. merges diarization windows into ASR segments by temporal overlap

Backends:

- `-d` or `-d coreml`: default `FluidInference/speaker-diarization-coreml` path via `fluidaudiocli process --mode offline`
- `-d lseend-dihard3`: alternate `FluidInference/ls-eend-coreml` DIHARD III path via `fluidaudiocli lseend --variant dihard3`

Controls:

- `--num-speakers N` is forwarded only to the default `coreml` backend
- `-t N` / `--threshold N` sets the diarization threshold
- `lseend-dihard3` does not support `--num-speakers`; use `-t` / `--threshold` instead

If `fluidaudiocli` is missing, `fscript` now returns a clear backend error instead of silently falling back.

## Defaults

- model dir:
  - macOS: `~/Library/Application Support/fast-transcript/models/parakeet-tdt-0.6b-v3-int8`
  - Linux: `~/.local/share/fast-transcript/models/parakeet-tdt-0.6b-v3-int8`
- model package cache:
  - macOS: `~/Library/Caches/fast-transcript/parakeet-v3-int8.tar.gz`
  - Linux: `~/.cache/fast-transcript/parakeet-v3-int8.tar.gz`
- model URL: `https://huggingface.co/brenorb/parakeet-tdt-0.6b-v3-int8-onnx-bundle/resolve/main/parakeet-v3-int8.tar.gz?download=1`
- chunk seconds: `120`
- chunk overlap seconds: `2`
- output path: `<audio>.transcript.json`

## Benchmarks

These are **local development benchmarks**, not universal claims. They were run on the same Apple Silicon Mac used during development, using a Portuguese lecture clip and the same broader workflow comparison.

### 2-minute lecture clip

| Engine | Setup | Speed | Peak RSS | Notes |
| --- | --- | ---: | ---: | --- |
| **fast-transcript** | Parakeet ONNX | **13.06x** real-time | **2.25 GB** | Best balance of speed and reliability |
| `mlx-whisper` | `whisper-large-v3-turbo` | `5.25x` | `1.70 GB` | Good quality, slower |
| `parakeet-mlx` | tuned for quality | `4.92x` | `1.29 GB` | Needed substantial tuning |
| `parakeet-mlx` | raw greedy | `10.16x` | `0.57 GB` | Faster on short audio, drifted into English on longer PT-BR |
| `insanely-fast-whisper` | `whisper-large-v3` CPU | `0.30x` | `6.18 GB` | Accurate, but too slow here |
| `insanely-fast-whisper` | MPS + fallback | `0.31x` | `3.04 GB` | Small gain, same general problem |

### Long lecture run

| Engine | Audio | Speed | Peak RSS | Notes |
| --- | --- | ---: | ---: | --- |
| **fast-transcript** | `29m47s` lecture | **13.38x** real-time | **2.51 GB** | Stable long run with default chunking |

### Practical reading

- `fast-transcript` was not the absolute fastest thing we saw in every synthetic case
- it **was** the best result once long Portuguese lecture audio, transcript quality, and unattended runs all mattered at the same time
- that is the target workload for this repo

## Output format

The output is JSON and includes:

- merged transcript text
- model path
- original input path
- prepared WAV path
- whether a remote URL used manual subtitles or the local model
- whether `ffmpeg` normalization was used
- load time
- transcribe time
- chunk configuration
- per-chunk timing
- transcript `segments`
- optional `speaker_diarization` metadata

When diarization is enabled, each transcript segment may include:

- `speaker`

## Motivation

This project is optimized for **large lectures and classes**, including files in the **30-minute to 2-hour** range, where:

- startup friction matters
- background CPU usage matters
- memory spikes matter
- brittle hand-tuned command lines become a tax

The design goal is not “highest benchmark on a cherry-picked GPU server”.
The goal is “transcribe big local lecture audio fast enough that you actually keep using it”.

## Inspiration

This project was heavily informed by:

- [Handy](https://github.com/cjpais/Handy)
- [GLaDOS](https://github.com/dnhkng/GLaDOS)
- [transcribe-rs](https://github.com/cjpais/transcribe-rs)

In particular, the ONNX Parakeet path here was shaped by the packaging and implementation ideas used in Handy and GLaDOS.

## Default model bundle

The default auto-download bundle is published in our own Hugging Face model repository:

- [brenorb/parakeet-tdt-0.6b-v3-int8-onnx-bundle](https://huggingface.co/brenorb/parakeet-tdt-0.6b-v3-int8-onnx-bundle)

This keeps the default install path tied to the exact validated tarball instead of an app-specific blob host.

## License

MIT
