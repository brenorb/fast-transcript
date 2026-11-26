# Incremental transcription

`fscript MEDIA --stream` prints text as audio is decoded and transcribed.
`fscript MEDIA --stream --json` emits the versioned JSONL protocol used by agents
and GUI clients. Ordinary one-argument transcription keeps its existing
completed-file behavior.

```bash
fscript lecture.mp3 --stream
fscript 'https://example.com/video' --stream --json
cat lecture.wav | fscript - --stream
fscript lecture.mp3 notes.json --stream --json
fscript lecture.mp3 --last 5:00 --stream --json
fscript meeting.m4a --stream --json --diarize coreml
```

## Defaults and selection

Opening chunks stay at two seconds until the model recognizes text, followed by
eight-second chunks with half a second of overlap. This avoids waiting through a
long steady chunk when a video begins with silence. An explicit `--chunk` uses that duration throughout; `--overlap`
also remains configurable. Chunking cannot be disabled in streaming mode. The
loaded model is reused for every chunk in the job.

FFmpeg decodes directly into a bounded PCM pipe. The application holds one chunk
of audio plus a small read block; transcript text, segments, and chunk metadata
naturally grow with the transcript. FFmpeg uses a 32 KiB probe budget and 100 ms
analysis budget to avoid holding initial audio for its usual longer probe. Very
unusual containers may need the ordinary non-streaming path. This is incremental
chunked recognition, not a claim that every backend offers native token streaming.

For remote media, the shared yt-dlp resolver selects an audio stream and its HTTP
headers. FFmpeg reads that stream directly and transcribes before its download
finishes; it does not wait for a complete temporary media file. HTTP reads have a
15-second inactivity timeout. Remote manual captions remain preferred when
eligible and are returned immediately without loading a model. `--local` forces
local audio recognition. Stream URLs must use HTTP(S).

Start/end/last selection uses the same range contract as batch mode. Source
positions are preserved in every update and final result. `--last` on stdin must
first spool the input to determine its duration; ordinary streamed stdin does not
spool. Non-seekable input must decode preceding audio to reach a requested start.
Containers that require their tail metadata and servers that ignore seeking can
also delay initial decoding or transfer additional bytes.

Streaming does not enable diarization implicitly. Explicit `--diarize` runs after
decoding, using only the selected audio spooled incrementally to a temporary WAV.
Live updates then have no speaker labels; the completed result includes them.

## Event protocol version 1

Each stdout line is one JSON object, flushed immediately:

```json
{"version":1,"job_id":"123-456","sequence":0,"type":"started","data":{"input_source":"lecture.mp3"}}
```

| Event | Payload under `data` | Consumer behavior |
| --- | --- | --- |
| `started` | `input_source` | Begin a new job. |
| `ready` | `model`, `source_start_s`, and model readiness time for local inference; captions report their source instead | Show the selected source/model. A ready model does not imply decoded audio is already available. |
| `update` | `provisional: true`, full merged `text`, full `segments`, latest `chunk` | Replace the previous snapshot. Overlap merging can revise the last segment. All times refer to the original source. |
| `completed` | Final `result`, `metrics`, optional `output_path` | Replace provisional data with the final cleaned and optionally diarized result. This is the only successful terminal event. |
| `error` | `message` | Retain any earlier partial data, mark the job failed, and inspect the nonzero process status. |

Sequences start at zero and increase by one within a job; job identifiers are
unique to CLI invocations. Consumers must check both the terminal event and the
process exit status. Validation can fail before `started`, in which case the
error is on stderr. Cancellation or abrupt termination can end the stream without
a terminal event. EOF alone never proves successful transcription.

Provisional snapshots contain uncleaned model text. The completed result uses the
same cleanup, speaker assignment, and export renderer as ordinary transcription.
Human live output appends new raw text and cannot revise previously printed text;
use JSON snapshots when revisions or final cleaned text matter, or `--raw` when
comparing human live output with the final export.

## Destinations and errors

Completed `metrics` report worker wall time to model readiness, first nonempty
text, and finalization. They exclude the outer supervisor launch and completion
event serialization; `wall_seconds` is captured before optional file export.
The final result's `load_seconds` and `transcribe_seconds` retain their ordinary
model-load and inference-only definitions. They do not measure media download,
decoder work, compilation, or total user-observed latency.

Live output always uses stdout; diagnostics use stderr. `--quiet` suppresses status
messages without removing events or errors. With an explicit output file or
directory, the final transcript is also saved using the existing atomic writer.
JSONL remains on stdout; `completed.data.output_path` identifies the saved JSON
**document**, not a JSONL log. Failed transcription leaves an existing destination
untouched. Without a destination, streaming does not create a transcript file.

`--srt` and `--vtt` are completed-document formats and cannot be combined with
`--stream`. Use ordinary transcription to export subtitles. A downstream stdout
consumer closing its pipe ends the job successfully and stops further decoding.
An incomplete PCM sample, decoder error, or inference failure never emits
`completed`.

## Cancellation and ownership

The CLI supervises a worker process and a per-job temporary directory. On macOS
and Linux the worker owns a separate process group containing FFmpeg, model
workers, and setup helpers. Ctrl-C, SIGTERM, and SIGHUP cancel that group, reap the
worker, remove its scratch directory, and return status 130. Stopping the GUI's
CLI process therefore also stops its helper processes. No cancellation prompt or
extra flag is required. Windows descendant-process cancellation is not yet
validated; this milestone targets the existing macOS/Linux builds.

The Rust `run_from_args` embedding API remains unsupervised; hosts own their own
signal policy. The binary uses `run_cli_from_args` for supervised job ownership.

## Shared core and validation

Batch and arriving audio share `InferenceOutcome::decode_chunk` for invocation,
timing, source offsets, and merging, and `finalize_transcription` for speaker
assignment and result construction. Captions, media selection, model loading,
cleanup, and atomic export also use the existing core.

`PcmChunker` accepts arbitrarily sized arrivals without an overlap-only final
chunk. Property tests compare 3,000 reproducible arrival patterns with batch chunk
ranges and verify every emitted sample's source position. Another 1,000 byte
fragmentation cases check signed PCM reconstruction. A differential test compares
batch and incremental text, segment times, and boundaries through one engine.

The process E2E suite checks actual FFmpeg decoding of WAV/FLAC/MP3/M4A, stdin
results before EOF, real yt-dlp HTTP results before the server releases the rest
of the audio, saved-document parity, source positions, captions, final diarization,
failed inference preserving output, interrupted/blocked model workers, decoder
and descendant cleanup, and closed stdout. Required dependencies are Python,
FFmpeg/ffprobe, yt-dlp, and permission to bind loopback HTTP and inspect test
processes. Run after building:

```bash
FSCRIPT_TEST_BINARY=target/debug/fscript python3 -m unittest tests.test_streaming_cli
```

See [streaming-validation.md](streaming-validation.md) for the real-model check
and precise timing scope.
