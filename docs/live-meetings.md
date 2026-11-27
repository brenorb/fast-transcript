# Live meetings

Roadmap milestone 6 is in progress. The shared Rust storage/recovery foundation is
implemented; live meeting capture, incremental recognition, CLI control, and the
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
