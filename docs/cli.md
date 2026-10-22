# fscript

## Name

fscript - transcribe local audio, video, and remote media URLs.

## Synopsis

```text
fscript <media-or-url> [destination] [options]
fscript - [destination] [options]
fscript help [manual]
fscript --list-models
fscript --set-default-model NAME
fscript --get-default-model
fscript --reset-default-model
```

## Description

With a local file or URL, fscript writes a transcript file and prints its absolute
path to stdout. Progress, warnings, and completion notices go to stderr. Animated progress
requires an interactive stderr and is disabled with `TERM=dumb`; redirected
streams use plain lines. A failed operation stops its progress worker.
The default is speaker-aware text with timestamps, repetition cleanup, 120-second
chunks, and two seconds of overlap. The built-in model is parakeet-v3-int8.
A saved model preference or explicit override can select another model.

Speaker diarization uses FluidAudio when available. If the helper is unavailable,
the default mode warns and continues with plain transcription. Explicit
`--diarize` requests require the helper and fail when it is unavailable.
Text modes always disable diarization; conflicting diarization flags produce a warning.

Local media decoding requires ffmpeg and ffprobe. Remote URLs use yt-dlp, with
uvx yt-dlp as a fallback. Remote input prefers manual subtitles when diarization
is disabled; `--local` forces audio download and local transcription.
The first model download and SDK setup require network access and writable storage.

## Examples

```bash
fscript lecture.mp3
fscript lecture.mp3 notes/
fscript lecture.mp3 --text=plain --stdout
cat lecture.wav | fscript - --text=plain
cat lecture.mp3 | fscript - --no-diarization --json | jq .text
fscript meeting.m4a --diarize coreml --num-speakers 2
fscript lecture.mp3 --model ultra --device cpu
fscript lecture.mp3 --text=plain --quiet --output transcript.txt
fscript --list-models
```

Use `-` as input for piped media. Its default destination is stdout. The command
spools media to a temporary file so decoding and diarization can seek, then deletes
that file on completion or an ordinary error. Empty stdin or a terminal is rejected
with a pipeline example. An explicit output file overrides the stdout default;
a directory destination uses `stdin` as the filename stem.

Use `--` before paths that begin with a hyphen. Local filenames are passed as
absolute paths to ffprobe, ffmpeg, and the diarization helper so those programs
do not reinterpret them as options. To read a file named help, use
`fscript ./help`. Bare `fscript help` displays help.

## Options

- `--speakers[=plain|timestamps]`: speaker-aware text; timestamps are the default.
- `--text[=plain|compact|timestamps]`: text without diarization; timestamps are the default. Plain writes one segment per line; compact flattens the transcript.
- `--json`: structured transcript, segments, model metadata, and timings.
- `--srt`, `--vtt`: experimental subtitle formats.
- `-o, --output PATH`: output file or directory. A trailing slash denotes a directory even if it does not exist. `--output -` writes transcript contents to stdout.
- `--stdout`: write transcript contents to stdout. A positional destination of `-` is equivalent.
- `--raw`: disable repeated-word cleanup.
- `-c, --clean`: enable repeated-word cleanup, the default.
- `--diarize[=coreml|lseend-dihard3]`: require speaker diarization; defaults to coreml when no value is given. A space before the backend value also works. The legacy `-d` alias remains supported; it does not mean debug.
- `-D, --no-diarization`: disable diarization.
- `-n, --num-speakers N`: expected positive speaker count for coreml.
- `-t, --threshold N`: finite ls-eend-dihard3 threshold; defaults to 0.3.
- `--backend coreml|lseend-dihard3|none`: legacy diarization selection.
- `-l, --local`: download remote audio and transcribe locally. `--prefer-local-for-remote` is an alias.
- `--chunk N`: chunk duration in seconds; 0 disables chunking. `--chunk-seconds` is an alias.
- `--overlap N`: chunk overlap in seconds; must be smaller than the chunk duration. `--chunk-overlap-seconds` is an alias.
- `--model NAME|URL`: predefined model or custom http(s) ONNX bundle URL.
- `--device auto|cpu|mps|mlx|cuda`: choose a device supported by the selected model. The default is auto.
- `--model-dir PATH`: existing extracted ONNX model or SDK model directory.
- `--model-package PATH`: override the cached ONNX tarball path.
- `--model-url URL`: explicit http(s) ONNX bundle URL.
- `--list-models`: available models, aliases, benchmark advice, and device notes.
- `--set-default-model NAME`: save a predefined model or alias without downloading it.
- `--get-default-model`: print the saved model, or the built-in default, to stdout.
- `--reset-default-model`: clear the saved preference.
- `-q, --quiet`: suppress progress, completion notices, and successful model-worker diagnostics. Keep transcript data, result paths, model queries, warnings, and errors. SDK installers can still report dependency diagnostics during first setup.
- `-h, --help`: full help, common examples, and defaults. Bare invocation displays a short introduction.
- `-V, --version`: installed version.
- `--manual`: this offline manual. `fscript help manual` is equivalent.
- `--man-page`: export this manual in roff format for man(1).

Output formats are mutually exclusive. Explicit output paths and stdout are
mutually exclusive. Specify one model selection. Settings commands run on their
own, with an optional `--quiet` flag. Their confirmations go to stderr.
Optional output-mode values also accept a space, for example `--text plain`.
When an output path follows an optional-value flag, use `--output PATH` to make
the destination explicit.

## Configuration

Model selection precedence is explicit `--model` or `--model-url`, then
`FSCRIPT_MODEL_URL`, then the saved preference, then the built-in default.
The saved model is per user and applies in future shells and scripts.
Use `--get-default-model` to inspect the saved preference; it does not report an
environment override. An explicit selection bypasses loading the saved setting.

Settings live in default-model.json in the platform configuration directory:
on macOS, ~/Library/Application Support/fast-transcript; on Linux,
~/.config/fast-transcript. `FSCRIPT_CONFIG_FILE` overrides the entire filename.

## Environment

- `FSCRIPT_CONFIG_FILE`: default-model configuration filename.
- `FSCRIPT_MODEL_DIR`: default model directory.
- `FSCRIPT_MODEL_PACKAGE`: default cached ONNX bundle filename.
- `FSCRIPT_MODEL_URL`: model source override, ahead of the saved preference.
- `FSCRIPT_DIARIZATION_BINARY`: explicit FluidAudio executable path or command name. Without an override, standard installation locations and PATH are searched.
- `FSCRIPT_RUNTIME_DIR`: persistent location for managed SDK runtimes, when using an installation that provisions them separately.
- `FSCRIPT_PYTHON_BINARY`: prepared Python interpreter with the selected model's pinned SDK dependencies.
- `UV_CACHE_DIR`: uv installer cache override.

## Output

Default local filenames are INPUT.speakers.txt, INPUT.transcript.txt,
INPUT.transcript.json, INPUT.srt, or INPUT.vtt, according to the selected format.
For remote media, filenames use a sanitized title or URL stem in the current
directory. Existing output files are replaced only after the complete transcript
is staged and synced in the destination directory. A failed write leaves the
earlier transcript intact. Existing file permissions and symlink destinations are preserved, including
links whose target file must be created. Saving an existing file still requires
write access to that file. Read-only files, special files, and symlink loops are
rejected; choose a writable regular file or stdout instead. New transcript files are private to the
current user on Unix. Choose another destination to retain earlier results.

File output prints only the final absolute path to stdout. Stdout mode prints
only the selected transcript representation. JSON is formatted and can be piped
to jq when stdout is selected. Use `--json --stdout` for local files, or `- --json`
for piped media. Quiet mode does not remove requested data from stdout. A consumer that closes
its input early, such as `head`, ends stdout delivery successfully without a
panic or a broken-pipe diagnostic. Other output failures remain errors.

## Troubleshooting

Unknown options and nearby model-name mistakes suggest likely corrections.
Use `--help` to inspect syntax or `--list-models` to choose a model.

If saved settings are malformed, run `fscript --reset-default-model`, or select
`--model NAME` explicitly for that invocation.

If audio inspection fails, install ffmpeg and ffprobe and check the input path.
For remote download failures, check yt-dlp or uvx and network access.

If speaker labels are required, pass `--diarize coreml`. Set
`FSCRIPT_DIARIZATION_BINARY` to the installed helper when it cannot be discovered.
Speaker labels identify voices; they are not people's names.

If SDK startup fails, install uv or set `FSCRIPT_PYTHON_BINARY` to a prepared
interpreter. For offline work, cache dependencies and model weights beforehand,
then use the same storage paths. `--model-dir PATH` selects existing model files.
Quiet-mode worker failures retain their last diagnostic messages.

Press Ctrl-C to interrupt. Temporary media and normal output are not a resume
checkpoint. An interrupted save leaves the previous transcript intact; abrupt
termination can leave a hidden `.fscript-output-*` staging file beside it. Remove
that staging file and rerun the command.

## Exit status

Zero indicates success, including help, manual, and version queries. Nonzero
indicates an argument, configuration, dependency, input, transcription, or output
failure. Scripts should check the exit status before using the result path.
Ordinary failures report their causes concisely on stderr, including when
`RUST_BACKTRACE` is set; backtraces are reserved for unexpected Rust panics.

## See also

Documentation: https://github.com/brenorb/fast-transcript#readme

Issues: https://github.com/brenorb/fast-transcript/issues
