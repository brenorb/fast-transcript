---
name: fast-transcript
description: Transcribe and inspect local audio/video or remote media URLs with fscript, including selected intervals, incremental results, speaker labels, exports, and notes through installed agents.
---

# Fast Transcript

Use the single `fscript` executable. Everyday transcription needs only a file or
URL: `fscript video.mp4` or `fscript "https://example.com/video"`. It saves the
transcript and prints its absolute path. Diagnostics use stderr; check the exit
status before reading the result. Read the generated file to answer the user.

For quick transcript-first analysis without a saved file, use
`fscript MEDIA --text --stdout`. Text mode keeps timestamps and skips diarization.
Use `--text=plain` for segment lines without timestamps, or `--json --stdout` for
structured text, segments, source information, and timings. Save large results
to a file rather than flooding the agent context.

When only part of the recording matters, transcribe that part directly:

```bash
fscript video.mp4 --last 5:00
fscript video.mp4 --start 10:00 --end 15:00
```

Times accept seconds or clock notation; end is exclusive. Last-duration selection
requires known media duration and conflicts with start/end. Segment timestamps
remain relative to the original recording. Do not transcribe the whole recording
and then discard most of it when a selected interval fulfills the request.

Use `--diarize coreml` when speaker separation is required; missing dependencies
then fail explicitly. Labels distinguish voices, not real names: identify a
speaker from evidence before attributing statements to a person. Text modes skip
diarization even when speaker flags are supplied.

Use `--stream --json` for incremental machine output. Each JSONL event has version,
job_id, sequence, type, and data. Update events replace the provisional snapshot;
completed.data.result is authoritative. Require a completed event and successful
exit before treating the transcript as finished. Cancel with SIGTERM or Ctrl-C;
owned decoder/model helpers stop. An interrupted stream is not a final document.

Remote URLs prefer available manual subtitles when eligible. `--local` forces
local audio recognition; it may require more download/model work. Model downloads
and SDK setup occur on first use. Inspect `fscript models --json` when choosing a
model; `fscript models set NAME` saves the same preference used by the desktop.

For microphone dictation on macOS, `fscript record --duration 10 --json` runs
without terminal interaction. `fscript record --control --json` accepts newline
JSON objects with action start, stop, cancel, or shutdown and reuses one model.
`--hotkey` enables the same warm global hold gesture as the app; ready reports
the actual selected chord and registration availability. Recording events use their own version 1 lifecycle (see `fscript record --help`).
Use stop to finish; closing the controller mid-recording discards the clip.

`fscript permissions --json` inspects automatic-paste access without prompting,
recording, or loading a model. Only an explicit `--request paste` may show macOS's
permission prompt. Do not request access automatically in unattended workflows.
Run through the same app/terminal as delivery; ad-hoc rebuilds can invalidate an
older grant. Missing paste permission does not prevent `export --copy`.

Completed JSON/JSONL can be exported without rerunning recognition:

```bash
fscript export transcript.json --format srt
fscript export transcript.json --format compact --stdout
fscript notes transcript.json
```

Notes use the saved or first installed Codex/Claude/omp agent and its own login
and configured model provider. Use `fscript agents` to inspect availability and
`fscript agents set NAME` to save a preference. Custom agents are configured with
`fscript agents add NAME -- PROGRAM ARGS...`; argv is literal, with no shell.

Use `fscript --help`, nested help such as `fscript notes --help`, or the complete
offline `fscript --manual` when details are needed. Action names are reserved:
for an actual file named notes, models, agents, export, skill, record, or meeting, pass an explicit
path such as `./notes` or use `fscript -- notes`. Help/discovery never loads a
speech model or starts a note agent.

For live meetings, `fscript meeting --duration 60 --json` saves microphone and
system audio separately and emits provisional channel-attributed text. macOS
14.2+ capture requires OS microphone/system-audio access. `fscript meeting
--control` waits for one JSONL `{"action":"start"}` request; send `stop` to finish
capture and drain recognition. `cancel`/`shutdown` interrupts recognition even
after capture stops. Do not close stdin before stop. Use `meeting status DIR` to
inspect a durable checkpoint and `meeting recover DIR` only after an interrupted
owner exits. Live snapshots are `transcript.partial.json`; only
`transcript.json` is complete and ready for final export/notes. Final text uses
longer context and can revise the preview. Plain output prints its final path on
stdout and preview/status on stderr; --json exposes the live protocol. Native
system capture, transcribing recovered recordings, route changes, and the meeting GUI are still under validation;
see docs/live-meetings.md for coverage and current limits.

For a stopped or interrupted meeting, run `fscript meeting transcribe SESSION`
to transcribe retained audio without opening capture devices. It repairs an
ownerless recording automatically; --model/--device/--model-dir follow transcribe.
The final path is printed on success; --json emits session events. Ctrl-C cancels
immediately. Failure retains any previous transcript and saved audio. The completed
document's optional meeting manifest reports capture state and track_frames: an
interrupted recording remains interrupted even after all retained audio is
transcribed. Never infer full-meeting capture from recognition completion alone.
