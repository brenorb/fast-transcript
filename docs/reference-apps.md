# Reference application study

Source inventory checked on 2026-11-26. These are design references, not vendored
dependencies. Complete recursive source trees were inventoried; the paths below
were inspected for the requested workflows. This is a focused architecture study,
not a line-by-line audit of all three applications.

| App | Pinned revision | Relevant paths inspected | Apply to Fast Transcript |
| --- | --- | --- | --- |
| [Handy](https://github.com/cjpais/Handy/tree/f236901e00b6ed44095b8b9eef90383fb87bda39) | `f236901e00b6ed44095b8b9eef90383fb87bda39` | `src-tauri/src/cli.rs`, `shortcut/handler.rs`, `managers/transcription/live.rs`, `managers/transcription/chunked.rs` | Hold/toggle shortcuts, isolated model execution, live versus chunked backend capabilities, and shared batch/live decoding. |
| [Say It](https://github.com/callebtc/sayit/tree/c02707a0745a21ecedff79725eabd156e988073e) | `c02707a0745a21ecedff79725eabd156e988073e` | `Sources/SayItCLI/CLIService.swift`, `Sources/SayItBackend/Synthesis/PocketTTSStreamRunner.swift`; protocol, XPC, HTTP, synthesis and model source inventory | One versioned service contract for GUI/CLI, cancellation, model reuse, small opening chunks and larger steady chunks. TTS model optimizations themselves do not implement ASR streaming. |
| [OpenOats 1.x](https://github.com/yazinsai/OpenOats/tree/b79fe07d60d326c6bc57fd1a7bc0aa2f5ab6b8e7) | `v1.87.0`, `b79fe07d60d326c6bc57fd1a7bc0aa2f5ab6b8e7` | `Meeting/CalendarManager.swift`, `Meeting/MeetingDetector.swift`, `App/MeetingDetectionController.swift`, `Transcription/StreamingTranscriber.swift` under `OpenOats/Sources/OpenOats` | Live local mic/system transcription, VAD with preroll, contextual calendar association, optional automatic recording. Avoid its ambiguous global-microphone detection. |
| [OpenOats current](https://github.com/yazinsai/OpenOats/tree/d8caac78ea283dc791f8d1c4185450e445d3f336) | `d8caac78ea283dc791f8d1c4185450e445d3f336` | `docs/adr/0005-meeting-detection-asks-about-the-app-on-the-mic.md`, `docs/adr/0006-meetings-are-named-after-calendar-events-optionally.md`, `OatsCore/CallDetector.swift`, `OpenOats/CalendarTitles.swift` | Attribute microphone activity to the actual call app; require corroboration; separate calendar naming from detection. |

## How OpenOats connects calendars and decides when to record

In 1.x, `CalendarManager` uses Apple's EventKit (`EKEventStore`), reading calendars
already connected to macOS. It requests full event access when calendar integration
is enabled. There is no separate Google/Microsoft OAuth connection in that path.
It queries events near now (a 15-minute window on either side), filters excluded
calendars and all-day events, and ranks likely meetings using attendees and join
links. At the end it can rebind using the recording's overlap with the event.

The Google Calendar path was rechecked on 2026-11-28 against current upstream
`d8caac78ea283dc791f8d1c4185450e445d3f336` (unchanged from the pinned revision).
[CalendarTitles.swift](https://github.com/yazinsai/OpenOats/blob/d8caac78ea283dc791f8d1c4185450e445d3f336/OpenOats/Sources/OpenOats/CalendarTitles.swift)
requests EventKit full access and reads all locally visible event calendars.
[SettingsModel.swift](https://github.com/yazinsai/OpenOats/blob/d8caac78ea283dc791f8d1c4185450e445d3f336/OpenOats/Sources/OpenOats/SettingsModel.swift)
invokes it from the naming toggle. There is no separate Google sign-in in this
calendar path. Google must be connected to macOS Internet Accounts with Calendars
enabled; granting OpenOats or Fast Transcript Calendar access alone does not add
that account. The user can continue using Google's browser interface. See
[Fast Transcript setup and native coverage](calendar.md#google-calendar).

Recording detection is separate: physical input-device activity comes from Core
Audio HAL listeners; a five-second debounce correlates it with running meeting
apps. Camera activity can also trigger detection. The controller either prompts
or accepts automatically when `autoRecordDetectedMeetings` and the prior recording
acknowledgement are enabled. Auto-start requires its recording notification to
succeed; otherwise it falls back to prompting. This explains why a calendar event
alone is not proof that the user joined a call.

Current OpenOats replaces that correlation with per-process Core Audio evidence.
The same recognized app must use input and output, with camera activity allowed
as output corroboration. Confirmation takes five seconds; twenty seconds without
that app using input ends the call. Current detection prompts once per call and
only auto-stops a recording started from that prompt. Calendar access is optional
and used for naming after Stop, choosing a sufficiently overlapping real meeting.
The current design explicitly rejects automatic recording, while the older
release implemented it. Fast Transcript will retain the requested opt-in automatic
policy with the newer, better-attributed detector.

## Streaming and large inputs

Handy now supports headless file transcription and shared live/long-form chunks,
so an installed older Handy's limitations should not be presented as limitations
of current upstream. Its live path distinguishes backend-native streaming from
pause-based chunking and falls back to batch if finalization fails.

Say It's short first synthesis flush and model/voice warm caches inform our
latency strategy. For ASR, chunk duration also affects recognition context and
accuracy; measure that tradeoff. File streaming requires incremental decoding,
not merely emitting text after a whole-file download. URLs should resolve a media
stream and decode progressively; partial requests should seek/download a section
instead of transcribing unwanted content. Preserve absolute media timestamps.

Meeting capture needs independent mic/system channels and crash-safe audio on
disk. Agent-generated notes operate on transcript text through external commands;
the app owns job execution and result storage, not another LLM client stack.

## Coverage and remaining evidence

All four pinned source trees above were accessible through the GitHub connector.
The public web search was used to identify the upstream repositories; pinned code
is the source of implementation conclusions. Local indexed discovery confirmed
existing Say It checkouts, but they were not treated as current upstream. Runtime
testing of these reference apps, exhaustive feature parity, and their release
packaging were not performed in this study. Native capture, permissions, and call
detection must be validated on this Mac when those milestones are implemented.

## Paste and permission follow-up (2026-11-27)

The native paste failure was reviewed independently with Astra at high reasoning.
This focused follow-up revisited the pinned source above; it does not claim full
feature parity or runtime testing of the reference apps.

| App | Actual event/output path | Permission and process ownership | Applied lesson |
| --- | --- | --- | --- |
| Handy | A serialized app paste worker calls main-thread Enigo. It resolves the V key for the active layout, places text on the clipboard, and sends a Command-V chord with a 100 ms Command hold. Its normal path restores the clipboard with settling waits; optional reliable paste observes a clipboard consumer read. | Tauri's macOS permission adapter checks/requests Accessibility. The app and its event sender remain alive. Its pinned Enigo implementation also delays teardown according to pending events. | A successful posting call is weaker than observed insertion. Test sender lifetime separately from focus and modifier timing. Retain Fast Transcript's explicit target PID and copied-text fallback; clipboard restoration is a separate product choice. |
| Say It | The selection agent reads Accessibility selection and can send Command-C to the captured PID using a combined-session CGEventSource. It checks foreground identity while retrying and observing clipboard changes, then restores its snapshot. This is selection ingestion for TTS. | Accessibility belongs to a long-lived selection helper. The packaging script signs embedded code with a supplied identity and preserves requirements. | PID targeting and focus checks are compatible with native keyboard events. Its sender outlives posting; a short-lived CLI needs separate teardown validation. |
| OpenOats | The inspected output path writes meeting files; the older exporter activates its app before showing a save panel. Current capture/onboarding explicitly request microphone and system-audio access and report the result of attempting capture. | Capture lives in shared native modules; processing uses the bundled oats child. Its builder supports Developer ID/Apple Development identities and signs nested code before the app. | Validate the operation behind a permission status. Reuse explicit native setup and shared capture/process ownership when implementing meetings. This inspected path supplies no keyboard-paste recipe. |

Primary implementation references:

- Handy [key handling](https://github.com/cjpais/Handy/blob/f236901e00b6ed44095b8b9eef90383fb87bda39/src-tauri/src/input.rs#L67-L201),
  [clipboard flow](https://github.com/cjpais/Handy/blob/f236901e00b6ed44095b8b9eef90383fb87bda39/src-tauri/src/clipboard.rs#L109-L147), and
  its pinned Enigo [pending-event timing](https://github.com/enigo-rs/enigo/blob/b297a14e807abdf818b7809a70b445ade4fc5897/src/macos/macos_impl.rs#L914-L928) / [teardown](https://github.com/enigo-rs/enigo/blob/b297a14e807abdf818b7809a70b445ade4fc5897/src/macos/macos_impl.rs#L1179-L1184).
- Say It [selection reader and PID-directed copy](https://github.com/callebtc/sayit/blob/c02707a0745a21ecedff79725eabd156e988073e/Sources/SayItSelectionAgent/SelectedTextReader.swift),
  [retry flow](https://github.com/callebtc/sayit/blob/c02707a0745a21ecedff79725eabd156e988073e/Sources/SayItXPC/SelectionCaptureFlow.swift), and
  [embedded signing](https://github.com/callebtc/sayit/blob/c02707a0745a21ecedff79725eabd156e988073e/Scripts/sign-embedded-code.sh).
- OpenOats [capture permission setup](https://github.com/yazinsai/OpenOats/blob/d8caac78ea283dc791f8d1c4185450e445d3f336/OpenOats/Sources/OpenOats/SettingsModel.swift#L151-L168),
  [processing child ownership](https://github.com/yazinsai/OpenOats/blob/d8caac78ea283dc791f8d1c4185450e445d3f336/OpenOats/Sources/OpenOats/AppModel.swift),
  [signing](https://github.com/yazinsai/OpenOats/blob/d8caac78ea283dc791f8d1c4185450e445d3f336/scripts/build_swift_app.sh), and
  [1.x export panel activation](https://github.com/yazinsai/OpenOats/blob/b79fe07d60d326c6bc57fd1a7bc0aa2f5ab6b8e7/OpenOats/Sources/OpenOats/Views/ContentExporter.swift#L54-L72).

The local differential isolated sender lifetime without changing the granted app:
the same old export executable inserted text only when held alive after posting,
in both test orders. With normal stdout and normal process exit, the 100 ms fix
then produced three exact native Paste callbacks; two old-binary controls exited
successfully without callbacks or insertion. See [native paste testing](native-paste-testing.md).

The route follow-up also inspected pinned OpenOats MicCapture.swift and
MicRebuildTests.swift alongside LiveCapture.swift. Applied patterns are separate
physical-route versus capture-format identity, same-UID hardware-format changes,
bounded/coalesced reopen behavior, and not interpreting idle output as a stall.
Fast Transcript retains its own Rust callback, resampling, durable coverage, and
process-ownership paths rather than adding a separate native meeting service.
