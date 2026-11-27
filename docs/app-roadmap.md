# Fast Transcript app roadmap

This is the delivery ledger for the complete transcription app. Finish, document,
validate, and commit each row before starting the next. English is the product and
documentation language. Existing uncommitted SDK/runtime and benchmark work is
outside these commits.

| Order | Feature | Status | Completion criteria |
| --- | --- | --- | --- |
| 0 | Reference study and architecture | Complete | Pin Handy, Say It, and OpenOats source revisions; document reusable patterns and detection behavior. |
| 1 | Partial transcription | Complete | `--start`, `--end`, `--last`; local/stdin/remote audio/captions; original timestamps; [validation evidence](partial-transcription-validation.md). |
| 2 | Incremental transcription | Complete | `--stream` / JSONL protocol, bounded PCM, short opening windows, one loaded model per job, final export/diarization, process cancellation; [validation](streaming-validation.md). |
| 3 | External agent notes | Complete | `fscript notes` and `agents`; shared headless runner, saved preference and literal custom commands; stdin text/JSON/JSONL, atomic Markdown, cancellation; [validation and real-provider coverage](agent-notes.md#validation). |
| 4 | Desktop file/URL app | Complete | Native macOS interface, shared model/export/notes commands, live file/URL text, partial selection, copy/export, cancellation and Quit cleanup; [GUI and native-provider validation with coverage limits](desktop-validation.md). Local ad-hoc-signed bundle; release distribution remains milestone 8. |
| 5 | Dictation | Complete | Shared CLI microphone capture; GUI hold-to-talk and physical global shortcut, release to finish, exact native paste and focus-change fallback, compact recording state, warm model reuse, cancellation and lifecycle; [validation and coverage limits](dictation-validation.md). |
| 6 | Live meetings | In progress: shared meeting CLI, adaptive clock alignment, live inference, durable capture/recovery, longer-context final recognition, saved-session transcription, and cancellation implemented; route changes, meeting GUI, and native acceptance next | Microphone and system audio captured separately, local incremental transcription, visible speaker channels, durable recording and recovery, CLI start/stop/status; [storage contract and current coverage](live-meetings.md). |
| 7 | Calendar and call detection | Planned | EventKit accounts already configured on macOS, process-attributed call detection, optional automatic recording policy, deduplication and stop grace period; CLI inspection/control. |
| 8 | Stable app distribution | Planned | Reproducible app packaging, CLI bundled with matching protocol, installation docs, complete E2E suite, green CI and stable version tags. |

## Architecture and defaults

Rust owns media selection, decoding, inference, chunk merging, transcript events,
session persistence, and external agent execution. CLI commands expose every
operation without interaction. A macOS SwiftUI shell launches that CLI and consumes
its structured protocol; it does not implement transcription or notes logic.
Native capture and EventKit adapters expose noninteractive helper commands as well
as GUI actions. Keep the existing one-argument file/URL invocation working.
Use [clig.dev](https://clig.dev/) for CLI design decisions; the concrete contracts
and compatibility plan live in [cli-architecture.md](cli-architecture.md).

One primary action per context: drop a file or paste a URL to transcribe, hold to
dictate, click Record for a meeting. Keep advanced controls out of the primary
path. Use the saved model, otherwise the current built-in default. Live text can
be revised until a chunk is committed; final exports use the same renderer as CLI.
Preserve separate local/remote audio tracks for meeting attribution. Save captured
audio incrementally so transcription failure does not lose a meeting.

Automatic recording is an explicit persistent preference. The calendar supplies
meeting context; actual attributed call activity supplies the recording trigger.
Manual recordings stop only on request. Never infer a call merely from an open
meeting app or a calendar time slot.

## Validation and stable checkpoints

Each feature needs process-level CLI E2E tests, randomized invariant checks where
appropriate, and differential checks against independent behavior (for example,
FFmpeg crops or batch versus incremental results). Test fixtures validate process
contracts; real local inference checks validate the installed model path. Label
these separately. Use Computer Use only for GUI controls after core checks pass.

Keep documentation with its implementation commit. Mark a feature complete only
after its acceptance checks pass. Push only reviewable scoped commits, wait for
all applicable CI jobs to finish green, then create stable version tags. Do not
label partially implemented milestones stable.

Reference evidence: [reference-apps.md](reference-apps.md).
