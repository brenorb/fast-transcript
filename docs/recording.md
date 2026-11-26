# Microphone dictation

The shared capture/backend milestone is implemented. The desktop hold button and
global shortcut are the next part of roadmap milestone 5; that milestone is not
yet complete.

## Defaults and unattended use

```sh
fscript record
fscript record --duration 10
fscript record --duration 10 --json --output-dir ./dictations
fscript record --control --json
```

Bare record uses the system's default microphone, shared saved/default speech
model, text mode (no diarization), shared cleaning/chunking/rendering, and prints
the finished plain transcript to stdout. The first Ctrl-C ends capture and starts
transcription; the second cancels. During initial model loading, the first
interrupt stops before opening the microphone once loading returns. SIGTERM and
SIGHUP always cancel with status 130, stopping owned helpers and removing scratch.
Status messages go to stderr; --quiet suppresses those messages.

The model loads before the microphone opens. Capture starts only after the
recording event/status, so speech spoken during model loading is not recorded.
A duration measures microphone capture time, excluding model loading and inference.
It must be positive finite seconds. Without a terminal, use duration or control;
no-input bare record fails before loading a model. --model, --model-dir, and
--device override the same configuration as file transcription.

Native microphone transport currently supports macOS. It uses CoreAudio through
[CPAL](https://docs.rs/cpal/0.18.2/cpal/), the device's default format/rate, a bounded
128-packet queue, and a disk WAV writer outside the callback. Queue overflow,
unsupported sample formats, and device errors fail explicitly. Finalization stops
the device, drains its queue, and closes the WAV before shared FFmpeg normalization
and inference. Audio is held only in job scratch and removed after the recording.
A meeting's durable audio/recovery is a separate milestone. Finished result fields
that refer to temporary audio are diagnostic provenance, not retained recordings.

macOS asks the responsible application (the desktop app or terminal) for
microphone permission on first use. The app bundle contains a microphone usage
description. A denied device reports a useful error and System Settings location;
help/catalog operations never request microphone access.

## Warm control protocol

Control mode always reads/writes newline-delimited JSON, never prompts, and opens
no microphone while idle. A single model instance serves successive recordings.
Send one of these objects per line:

```json
{"action":"start"}
{"action":"stop"}
{"action":"cancel"}
{"action":"shutdown"}
```

Requests allow no extra fields and have a 64 KiB line limit. Start requires idle
state. Stop requires active capture and produces the authoritative final result.
Cancel discards the active clip without inference/output. Shutdown cancels any
active clip and exits successfully. EOF while idle exits successfully; EOF during
capture fails and discards audio. An invalid transition produces a recoverable
error. Invalid JSON/framing, capture failure during polling, and startup failures
produce a fatal error and nonzero exit. Requests are processed serially: during
inference, use SIGTERM to interrupt immediately; queued protocol commands do not
interrupt an inference call. Restart the controller after that cancellation.

Every event contains version (1), monotonically increasing sequence starting at
zero, type, session_id (null outside a clip), and data. This is a recording-control
protocol, distinct from the single-media --stream lifecycle:

| Type | Data | Meaning |
| --- | --- | --- |
| ready | model, load_seconds | One model is loaded; idle, with no microphone capture. |
| recording | source (microphone) | A fresh unique session_id identifies active capture. |
| processing | empty object | Capture has ended; finalization/inference is running. |
| completed | result, output_path, model_reused | Finished shared transcript; ready for another start. |
| cancelled | empty object | Clip discarded; ready for another start. |
| error | message, fatal | Failed operation; fatal means the controller exits. |

With --output-dir, completion writes DIR/SESSION_ID/transcript.json atomically.
Cancelled/failed clips create no completed document. The JSON result uses the
normal transcript schema and works with fscript export and fscript notes. Events
are flushed immediately. The first successful clip includes model load timing;
later clips report load_seconds 0 and model_reused true. A failed/cancelled clip
does not unload the model. Exit success is required in addition to a completed
result when the controller itself has exited.

## Validation coverage

The backend has six process E2E cases: duration versus independent batch
transcription, repeated start/stop plus cancellation/export and a single model
worker start, SIGINT completion versus SIGTERM cancellation/descendant cleanup,
controller disconnect/invalid transitions, a second interrupt during blocked
inference, and validation before model loading.
They use a deterministic WAV transport (FSCRIPT_CAPTURE_TEST_WAV), real disk WAV
finalization/FFmpeg normalization, and a fixture ASR process; they do not establish
physical microphone capture or recognition quality. The test-only environment
variable is never selected automatically and is not a user-facing input mode.
A property test checks every 16-bit PCM value (65,536 cases), monotonic conversion,
clipping, and nonfinite handling. Existing streaming cancellation checks cover the
shared supervisor's other jobs. Physical capture, GUI buttons, shortcut behavior,
and real warm inference remain acceptance checks before milestone 5 is complete.
