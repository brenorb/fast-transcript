# Live meetings

Roadmap milestone 6 is complete. The meeting CLI connects shared capture,
continuous normalization, durable storage, incremental inference, final export,
and recovery, with adaptive clock alignment. Simultaneous native microphone and
system-audio capture passed on the built-in devices. Route/format recovery and
the [desktop meeting interface and acceptance](desktop-meetings.md) use the shared
service. Native Record/Stop and both menu-bar actions passed acceptance, with
hardware coverage limits documented alongside the results. Dictation is complete at
stable/dictation. Calendar/call detection remains milestone 7.

## Everyday use and unattended control

```sh
fscript meeting
fscript meeting --duration 60 --json
fscript meeting --control --json
fscript meeting status ./meetings/SESSION --json
fscript meeting recover ./meetings/SESSION --json
fscript meeting transcribe ./meetings/SESSION
fscript export ./meetings/SESSION/transcript.json --format speakers
fscript notes ./meetings/SESSION/transcript.json
```

The default output parent is ./meetings. Live preview text goes to stderr; after
success, stdout prints only the absolute final transcript path, matching file/URL
transcription. --quiet suppresses preview/status without removing that path.
--json keeps the structured event stream on stdout. Each start creates a new
private child;
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

## Transcribing retained audio

`fscript meeting transcribe SESSION` transcribes an existing session without
opening microphone/system devices or using an external audio converter. It needs
no terminal. The saved model preference and --model/--device/--model-dir overrides
are the same as live capture. --json emits the same session envelope; --quiet keeps
only the absolute final transcript path on stdout. Ctrl-C cancels immediately.
Model flags follow transcribe and its session argument; capture options cannot be
combined with a subcommand.

The command acquires the existing session lock before reading or repairing files.
An ownerless recording is automatically recovered, retaining complete samples
written after its last checkpoint and discarding only an incomplete final PCM
sample. Stopped/interrupted tracks must exactly match their manifest frame counts.
Missing, nonregular, linked, malformed, or empty-both-track input fails before model
loading. A single surviving track can be transcribed; no missing channel is
fabricated. Recovery never changes interrupted capture into stopped capture.

One loaded engine uses the same 30-second/2-second-overlap final recognition path
as a live meeting, in bounded disk reads, with no short-context preview pass.
restored replaces recording/saved events; no devices are opened and no live update
is implied. The completed result includes optional meeting metadata containing
the original capture manifest, including capture state and per-channel retained
frames. A completed transcript means all retained audio was processed; it does
not establish that an interrupted meeting was recorded in full. Consumers can
inspect result.meeting.state and result.meeting.track_frames directly. Other job
types omit this metadata, and old transcript documents remain readable.

The same exclusive lock now remains held from live capture through final
publication, and throughout saved-session recognition. Another recovery or
transcription attempt refuses an owned session. Successful recognition atomically
replaces transcript.json. Failure or cancellation preserves raw audio and any
previous completed transcript; a crash may leave a private pending file, which is
never a completed result and is replaced by the next successful attempt. Existing
partial previews are retained. Retry with another model uses the same command.

## Events and transcript publication

Session JSONL uses the shared dictation envelope: version 1, sequence (starting
at zero), type, nullable session_id, and data. The session ID is assigned by started;
ready precedes a controlled start and has no assigned ID. Events are flushed.
Diagnostics go to stderr; quiet suppresses status without removing result events.

| Event | Meaning |
| --- | --- |
| ready | Service accepts start; requires_start says whether control mode is enabled |
| started | Private directory exists; supplies directory and Microphone/System channel labels |
| restored | Saved-session transcription owns the retained tracks; includes their stopped/interrupted manifest |
| source_status | Bounded source snapshots: track, ready/reconnecting/receiving state, and optional reason; durable coverage lives in the manifest |
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

Each source has its own capture thread and one continuous Rubato 5.1.0 sinc
resampler. Native interleaved PCM16 is downmixed with equal channel weights and
converted to 16 kHz mono PCM16 in Rust. Meeting capture no longer launches FFmpeg;
file/URL decoding and dictation's completed-WAV normalization retain their existing
paths. The result's legacy used_ffmpeg_normalization flag is false for meetings.
Neither native callbacks nor disk writes wait for ASR. Both source loops checkpoint
at one-second intervals, and an independent timer stamps duration/signal stops
while inference is blocked. This does not bound arbitrary OS permission dialogs
or stalled storage.

One inference helper belongs to the existing supervised process group. It loads
one model, reads bounded immutable audio prefixes from disk, and uses the same
PcmChunker, InferenceOutcome merging, finalizer, and export schema as file jobs.
Preview opening windows are 2 seconds; after recognized text they become 8 seconds,
with 0.5-second overlap. A second chunker receives the same samples in 30-second
windows with 2-second overlap, using the same loaded engine and inference/merge
functions. Those longer-context results alone form the final transcript. Microphone
and System are processed in alternating bounded reads. Final windows are processed
as they become available during capture; after stop, the helper skips new preview
work and finishes only the remaining final windows. It does not reprocess the
entire meeting at the end. Buffers remain bounded by the window sizes. Slow
recognition accumulates an on-disk backlog, rather than an unbounded audio queue
in RAM. Recognition failures, including startup failures,
leave healthy capture running until the requested stop; the command subsequently
fails with the saved audio directory instead of claiming a complete transcript.

The resampler tracks cumulative native frames against the first-sample host
clock. Filtered phase feedback plus integrated frequency error compensate gradual
independent-clock drift, within a ±0.5% ratio range. A one-second ratio smoothing
step and an intra-block ramp avoid sudden rate changes. A hard rate derivative
limit was rejected because it wound up the integrator after timestamp steps.
Residual phase outside 100 ms fails capture rather than silently accepting an
untrackable clock.

A genuine positive adjacent-packet discontinuity over 100 ms flushes the previous
segment and leaves the missing interval silent. A backwards discontinuity over
100 ms fails with recoverable audio. Small timestamp jitter does not repeatedly
re-anchor the recording. Every arriving packet reads the shared stop cutoff again,
including packets drained after a slow disk callback. The tail is clipped to the
common cutoff; successful tracks end on the same 16 kHz frame. Empty or entirely
clipped sources cannot be turned into successful silent tracks.

Filter length grows above 48 kHz to preserve stopband rejection through 384 kHz.
The supported input is PCM16 with 1–64 channels and rates from 8 to 384 kHz.
Synthetic input preroll aligns the filter's initial sampling grid before integer
output cropping. It is computed immediately, without waiting for silence; the
remaining sinc-table phase quantization is below one microsecond. See the pinned
[Rubato release](https://docs.rs/rubato/5.1.0) and its source in the locked crate.

The resampler's input buffer is fixed at 1,024 mono frames plus filter history. The
output retains 320 frames (20 ms) before publication, so stop can clip queued audio
without rewriting saved prefixes. Flush padding has an explicit history bound;
it cannot accumulate a meeting-length audio buffer. Source packet queues remain
bounded in the shared transport. Meeting capture follows default-device and
hardware-format changes as described below; native hardware acceptance remains
part of milestone 6.

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
Each generation is bound to the physical output UID selected for it. The meeting
worker replaces it after a route/format change; the private aggregate input format
remains the independent decoding format. Native output-route acceptance is pending.

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
Our shared Rust service owns session state and inference; native capture is
an adapter used through that same service by the CLI and GUI.

[Apple's Core Audio tap sample](https://developer.apple.com/documentation/coreaudio/capturing-system-audio-with-core-audio-taps)
documents a tap feeding an aggregate device, explicit system-audio permission,
and NSAudioCaptureUsageDescription. The upcoming adapter must validate actual
sample delivery rather than infer success from device creation. CLI control will
remain under fscript, following [clig.dev](https://clig.dev/#subcommands), with
machine-readable stdout, diagnostics on stderr, and a fully unattended path.

Controller verification uses explicit WAV transports, continuous Rust resampling, the
actual CLI/service subprocesses, private storage, and a deterministic model worker.
It checks live pre-stop updates, exact SHA-256 recognition-input equality with saved
per-channel audio, source separation, one warm model for both tracks, final export,
durable status, active-owner recovery refusal, stdin interruption/recovery, graceful
first interrupt and forceful second cancellation, slow-inference-independent disk
progress/stop, recognition failures, cancellation during blocked final recognition,
absence of external meeting converters, missing/empty sources, and
completion/cancellation consistency.

Clock-alignment verification adds these checks:

| Check | Coverage |
| --- | --- |
| Independent FFmpeg differential | 8 native rates from 8–384 kHz × 1/2 channels, random packet sizes; a 300 Hz passband tone exceeds 45 dB direct waveform SNR after edge removal |
| Stopband rejection | 12 kHz at 48 kHz input; 9 kHz at 96/192/384 kHz input; output RMS below two PCM16 units after edge removal |
| Randomized clock property | 32 virtual hours, varying packet sizes, ±0.5 ms timestamp jitter and changing skew; analytic host position differs by less than 3 ms |
| Timestamp steps | Positive/negative 10/80 ms steps settle below 50 μs residual after 120 seconds without a growing oscillation |
| Unsupported drift | A 2% rate error fails instead of silently diverging beyond the supported correction range |
| Boundary/failure properties | Genuine gaps, backwards jumps, pre-epoch/post-cutoff input, empty sources, incomplete stereo frames, and failed sinks |
| Stop during queue drain | A sink requests stop while 128 packets are being delivered; subsequent packets observe the new cutoff and cannot extend the recording |
| Full-hour waveform acceptance | Two independent one-hour 16 kHz output streams, 16/44.1 kHz native inputs, changing ±150 ppm/constant −100 ppm clocks, jitter, and a 350 ms gap; all 120 interior markers retain amplitude and stay within 3 ms of host time |

The full-hour DSP check probes arriving blocks without retaining an hour of PCM.
It runs explicitly in optimized mode locally and in CI, rather than counting an
ignored default test as verified:

```sh
cargo test --release --locked simulated_hour_markers -- --ignored --nocapture
```

These are virtual clocks with actual resampling, not native-device or hour-long
ASR measurements. The existing randomized store/chunker tests remain applicable.
No check here proves device rerouting, native microphone/system delivery, or
meeting GUI behavior.

## Real speech cross-check and final recognition

The public [whisper.cpp JFK fixture](https://github.com/ggml-org/whisper.cpp/blob/master/samples/jfk.wav)
was verified against Git blob 3184d372cd2f8b804d3a540c70ec50d927b335d2 and SHA-256
59dfb9a4acb36fe2a2affc14bacbee2920ff435cb13cc314a08c13f66ba7860e.
Explicit WAV transports supplied its original 16 kHz microphone input and an
independent FFmpeg 48 kHz stereo conversion as system input. No native devices or
permissions were used. Recognition used the existing cached parakeet-v3-int8
ONNX model on CPU.

Whole-clip recognition of both saved normalized tracks preserves the original
word sequence exactly (case and punctuation ignored). The initial short-window
meeting result matched independent streaming recognition of the same saved PCM,
yet both produced poorer words than whole-clip recognition. This separates
resampling fidelity from recognition context: the 2/8-second preview profile can
make recognition errors despite retaining all audio. Final recognition now uses the 30/2-second profile instead. On the same
fixture, both final channel transcripts preserve the complete original word
sequence, and their segments exactly match independent batch recognition of the
saved PCM with those window settings. Preview updates still arrive before stop.
This single English clip is not a general speech-quality benchmark.

Two additional Rust checks compare final windows against batch across empty,
short, exact 30/58-second boundaries, and longer inputs, including randomized
arrival boundaries and stopping preview partway through a backlog. They verify
bounded input windows, unchanged audio coverage, metadata, and old JSON parsing.
Meeting E2E tests also check that a different final result replaces provisional
text, final-only failures retain the partial file, cancellation interrupts final
recognition, and plain/quiet output returns one final path without preview text
on stdout.

## Recognition timing metadata

Completed meeting documents use chunk_seconds 30 and chunk_overlap_seconds 2.
Their chunk list describes final recognition windows only. transcribe_seconds
counts both preview and final inference on both tracks. The optional
preview_transcribe_seconds field identifies the preview portion, so the final
chunk times plus that field equal transcribe_seconds. total_inside_seconds adds
the backend-reported model load once; derived speed fields use this total.
Transcription times measure elapsed engine calls, including a runtime's temporary
audio-file/IPC work and any lazy first-call setup. Capture waiting, capture-side
normalization/storage, rendering, and external notes are excluded. Backend-reported
model loading is not a measurement of the entire environment/setup pipeline.
Provisional snapshot metrics describe only their preview pass and are not final
processing totals. Other job types omit the new field; older JSON without it
remains readable.

The completed event replaces the full provisional document; clients must not
append it to preview text or assume a stable prefix. The internal helper stages
only a successful final result, and the existing controller still owns its atomic
publication/cancellation decision. Partial files never masquerade as final files.

## Saved-session validation

Rust ownership tests cover the lock after successful capture, after automatic
repair, and during saved-session processing. Empty sessions and changed track
lengths fail preflight. CLI E2E checks repair an incomplete final write, preserve
all complete samples, retain interrupted state, process unequal channel lengths
and a single surviving track, and compare a 59-second/31-second recording against
independent batch recognition with the same final window settings. They also
check that concurrent attempts cannot start another model, cancellation kills the
owned model process, failed retries preserve an existing transcript byte for byte,
and a subsequent successful quiet retry atomically replaces it. These tests use
explicit stored PCM fixtures and a deterministic model worker, not native capture.

A separate local CPU ONNX check replayed a copy of the verified public JFK session
after resetting its manifest to an uncheckpointed recording and appending one
incomplete PCM byte. Both final channel texts and segments exactly matched the
previous live final result, complete-sample SHA-256 digests were unchanged, preview
inference time was zero, and the capture manifest remained interrupted. This is
one English fixture with no native devices, not a general quality benchmark or
native meeting acceptance.

## Default devices and route/format recovery

The CLI meeting service follows the current default input and output. Passive
Core Audio queries poll physical UID and the complete hardware stream format
(including rate/channels) every 250 ms. Duplicate observations do not rebuild a
stream. Output fingerprinting does not impose CPAL's physical PCM format or
buffer-size restrictions: the separate mono aggregate input is the decoding
format. A tap is created with the UID from that same physical-route snapshot.
Readiness is announced only after checking the opened generation's binding.
These passive queries never create a tap or ask for permission.

A device/format change or known CPAL stream invalidation starts reconnection.
The old generation closes callback admission, stops/drops its stream, waits for
in-flight callbacks, and drains valid packets using its original format and
resampler. Its resampler flushes only through accepted audio, capped at the close-time host
observation and any existing stop cutoff. It does not pad to the time reconnection was detected.
Replacement audio uses a fresh resampler and its own host timestamps on the same
meeting epoch. Storage preserves old samples on overlap and fills positive gaps
with silence. Only final track shutdown pads through the common stop cutoff.

Reopen attempts wait 500 ms after a route change, then one second between known
transient failures, with at most eight attempts and a 15-second episode budget.
The latest route is selected at each attempt. Two seconds of verified stable
open transport resets this budget even if system output is idle; coverage remains
pending until replacement PCM is retained. Idle system output alone never starts
reconnection. Unknown/permission/setup/format errors and transport overflow remain
fatal; passive metadata unavailability and a confirmed aggregate-registration
timeout may retry. No background permission request or general permission retry
loop was added. Dictation retains its existing fixed-format WAV contract.

Both PCM and error callbacks participate in the quiescence boundary. Fatal errors
have a separate retained slot from retryable notifications, and overflow is
latched. A final drain retains good packets before checking these failures, so a
fatal error arriving after the last regular poll cannot become a successful save.
An observed permanent setup failure is retained even when a stop arrives at the
same time. The first capture-thread failure is retained before stopping its peer,
rather than replacing it with the peer's secondary no-audio error. Synchronous
pause errors also reach this decision. Route detection is stamped before teardown
and bounded by stop; finalization requires both track lengths to equal the shared
cutoff. A rare late cutoff publication that finds already retained audio past the
cutoff is an interrupted capture, not a successful misaligned recording.

The manifest's optional route_interruptions array records track, reason,
last_audio_end_frame, detected_frame, resume_frame, resolved_frame, and resolution.
Absent arrays in older version-1 manifests remain readable. On a successful
replacement, resolution is reconnected; the annotation is checkpointed only with
new PCM beyond the old saved prefix and the replacement's first frame. A short
packet still buffered in the resampler cannot publish a premature recovery claim.
An overlapping replacement must add novel audio before resolution. These records
are copied unchanged into result.meeting and saved-session transcription.

If stop/duration wins while both tracks have supplied valid audio at some point,
capture may finish stopped with an explicit stopped_during_reconnect record.
An opened but still undelivered replacement uses no_post_route_audio instead.
resume_frame remains null in both cases; resolved_frame is the common cutoff.
A track that never supplied accepted audio still fails rather than fabricating a
complete silent recording. Reconnection exhaustion or permanent capture failure
prevents saved/completed; after workers stop, healthy storage is checkpointed as
interrupted. A failed storage checkpoint, process crash, or cancelled-owner Drop
can instead leave recording for explicit recovery. Unresolved interruption records
remain unresolved during repair; byte lengths cannot establish a resume time.

A route interruption records uncertain coverage, not a claim that audible speech
was lost: an output device can be legitimately idle. source_status is bounded,
nonblocking UI/agent status; the durable manifest is authoritative. Retry sleeps
and aggregate-discovery polling observe stop/cancel. Synchronous OS permission or
driver calls themselves still have no hard wall-time guarantee; process supervision
remains the final cancellation boundary.

## Route validation and limits

Randomized packet-boundary tests switch from 16 kHz mono to 44.1/48/96 kHz stereo,
retain terminal/initial waveform markers, verify sub-3-ms marker placement, flush
old tails without wall-time padding, preserve exact zero gaps, and keep the old
byte prefix immutable during overlapping generations. A callback-gate test covers
in-flight terminal packets and closed admission. Native-transport tests use
constructed queues (no devices) to inject a final valid marker, a retryable notice,
and a later fatal error/overflow; good samples remain available while success is
rejected. Store tests require newly retained samples before checkpointing resume.

CLI E2E uses explicit scripted WAV route substitutions: format switches while the
peer keeps recording, persisted coverage, exact saved/live transcript agreement,
stop and cancel during retry, retry exhaustion with transcribable retained audio,
and a healthy idle replacement followed by another route change. Existing shared
recording and file/export regressions remain applicable. Hour-long waveform and
clock alignment acceptance still runs separately in release mode. These checks
prove the shared controller and normalization behavior under simulated routing;
they do not prove real USB/Bluetooth switching or first-use OS permission behavior.
The later native and GUI checks are recorded in [desktop meetings](desktop-meetings.md).

A lock-lifetime regression deliberately retains the lock's open-file description
in a disposable child. Explicit owner unlock releases it even while that child
lives; normal descriptors still use close-on-exec and the lock inode is never
unlinked. This also removes a transient lock-retention failure observed during
parallel process tests. A stop-during-tail-flush check covers future-dated fixture
frames and delayed cutoff publication, requiring exact final length or an
interrupted result. Global --json placement remains valid on either side of the
meeting verb while capture-only options are rejected with subcommands.

## Native capture acceptance follow-up

The committed desktop bundle captured the real built-in microphone and system
output together, showed recognized channel rows before Stop, and finalized both
tracks at 542,866 frames (33.929125 seconds). Each retained track matched an
independent batch run exactly: two microphone segments and four system segments.
The [full acceptance record](desktop-meetings.md#native-audio-acceptance) identifies
devices, transport, model and coverage limits. Earlier sections record the evidence
available at each implementation checkpoint; their fixture-only limits do not
supersede this later native check. Physical route switching and first-use grant
UX remain unverified.
