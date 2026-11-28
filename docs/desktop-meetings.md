# Desktop meetings

The desktop meeting interface uses the same `fscript meeting --control --json`
service as unattended clients. Saved audio uses `fscript meeting transcribe DIR`.
No audio capture, model loading, resampling, transcript merging, recovery policy,
or note-agent adapter is implemented in Swift.

Click **Record meeting**, then **Stop meeting**. Microphone and system text appears
in the main document as recognition arrives. Short-context text is provisional;
the final result replaces it. Audio is saved independently of recognition. If
recognition fails, the app reports that capture continues until Stop. After
completion, the existing Copy, Export, and Create notes actions call the same CLI
commands as file transcription. External notes remain an explicit action using
the selected agent and its own account.

While a meeting is active, the macOS menu bar contains its recording/progress
indicator and Stop/Cancel actions. Closing the main window keeps capture running;
Show transcript reopens it. Removing the indicator cancels the owned job, retaining
saved audio, so it cannot leave an invisible recording. Quit cancels the supervisor
and waits for process cleanup. A microphone/system permission or driver call is
still governed by macOS; the supervisor owns the final termination boundary.
The menu uses Apple's [MenuBarExtra](https://developer.apple.com/documentation/swiftui/menubarextra),
available before the app's macOS 14.2 minimum.

Starting a meeting stops the warm dictation helper before launching the meeting
service. File jobs, dictation, model changes and notes are disabled during capture
and final recognition. Previously prepared shortcut dictation is restored only
after meeting cleanup, and is suppressed on Quit. Late dictation callbacks cannot
replace meeting text or send an old paste. Stopping before the CLI start request
cancels preparation without opening a capture session.

**Open last meeting** reads the existing completed document with `fscript export`
and does not transcribe it again. If no completed result is available, **Transcribe
saved audio** runs the saved-session command. File > Transcribe saved meeting
audio selects another session folder. This can use a newly selected model and
needs no recording permission. The same command repairs ownerless recordings;
interrupted capture remains interrupted after successful recognition.

Coverage changes are shown beside the transcript. The source status is provisional;
the completed document's `meeting` manifest is authoritative. Device switching or
an interrupted recording does not establish that audible speech was lost, but
recognition completion also does not establish full meeting coverage.

## Validation

The native decoder checks version/sequence/session identity, capture/model
readiness in either order, saved/restored manifests, cancellation and terminal
publication. Randomized byte fragmentation preserves Unicode and the complete
final replacement. Presenter E2E runs the real CLI with explicit WAV transports
and a deterministic model, verifies updates before Stop, saved replay agreement,
stop before capture, and bounded shutdown during a confirmed blocked inference
request with no surviving model process. Existing native file, dictation,
export/process and window-lifecycle tests remain in the same suite.

### Actual controls and real local inference

Checked on 2026-11-27 on the same Apple M5 Max/macOS 27.0.1 host as the earlier
desktop validation. The isolated ad-hoc-signed development bundle used its own
matching CLI, preferences, model configuration, notes configuration, and output
root under /tmp. The user's installed bundle and defaults were unchanged.
Explicit public-speech WAV transports supplied microphone (16 kHz mono) and
system (48 kHz stereo) input. Neither native audio device was opened and no audio
was played. Recognition used cached parakeet-v3-int8 ONNX locally on CPU.

| Control/workflow | Observed result |
| --- | --- |
| Record meeting | One click started the CLI; Microphone/System rows appeared before Stop. |
| Stop meeting | Closed both tracks at a shared frame and replaced provisional text with the final document; Copy/Export/notes became available. |
| Dictation/resource exclusion | Hold, model/agent changes and file actions were disabled during capture and saved replay. An initial GUI check exposed an enabled native Hold bridge; both the SwiftUI disabled state and guarded presenter entry points now prevent that second recording. |
| Close/reopen | Closing the main window kept the same CLI session running. Opening the bundle from Finder reopened exactly one document in the same app process; live rows continued and Stop completed the same recording. |
| Transcribe saved audio | After cancellation retained raw audio, the button repaired the ownerless session and produced a final transcript. Its manifest remained interrupted and the GUI kept the coverage notice. |
| Open last meeting | Read the completed document through CLI export; showed channel rows and capture coverage without loading a model. |
| Create notes | One click used an explicitly registered local deterministic test agent and saved/displayed Markdown from the recovered transcript. This checks integration, not another real-provider login. |
| Quit | Closed the isolated app; no owned meeting/model helper survived. Automated shutdown also covers a confirmed blocked inference request. |

The first stopped GUI recording retained 1,226,074 frames per channel. Each track
was independently recognized in batch with the same real model and 30-second,
2-second-overlap final settings: all four segments per channel matched exactly.
The close/reopen recording retained 890,561 frames per channel and completed
normally. The recovery check retained 5,279,301 microphone and 5,279,389 system
frames, with interrupted capture preserved in the completed document. These are
correctness checks, not latency, memory, or general speech-accuracy benchmarks.

The window check exposed AppKit status-item windows being counted as documents.
Reopen now selects main-capable document windows, otherwise uses the registered
SwiftUI opener, and returns false after custom handling. A regression test covers
visible status windows. The real reopen log recorded a new document 99 ms after
the callback, with the original process/session preserved. An accessibility
snapshot then timed out; reacquiring the app showed the reopened recording and
allowed Stop. The snapshot timeout is not evidence of a window failure.

The native suite now has 24 passing tests (18 protocol/process client tests and
six presenter/window tests), including Unicode fragmentation, invalid lifecycles,
500 resource-exclusion states, batch/live/saved agreement, preparation stop, and
bounded cancellation after an inference request is confirmed.

### Native audio acceptance

The same frozen committed development bundle then ran without either WAV
transport override. Clicking Record opened native microphone capture and a Core
Audio process tap simultaneously; existing OS grants allowed both without a new
prompt. The defaults were MacBook Pro Microphone (48 kHz, one input channel) and
MacBook Pro Speakers (48 kHz, two output channels). The tap mixes system output
to mono; shared Rust normalization saves each track as 16 kHz mono PCM16.

The checked-in public 15-second Portuguese speech sample was played with afplay
through the normal output during the recording. Both channel labels showed
recognized words before Stop. Stop completed capture and recognition, with no
route interruption in the manifest. Raw tracks were independently wrapped as
WAV and passed through the packaged CLI with the same cached model, CPU device,
30-second chunks and 2-second overlap.

| Track | Retained frames | Duration | Nonzero samples | PCM peak | Final segments | Independent batch agreement |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| Microphone | 542,866 | 33.929125 s | 500,738 | 23,378 | 2 | Exact timestamps and text for every segment |
| System | 542,866 | 33.929125 s | 239,578 | 18,154 | 4 | Exact timestamps and text for every segment |

Both sources delivered nonempty audio and ended at the same frame. The microphone
also picked up playback acoustically; this does not demonstrate echo cancellation
or separation of individual remote speakers. This check establishes working
native simultaneous capture, pre-stop local recognition, final publication and
batch agreement on the current built-in devices. The app was then closed and its
owned capture/model processes exited. No real external agent was invoked.

### Remaining acceptance

Actual menu-bar Show transcript/Stop controls have not been operated. Computer
Use hung while inspecting the system menu-bar surface; that surface is excluded
from further automation; a short manual menu check is pending. The Finder reopen
path is verified separately. Real USB/Bluetooth switching, a first-use OS grant
flow, echo cancellation and individual participant diarization remain unverified.
The core native capture check above does not establish those behaviors. A stable
milestone-6 tag still awaits the final menu check and green CI.
