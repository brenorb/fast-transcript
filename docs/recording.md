# Microphone dictation

The shared backend and native client are implemented. Desktop hold-to-talk and
the shared global-shortcut/clipboard integration are under end-to-end validation;
roadmap milestone 5 is not yet complete.

The native process client now supports an explicit two-way control pipe. Writes
are serialized off the UI thread, bounded to 64 KiB, and protected against SIGPIPE;
ordinary media/notes jobs still receive closed stdin. A recording decoder checks
protocol version, sequence, unique session identity, state transitions, complete
final results, and exit status. Media and recording decoders share byte framing.
Fifteen native tests include two real-CLI clips versus independent batch output
with exactly one model worker start, full-duplex EOF, broken-pipe recovery, and
500 randomized UTF-8 fragmentations of repeated/cancelled recording sessions.
These use the deterministic capture/ASR fixture and do not prove physical capture.

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

## Shared shortcut and text delivery

Use fscript record --hotkey for a warm global hold gesture, or combine --hotkey
with --control for the desktop. Rust owns registration, press/release handling,
repeat suppression, and microphone/model lifecycle. It uses exclusive Carbon
registration to detect competing owners, following the [same API behavior
handled by global-hotkey](https://github.com/tauri-apps/global-hotkey/blob/dev/src/platform_impl/macos/mod.rs).
Auto tries Control-Shift-Space, Control-Shift-D, then Control-Shift-Period; explicit
--hotkey ctrl+shift+space, ctrl+shift+d, or ctrl+shift+period selects one chord.
Ready reports shortcut_label, shortcut_available, and shortcut_error. Control
clients can still record when global registration fails; bare shortcut mode exits
nonzero. Cancel/exit unregisters the owned key. A 10,000-pattern property test
checks repeated input suppression and release edges; queue overflow fails loudly.
Native event pumping uses a small positive timeout, including while idle.
Gestures queued during synchronous inference are discarded, avoiding a stale
press starting capture after the user has already released it.

The desktop's Copy and automatic dictation delivery call fscript export --copy
with the shared renderer. Optional --paste --target-pid PID adds an explicitly
targeted Command-V through CoreGraphics. It checks CoreGraphics event-posting access for the process performing the
operation (CGPreflightPostEventAccess), rather than assuming the parent GUI
process and helper have the same grant. It waits for modifiers to be released, and never types transcript characters as
shell input. The app additionally checks the originally active application is
still active. If focus changed, it copies; if targeted paste fails, it retries
clipboard-only delivery and reports the failure while keeping the transcript.
The app logs its own bundle path/PID and AX/event-posting preflight values for
diagnostics instead of telling users an enabled Settings switch is disabled.
Ad-hoc builds have a version-specific code identity, so an enabled entry may
refer to a different build; [Apple explains code identity and permission
continuity](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements/).
The CLI cannot acknowledge that a target accepted paste. Clipboard command
failures are surfaced; completed transcripts remain saved. The clipboard E2E
uses a literal fixture executable and compares all rendered bytes without
changing the real clipboard or injecting keys. Tests reject implicit/invalid
paste targets and incompatible file destinations before execution.

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
