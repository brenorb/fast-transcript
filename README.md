# fast-transcript

**`fast-transcript` is a local lecture transcription CLI built to beat the usual Apple Silicon tradeoff: either fast but flaky, or accurate but painfully slow.**

On the same Mac used for development, this project transcribed a **29m47s** Portuguese lecture at **13.38x real-time** while staying around **2.51 GB RSS** on the long run. In the same local test set, it beat **`mlx-whisper`**, **`insanely-fast-whisper`**, and **`parakeet-mlx`**.

The CLI binary is called **`fscript`**:

```bash
fscript lecture.mp3
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
- auto-converts unsupported audio to **16 kHz mono PCM16 WAV**
- uses **120s chunks** with **2s overlap** by default
- writes `<audio>.transcript.json` next to the input unless you choose a different output path

## Install

### Requirements

- `ffmpeg`
- `ffprobe`

### Install with Homebrew

```bash
brew install brenorb/fast-transcript/fast-transcript
```

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
```

This will:

1. ensure the default model exists
2. normalize the audio if needed
3. transcribe with the default chunking strategy
4. write `lecture.transcript.json`

## Usage

```bash
fscript <audio> [output.json]
```

Optional overrides:

```bash
fscript lecture.wav custom-output.json
fscript lecture.wav --chunk-seconds 180 --chunk-overlap-seconds 3
fscript lecture.wav --chunk-seconds 0
fscript lecture.wav --model-dir ./models/parakeet/custom-copy
fscript lecture.wav --model-package ./models/parakeet-v3-int8.tar.gz
fscript lecture.wav --model-url https://example.com/parakeet-v3-int8.tar.gz
```

Environment overrides:

- `FSCRIPT_MODEL_DIR`
- `FSCRIPT_MODEL_PACKAGE`
- `FSCRIPT_MODEL_URL`

## Defaults

- model dir:
  - macOS: `~/Library/Application Support/fast-transcript/models/parakeet-tdt-0.6b-v3-int8`
  - Linux: `~/.local/share/fast-transcript/models/parakeet-tdt-0.6b-v3-int8`
- model package cache:
  - macOS: `~/Library/Caches/fast-transcript/parakeet-v3-int8.tar.gz`
  - Linux: `~/.cache/fast-transcript/parakeet-v3-int8.tar.gz`
- model URL: `https://blob.handy.computer/parakeet-v3-int8.tar.gz`
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
- whether `ffmpeg` normalization was used
- load time
- transcribe time
- chunk configuration
- per-chunk timing

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

## License

MIT
