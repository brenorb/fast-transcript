# fast-transcript

**`fast-transcript` is a local lecture transcription CLI built to beat the usual Apple Silicon tradeoff: either fast but flaky, or accurate but painfully slow.**

On the development machine, this project handled **30 minutes in 2!*** while staying around **2.51 GB RSS** on the long run. In the same local test set, it beat **`mlx-whisper`**, **`insanely-fast-whisper`**, and **`parakeet-mlx`**.

<sub>* Benchmark run on a MacBook Pro M1. The exact long-run measurement was **29m47s** of Portuguese lecture audio transcribed in about **2m14s** (**13.38x real-time**).</sub>

The CLI binary is called **`fscript`**:

```bash
fscript lecture.mp3
fscript lecture.mp3 notes/
fscript lecture.mp3 --text
fscript lecture.mp3 --text=plain
fscript lecture.mp3 --text=compact
fscript lecture.mp3 --raw
fscript lecture.mp3 --srt
fscript lecture.mp3 --vtt
fscript lecture.mp3 --json
fscript lecture.mp3 --diarize lseend-dihard3
fscript lecture.mp3 -D --json --raw
fscript lecture.mp3 -n 2
```

`--srt` and `--vtt` subtitle output are experimental.

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
- accepts local audio/video files in formats supported by `ffmpeg`
- accepts remote `http(s)` video/audio URLs supported by `yt-dlp`
- prefers platform-provided manual subtitles for remote URLs when available
- falls back to downloading remote audio and transcribing locally when only auto-captions exist or no captions exist
- auto-converts unsupported audio to **16 kHz mono PCM16 WAV**
- uses **120s chunks** with **2s overlap** by default
- runs local speaker diarization by default via `fluidaudiocli process --mode offline`
- writes `<input>.speakers.txt` next to the input unless you choose a different output path
- can alternatively write raw transcript text via `--text`, with timestamps on by default, `--text=plain` for one line per segment, and `--text=compact` for a single flattened line
  - text modes never run diarization; explicit diarization flags are ignored with a warning
- can alternatively write experimental subtitle files via `--srt` or `--vtt`
- can alternatively write speaker-aware text via `--speakers`, defaulting to `HH:MM:SS - SPEAKER_01: ...`
- cleans pathological repeated-word runs such as `we we we we` into `we... we` by default, with `--raw` as the opt-out
- stays quiet by default: concise progress in the terminal, transcript JSON on disk
- shows a spinner and chunk progress bar on interactive terminals

## Install

### Requirements

- `ffmpeg`
- `ffprobe`
- `yt-dlp` for remote URLs, or `uvx yt-dlp`
- `fluidaudiocli` on `PATH` if you want speaker-aware diarization
  - when it is missing, `fscript` warns and continues without diarization
  - use `-d`, `--diarize`, or `--diarize lseend-dihard3` to require diarization explicitly

### Install with Homebrew

```bash
brew tap brenorb/tap
brew install fast-transcript
```

On Apple Silicon macOS, the tap also installs `fluidaudio-cli`, so the default speaker-aware mode works out of the box.

If you prefer the fully-qualified formula name:

```bash
brew install brenorb/tap/fast-transcript
```

If you previously used the old tap name, migrate with:

```bash
brew uninstall brenorb/fast-transcript/fast-transcript
brew untap brenorb/fast-transcript
brew tap brenorb/tap
brew install fast-transcript
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

On macOS, the build now auto-detects the active Xcode or Command Line Tools Clang runtime directories so `cargo test` keeps linking even if your Rust toolchain points at a stale `libclang_rt.osx` path.

### Development

For local development, install the checked-in git hooks once per clone:

```bash
scripts/install-git-hooks.sh
```

The pre-commit hook blocks Rust commits that would fail `cargo fmt --check` in CI.

## Quick start

```bash
fscript lecture.mp3
fscript https://www.youtube.com/watch?v=QSdh8Gj0mEg
```

This will:

1. ensure the default model exists
2. normalize the audio if needed
3. transcribe with the default chunking strategy
4. diarize with the default `coreml` backend
5. write `lecture.speakers.txt`
6. print the final absolute transcript path to `stdout`

For remote URLs, the default speaker-aware flow is:

1. inspect the URL with `yt-dlp`
2. download the remote audio
3. run the normal local transcription + diarization pipeline

If you switch to `-D` or `--no-diarization`, `fscript` can still use platform-provided manual subtitles directly when they are available unless you also force `--local`.

## Usage

```bash
fscript <media-or-url> [output-path]
fscript <media-or-url> -o output-path
fscript <media-or-url> --stdout
fscript <media-or-url> -
fscript <media-or-url> --speakers
fscript <media-or-url> --speakers=plain
fscript <media-or-url> --speakers=timestamps
fscript <media-or-url> --text
fscript <media-or-url> --text=plain
fscript <media-or-url> --text=compact
fscript <media-or-url> --raw
fscript <media-or-url> --json
fscript <media-or-url> --srt
fscript <media-or-url> --vtt
fscript <media-or-url> --diarize lseend-dihard3
fscript <media-or-url> -D --json --raw
fscript <media-or-url> -n 2
fscript --version
```

`<media-or-url>` can be a local audio file, a local video file, or a supported remote media URL.

When `fscript` writes the transcript to a file, it keeps progress and human-readable status on `stderr` and prints only the final absolute transcript path on `stdout`.
That makes it easy to compose in shell scripts:

```bash
out=$(fscript lecture.mp3)
open "$out"
```

If the explicit `output-path` already exists as a directory, `fscript` writes the default filename for the chosen mode inside that directory.

Optional overrides:

```bash
fscript lecture.wav custom-output.txt
fscript lecture.wav exports/
fscript lecture.wav -o custom-output.txt
fscript lecture.wav --stdout
fscript lecture.wav --speakers
fscript lecture.wav --speakers=plain
fscript lecture.wav --speakers=timestamps
fscript lecture.wav --text
fscript lecture.wav --text=plain
fscript lecture.wav --text=compact
fscript lecture.wav --raw
fscript lecture.wav --json
fscript lecture.wav --srt
fscript lecture.wav --vtt
fscript lecture.wav --diarize lseend-dihard3
fscript lecture.wav -D --json --raw
fscript lecture.wav -n 2
fscript lecture.wav --chunk 180 --overlap 3
fscript lecture.wav --chunk 0
fscript lecture.wav --model-dir ./models/parakeet/custom-copy
fscript lecture.wav --model-package ./models/parakeet-v3-int8.tar.gz
fscript lecture.wav --model-url https://example.com/parakeet-v3-int8.tar.gz
fscript https://www.youtube.com/watch?v=QSdh8Gj0mEg
fscript https://www.youtube.com/watch?v=QSdh8Gj0mEg --local
```

Raw text output modes:

- `--text`: transcript text with segment timestamps, one line per segment with `HH:MM:SS - ...`
- `--text=plain`: transcript text without timestamps or speaker labels, keeping one line per segment
- `--text=compact`: transcript text without timestamps or speaker labels, flattened to a single line
- text modes never run diarization; if you pass `-d`, `--diarize`, `--backend`, `--num-speakers`, or `--threshold`, `fscript` warns and continues without diarization
- when `--text` is active and you do not pass an explicit output path, the default file becomes `<input>.transcript.txt`

Cleaning mode:

- cleaning is on by default and affects only the output being written for that invocation
- `--raw`: disables output cleaning for that invocation
- it applies to JSON, speakers, text, SRT, and VTT outputs
- it is intentionally conservative and leaves ordinary repetition alone

Subtitle output modes:

- `--srt`: experimental SubRip subtitle file
- `--vtt`: experimental WebVTT subtitle file
- subtitle output is still experimental and may change
- if diarization is active, subtitle cues include normalized speaker labels such as `SPEAKER_01: ...`
- when `--srt` is active and you do not pass an explicit output path, the default file becomes `<input>.srt`
- when `--vtt` is active and you do not pass an explicit output path, the default file becomes `<input>.vtt`

Speaker-aware output modes:

- `--speakers`: speaker-aware output with timestamps, for example `00:12:34 - SPEAKER_01: ...`
- `--speakers=timestamps`: explicit alias for the default speaker-aware timestamped output
- `--speakers=plain`: speaker-aware output without timestamps, for example `SPEAKER_01: ...`
- if diarization is disabled or a segment has no speaker label, the line falls back to plain segment text without an `UNKNOWN:` prefix
- when no output mode is passed, `--speakers` is the default
- when `--speakers` is active and you do not pass an explicit output path, the default file becomes `<input>.speakers.txt`

Environment overrides:

- `FSCRIPT_MODEL_DIR`
- `FSCRIPT_MODEL_PACKAGE`
- `FSCRIPT_MODEL_URL`
- `FSCRIPT_DIARIZATION_BINARY`

## Optional diarization

`fscript` automatically enables speaker diarization when `fluidaudiocli` is available.
If the helper is missing and you did not request diarization explicitly, `fscript` falls back to plain transcript segments after printing a warning on `stderr`.

By default, it:

1. runs the normal Parakeet ASR flow first
2. releases the ASR model
3. runs a separate `fluidaudiocli` diarization subprocess
4. merges diarization windows into ASR segments by temporal overlap

Modes:

- `-d` / `--diarize`: enable diarization explicitly with the default `coreml` model
- `-d coreml` / `--diarize coreml`: `FluidInference/speaker-diarization-coreml` path via `fluidaudiocli process --mode offline`
- `-d lseend-dihard3` / `--diarize lseend-dihard3`: alternate `FluidInference/ls-eend-coreml` DIHARD III path via `fluidaudiocli lseend --variant dihard3`
  - defaults to `--threshold 0.3`
- `-D` / `--no-diarization`: skip diarization entirely
- `--backend=coreml|lseend-dihard3|none`: legacy alias kept for backwards compatibility

Text-mode precedence:

- `--text`, `--text=plain`, and `--text=compact` always disable diarization
- if you combine a text mode with diarization flags, `fscript` warns and keeps the text mode semantics instead of running diarization

Controls:

- `-n N` / `--num-speakers N` is forwarded only to the default `coreml` backend
- `-t N` / `--threshold N` overrides the default diarization threshold for `lseend-dihard3`
- `lseend-dihard3` does not support `--num-speakers`; use the default threshold or override it with `-t` / `--threshold`

If you explicitly request `-d`, `--diarize`, or a concrete diarization model and `fluidaudiocli` is missing, `fscript` returns a clear backend error instead of silently falling back.

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
- default diarization mode: `coreml` when `fluidaudiocli` is available
- cleaning: on
- default output path: `<input>.speakers.txt`
- output path with `--json`: `<input>.transcript.json`
- output path with `--text`: `<input>.transcript.txt`
- output path with `--srt`: `<input>.srt`
- output path with `--vtt`: `<input>.vtt`
- output path with `--speakers`: `<input>.speakers.txt`

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

Default output is speaker-aware text and includes:

- segment timestamps
- speaker labels when diarization returns them
- cleaned repeated-word runs unless you pass `--raw`

JSON output via `--json` includes:

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

Alternative output modes:

- `--speakers`: speaker-aware text with timestamps
- `--speakers=timestamps`: explicit speaker-aware text with timestamps
- `--speakers=plain`: speaker-aware text without timestamps
- `--text`: transcript text with segment timestamps
- `--text=plain`: transcript text without timestamps, one line per segment
- `--text=compact`: transcript text without timestamps, flattened to one line
- `--json`: structured JSON benchmark/transcript payload
- `--srt`: experimental subtitle file
- `--vtt`: experimental subtitle file

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
