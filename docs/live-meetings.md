# Live meetings

Roadmap milestone 6 is in progress. The shared Rust storage/recovery foundation is
implemented, both capture sources share timestamped packets, and the native tap
adapter compiles. Native system-audio delivery remains unverified; live meeting
capture, incremental recognition, CLI control, and the
meeting GUI are not yet available. Dictation is independently complete at
stable/dictation. Calendar/call detection remains milestone 7.

## Storage contract

The meeting controller will initialize a new session folder before starting
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
alignment belong to the upcoming capture adapter, not a second GUI implementation.

Each checkpoint syncs both audio files before atomically replacing and syncing
the manifest. The controller must schedule a bounded checkpoint interval;
this storage module does not start a timer or claim one is running. A write or
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

Dictation's WAV sink and the upcoming meeting controller use AudioSource in
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
The current adapter is bound to the initial output; route-change recovery belongs
to the upcoming controller and is not claimed here.

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
