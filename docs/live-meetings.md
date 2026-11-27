# Live meetings

Roadmap milestone 6 is in progress. The meeting CLI now connects shared capture,
continuous normalization, durable storage, incremental inference, final export,
and recovery. Native system-audio delivery remains unverified. Clock-drift
compensation, route-change recovery, the meeting GUI, and native acceptance are
still outstanding; this milestone has no stable tag. Dictation is independently
complete at stable/dictation. Calendar/call detection remains milestone 7.

## Everyday use and unattended control

```sh
fscript meeting
fscript meeting --duration 60 --json
fscript meeting --control --json
fscript meeting status ./meetings/SESSION --json
fscript meeting recover ./meetings/SESSION --json
fscript export ./meetings/SESSION/transcript.json --format speakers
fscript notes ./meetings/SESSION/transcript.json
```

The default output parent is ./meetings. Each start creates a new private child;
--output-dir overrides that parent. The saved speech model is shared with file
transcription and dictation, with --model/--model-dir/--device overrides. Normal
foreground capture finishes on first Ctrl-C; the second cancels owned processes.
--duration requires positive finite seconds and needs no terminal. --control
waits for one JSONL start request, accepts stop/cancel/shutdown, and always emits
JSONL. Ctrl-C cancels control mode immediately. No GUI-only recording API exists.
A literal media file named meeting uses ./meeting or -- meeting.

A stop request stamps one common host-clock cutoff. Capture stops independently
of blocked recognition, drains queued packets through each continuous converter,
and saves both tracks. The requested duration starts at session creation; startup
and initial permission time are included. A source that becomes ready only after
the cutoff, or supplies no accepted audio frames, is an interrupted capture rather
than a successful track manufactured from padding. Actual silence supplied by a
working endpoint remains valid captured audio.

Closing stdin before stop cancels and preserves recoverable audio. Closing it
after stop permits completion. Cancel/shutdown interrupts blocked model loading,
recognition, and the final backlog pass. If audio has already been saved, its
manifest remains stopped; cancellation does not relabel successful capture as an
interrupted recording. One controller process handles exactly one meeting.

## Events and transcript publication

Session JSONL uses the shared dictation envelope: version 1, sequence (starting
at zero), type, nullable session_id, and data. The session ID is assigned by started;
ready precedes a controlled start and has no assigned ID. Events are flushed.
Diagnostics go to stderr; quiet suppresses status without removing result events.

| Event | Meaning |
| --- | --- |
| ready | Service accepts start; requires_start says whether control mode is enabled |
| started | Private directory exists; supplies directory and Microphone/System channel labels |
| recording | Both capture adapters are ready, independently of model loading |
| model_ready | One shared model is loaded for both tracks; includes model metadata and load time |
| update | Provisional full text/segments, newly printable addition, and transcript.partial.json output_path |
| recognition_error | Recognition failed; recording_continues reflects whether capture is still active |
| saved | Both tracks were closed, synced, and marked stopped; includes final manifest |
| completed | All saved frames were processed and transcript.json was committed; includes the existing result schema |
| cancelled | Recognition was interrupted; directory, audio_stopped, and recoverable distinguish saved capture from interruption |
| error | Fatal capture/control/output failure; no completed event is implied |

Provisional snapshots never occupy the final filename. The inference helper stages
its result privately; the controller owns atomic final promotion and the completion
commit point. Cancellation accepted before that point prevents publication.
Cancellation arriving after it cannot produce a second terminal event. An audio
manifest marked stopped proves capture completion; it does not by itself prove
that recognition or final transcript publication succeeded. Partial snapshots may
remain alongside a complete transcript, but only transcript.json is the final
export/notes input. Channel names do not identify individual remote participants.

## Capture and recognition execution

Each source has its own capture thread and one continuous FFmpeg normalization
context: native-rate interleaved PCM16 becomes 16 kHz mono PCM16. Output readers
write directly to the shared private store in bounded blocks; neither native
callbacks nor disk writes wait for the ASR result. Both source loops checkpoint
at one-second intervals. A separate stop/deadline watcher acts even if a converter
blocks its input or shutdown; after two seconds it kills that owned converter.
A killed/failed converter cannot mark the session successfully stopped. This
bounds converter stalls, not arbitrary OS permission dialogs or stalled storage.

One inference helper belongs to the existing supervised process group. It loads
one model, reads bounded immutable audio prefixes from disk, and uses the same
PcmChunker, InferenceOutcome merging, finalizer, and export schema as file jobs.
Opening windows are 2 seconds; after recognized text they become 8 seconds, with
0.5-second overlap. Microphone and System are processed in alternating bounded
reads. Slow recognition accumulates an on-disk backlog; it does not create an
unbounded queue of audio in RAM. Recognition failures, including startup failures,
leave healthy capture running until the requested stop; the command subsequently
fails with the saved audio directory instead of claiming a complete transcript.

The current alignment compares cumulative native frames against the host clock,
keeps small callback jitter contiguous, fills positive discrepancies exceeding
100 ms, and fails on a backwards discrepancy exceeding 100 ms. This is not an
adaptive device-clock resampler: gradual independent-clock drift is not yet
compensated, and a large positive discrepancy can insert silence. Long meetings
with independent devices therefore still require the upcoming drift work and
interior-marker simulation before milestone 6 can be considered complete. Default
device changes are also not followed yet. These limits are deliberately retained
in the milestone ledger instead of labeling the current capture path stable.

## Storage contract

The meeting controller initializes a new session folder before starting
capture. The storage layer in src/meeting_store.rs owns this format:

| File | Contract |
| --- | --- |
| meeting.json | Version 1 manifest: 16,000 Hz, one channel, s16le; start time in Unix milliseconds; recording/stopped/interrupted state; checkpointed frame counts in microphone/system order |
| microphone.pcm | Microphone track, signed 16-bit little-endian PCM, with no container header |
| system.pcm | System-audio track in the same format, kept separate for channel attribution |
| .lock | Advisory process lock held throughout writing or recovery; never unlinked as a way to reclaim ownership |

Track paths are fixed by the format. Creation refuses to reuse an existing
destination. On Unix, a new session directory is private (0700), new audio/lock
files are private (0600), and final path components are opened without following
symlinks. The temporary atomic manifest also uses private permissions.

Packets supply frame offsets on a common meeting clock. Gaps become silence;
overlapping packet prefixes cannot overwrite earlier audio. Empty packets do not
extend a track. Appending writes in fixed-size blocks, including long gaps,
instead of accumulating the whole recording in memory. This work runs on the
disk writer, never the realtime audio callback. Capture resampling and clock
alignment run in the shared controller, not in the GUI.

Each checkpoint syncs both audio files before atomically replacing and syncing
the manifest. The capture loops schedule one-second checkpoints;
the storage module itself does not start a timer. A write or
checkpoint failure prevents further writes and successful completion. The session
stays recoverable rather than being marked stopped by Drop or process teardown.

Recovery acquires the same exclusive lock, validates the manifest, and opens both
tracks before changing either. It refuses live ownership and unsupported formats.
For a recording left without an owner, it retains every complete on-disk sample,
including samples written after the last checkpoint, removes only an incomplete
trailing byte, syncs, and marks the session interrupted. Repeating recovery is
idempotent. It never marks a recovered session successfully stopped or resumes
recording into it. This covers process interruption; physical power-loss recovery
is limited to storage successfully synced by the OS.

Incremental inference can read bounded windows of at most 120 seconds without
loading an entire meeting. Capture and storage do not wait for an inference
result before preserving the next audio packet.

## Shared capture transport

Dictation's WAV sink and the meeting controller use AudioSource in
src/capture.rs. The source supplies native-rate, interleaved Int16 packets plus
the first sample's host-clock timestamp; it does not resample or mix tracks.
Native microphone capture retains its existing bounded CPAL queue, conversion,
overflow reporting, device errors, and stop-before-drain behavior.

On macOS the timestamp comes from CPAL's capture instant on the mach_absolute_time
clock, expressed as nanoseconds. Reading the same host clock uses mach2, already
present in CPAL's locked dependency graph. The tap adapter can use that common
clock; [CPAL's timestamp documentation](https://docs.rs/cpal/0.18.2/cpal/struct.StreamInstant.html)
explicitly identifies its Core Audio source. This is a macOS-specific alignment
decision, not an assumption that all CPAL backends share clock origins.

Explicit WAV fixtures retain their own epoch and derive packet timestamps from
delivered sample counts, so delayed polling cannot shift audio onto the poll
clock. Packet scheduling uses actual frame duration, including non-integral
20 ms rates and a short final packet. Stop freezes the fixture's arrival cutoff;
late draining or a repeated stop cannot extend the captured interval. A consumer
error stops draining before subsequent packets are consumed. No fixture is
selected automatically and these tests do not request microphone permission.

## System audio adapter

The macOS adapter in src/system_capture.rs explicitly creates a private,
unmuted mono tap on the selected output device's UID and stream zero. It excludes
the recorder's HAL process when available. Its private aggregate is selected by
the newly created UID, with usable input configurations required before it is
passed to the same CPAL input transport as the microphone. It does not select a
different physical input if the intended endpoint is unavailable.

An explicit tap is necessary even though CPAL has a loopback fallback. In the
locked version, selecting an output device that also supports hardware input
chooses that input, which can put microphone/line-in audio on the system track.
The fallback also hard-codes waiting for first playback and lacks a tap guard
between successful tap creation and failed aggregate creation. See the pinned
[selection branch](https://github.com/RustAudio/cpal/blob/e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7/src/host/coreaudio/macos/device.rs#L684-L735)
and [loopback lifecycle](https://github.com/RustAudio/cpal/blob/e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7/src/host/coreaudio/macos/loopback.rs#L86-L150).
The adapter supplies the missing selection/ownership policy; packet conversion,
timestamps, bounded queues, and streaming remain shared with microphone capture.

Aggregate tap auto-start is zero: Apple's SDK documents a nonzero value as making
device startup wait for playback. This avoids requesting that wait; actual startup
in silence still needs native acceptance. Stop drops the input stream before
destroying the aggregate and then the tap. Initialization failures use the same
ownership guard. Cleanup attempts both resources, propagates errors, and retains
failed IDs for a final Drop retry rather than reporting an unconditional success.
The current adapter is bound to the initial output; route-change recovery remains
upcoming work and is not claimed here.

Only an explicit system capture start can create devices or trigger its OS
permission flow. FSCRIPT_SYSTEM_CAPTURE_TEST_WAV explicitly substitutes a WAV
for that source, independently of the microphone fixture. No native system audio
has been captured in the checks described here.

## Native permission metadata and build verification

Native macOS builds require 14.2 or later. CPAL already imports the process-tap
functions in the executable, including when another action is selected; inspecting
the previous and new binaries established this existing minimum. The CLI's Mach-O
deployment target and the desktop package now declare it consistently. A local
symbol lookup would not make an otherwise strongly linked executable compatible
with older macOS versions.

resources/fscript-Info.plist is the shared source of microphone/system-audio usage
descriptions. The Rust build script preserves existing Clang runtime discovery
and embeds a versioned __TEXT,__info_plist section in the CLI. The app builder
uses the same descriptions. Apple's
[single-file tool guidance](https://developer.apple.com/library/archive/documentation/Security/Conceptual/CodeSigningGuide/Procedures/Procedures.html)
explains embedded Info.plist metadata; its presence is not a permission grant.

The builder takes the executable path from Cargo's compiler-artifact message,
including a configured CARGO_TARGET_DIR, and verifies its actual embedded metadata
before packaging. A shared Python checker validates both usage descriptions,
bundle identity, self-reported/embedded version, and the minimum deployment target.
Native client tests use that same checker. A stale same-version executable is not
accepted merely because it occupies the usual target/debug path.

Local verification used a separate Cargo target after the shared cache was found
rerunning a build-script executable from an older packaged checkout. It did not
remove or clean the user's shared target. The validated dictation app was not
rebuilt or replaced by this verification.

## Validation

Nine Rust backend checks cover separate tracks, gaps/overlaps, checkpoint versus
current progress, active-owner exclusion, non-overwrite, idempotent recovery,
malformed versions, write-failure poisoning, bounded reads, and symlink rejection.
The randomized timeline comparison uses 64 seeds and 128 interleaved packets per
seed against an independent per-frame reference. All 65,536 Int16 values round-trip.

A subprocess acceptance test creates and checkpoints both tracks, writes additional
samples and a partial frame, and remains alive while its parent verifies recovery
is refused. The parent then kills that owned child and verifies that recovery
retains the complete post-checkpoint samples, trims only the partial byte, and
records interruption. This is a backend process test, not evidence of native
system-audio capture, a meeting CLI, or meeting GUI acceptance.

Five capture checks include complete PCM preservation under delayed polling,
exact timestamp/PCM comparisons at five sample rates and one/two channels,
idempotent stop cutoffs, consumer-failure propagation, and the existing exhaustive
quantization property. Recording CLI and native process-client E2E checks compare
the refactored source with independent batch output and cover repeated clips,
cancellation, model reuse, and process cleanup. Native microphone and tap delivery
through the upcoming meeting controller still require their own acceptance.
The extraction passed 226 Rust checks, seven recording CLI E2E cases (one
additional platform-specific case skipped), and sixteen native client/app checks
on this Mac. No app bundle rebuild or permission refresh was needed for these
fixture-only backend checks.

The tap's cleanup property covers all combinations of partial ownership and
injected first cleanup failures, including idempotent success and retrying only
failed resources. It uses fake destroy functions and never creates native devices.
These checks and the metadata checks do not establish real system-audio capture,
silent startup, output-route following, or the first-use permission experience.
The adapter integration passed 227 Rust checks, the seven recording E2E cases
(one additional platform-specific case skipped), and sixteen native client/app
checks against the freshly isolated executable. A separate debug app bundle built
with that target passed matching CLI/app permission metadata, deployment minimum,
and deep strict signature verification. It was not launched.

## Reference decisions

The focused follow-up inspected OpenOats' pinned
[recorder](https://github.com/yazinsai/OpenOats/blob/d8caac78ea283dc791f8d1c4185450e445d3f336/OpenOats/Sources/OatsCore/Recorder.swift),
[live device management](https://github.com/yazinsai/OpenOats/blob/d8caac78ea283dc791f8d1c4185450e445d3f336/OpenOats/Sources/OatsCapture/LiveCapture.swift),
[system capture](https://github.com/yazinsai/OpenOats/blob/d8caac78ea283dc791f8d1c4185450e445d3f336/OpenOats/Sources/OatsCapture/SystemAudioCapture.swift),
and [resampling](https://github.com/yazinsai/OpenOats/blob/d8caac78ea283dc791f8d1c4185450e445d3f336/OpenOats/Sources/OatsCapture/Resampler.swift).
The useful patterns are separate raw tracks, a common host clock, explicit
interruption recovery, output-device rebinding, and complete tap/device teardown.
Our shared Rust service will own session state and inference; native capture is
an adapter used through that same service by the CLI and GUI.

[Apple's Core Audio tap sample](https://developer.apple.com/documentation/coreaudio/capturing-system-audio-with-core-audio-taps)
documents a tap feeding an aggregate device, explicit system-audio permission,
and NSAudioCaptureUsageDescription. The upcoming adapter must validate actual
sample delivery rather than infer success from device creation. CLI control will
remain under fscript, following [clig.dev](https://clig.dev/#subcommands), with
machine-readable stdout, diagnostics on stderr, and a fully unattended path.

Controller verification uses explicit WAV transports, real FFmpeg processes, the
actual CLI/service subprocesses, private storage, and a deterministic model worker.
It checks live pre-stop updates, exact SHA-256 recognition-input equality with saved
per-channel audio, source separation, one warm model for both tracks, final export,
durable status, active-owner recovery refusal, stdin interruption/recovery, graceful
first interrupt and forceful second cancellation, slow-inference-independent disk
progress/stop, recognition failures, cancellation during blocked final recognition,
stalled converters, missing/empty sources, and completion/cancellation consistency.

Three additional Rust checks cover 5 native sample rates × 1/2 channels and random
input fragmentation against an independent whole-file FFmpeg conversion, source
jitter/real gaps/cutoff against an independently constructed PCM timeline, and
idempotent shared cutoff conversion. The existing randomized store/chunker tests
remain applicable. None of these fixtures prove native microphone/system delivery,
independent-clock drift compensation, device rerouting, or meeting GUI behavior.
