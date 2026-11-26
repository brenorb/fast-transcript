# Desktop validation

Checked on 2026-11-26 on Apple M5 Max arm64, macOS 27.0.1. The application is the
local Fast Transcript bundle built from this repository, with its own matching
fscript helper. GUI tests used isolated model/agent settings and
output storage under /tmp/fast-transcript-desktop-e2e; the user's defaults were
not modified. The selected speech model was the cached parakeet-v3-int8 ONNX
model running locally on CPU.

Input was the checked-in 15-second audio_15s_16k_mono.wav speech sample, copied to
an isolated test directory. The cancellation fixture repeated those samples to
five minutes. A loopback HTTP server supplied the URL test, with the actual
CLI downloader/decoder path. No public media host was needed.

## Actual GUI controls

Computer Use operated the native app controls. Files produced by those actions
were then inspected through the filesystem and CLI, independently of the UI.

| Control/workflow | Observed result |
| --- | --- |
| Open file | Native chooser selected speech.wav; local text appeared and the job completed with saved JSON. |
| Transcribe | The path field started the same CLI job; no alternate recognition implementation was used. |
| Copy | Pasting into the input field reproduced the displayed transcript exactly. |
| Text Export | Native Save wrote gui-transcript.txt and the app reported its saved filename. |
| Model picker/reset | Selecting Ultra changed the visible default; Reset returned it to parakeet-v3-int8. Settings were isolated. |
| Agent picker/Create notes | Selected Claude; after the user's login refresh, generated English sections with source timestamps from the completed transcript. |
| Notes Export | gui-notes.md was byte-for-byte identical to the CLI-generated Markdown. |
| Last duration | 0:05 produced exactly five seconds of selected audio; the first segment began at source time 10.0 in the 15-second recording. |
| Start/end | 0:04 to 0:08 produced four seconds; all segment positions stayed within the original [4, 8] interval. |
| SRT menu Export | gui-range.srt retained source timing (first cue 00:00:04,070 to 00:00:07,940). |
| Default SRT filename | Save produced Transcript.srt with one suffix; its complete contents matched CLI export of the saved JSON. |
| Paste link/live text | A copied loopback audio URL started transcription and visibly updated text while the job was still active. |
| Cancel | Stopped that URL job, retained provisional text, disabled final export/notes, and left no owned CLI helpers running. |
| Missing file | Displayed the direct input-path error before any model/decoder work; raw diagnostics stay behind Details when needed. |
| Quit | Both idle Quit and Quit during active local transcription exited; process inspection found no remaining owned model/media helpers. |

The checks exposed and fixed a duplicated subtitle suffix, duplicate default error
text, and late missing-file validation. No ASR latency, model RAM, or cold/warm
benchmark is claimed by these GUI checks.

## Automated native checks

Nine tests run through scripts/test_desktop_client.py. They use the real Rust
CLI and FFmpeg with a deterministic ASR worker, so process/audio/protocol behavior
is tested independently of speech model accuracy. Native-to-CLI output matches
the corresponding batch document. Cancellation before first inference finishes
returns 130 and cannot be mistaken for completion.

A blocked producer sends initial text and waits for the native consumer before
finishing, proving pipe delivery before EOF. Five hundred randomized byte
fragmentation cases preserve UTF-8 snapshots and validate lifecycle failures.
Five hundred path cases check URL objects, URI strings, encoded ASCII URI bytes,
and plain paths containing spaces, Unicode, #, and %. A native NSItemProvider
file payload feeds a real CLI job with such a filename and matches batch output.

The first drag-data fixture incorrectly used URL.dataRepresentation, whose raw
file bytes need not be a legal ASCII URI for Foundation's URL initializer. The
fixture now supplies encoded URI bytes; the decoder rejects malformed bytes
instead of silently truncating a filename. This is a transport check, not a claim
that every third-party producer supplies the documented encoding.

## Coverage limits

Two attempts to drag selected text within the app did not trigger a drop through
the automation tool. Physical Finder-to-window dragging was not verified; the
native provider/decoder/CLI path is covered automatically as described above.
Computer Use stopped responding after the final default-name Save. The file had
been written correctly; a one-second process sample showed the app's main thread
idle in its normal AppKit event loop. Later automated tests and artifact checks
remain separate evidence from those GUI observations.

The local bundle is ad-hoc signed and depends on installed CLI prerequisites.
The exact staged source, excluding pre-existing local runtime changes, passed
209 Rust tests, 47 affected Python E2E/wrapper checks, and all nine native tests.
A release-mode bundle built from that tree ran real cached ONNX inference from
its relocated helper. Its text, segments, duration, and chunk count matched the
earlier GUI-generated document. Compilation and these correctness checks are not
timing benchmarks.

Notarization, release installers, microphone/system capture, meeting recovery,
and calendar/call detection belong to later milestones. Tests do not establish
compatibility with every remote host, file container, window size, or external
agent version.

## Single-command follow-up

All actions now use fscript; no second executable or wrapper is bundled. The
clean source passed 210 Rust tests, 50 affected Python checks, nine native tests,
and release/wheel artifact checks. New E2E coverage runs a file and a remote URL
with no flags, verifies explicit paths for reserved filenames, and checks skill
text/path output without inference or agent startup. The rebuilt app contains
only fscript; its model picker and reset were exercised again through that helper.
The bundled skill passed the skill-creator validator.
