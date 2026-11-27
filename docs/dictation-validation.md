# Dictation validation

Roadmap milestone 5 is complete. Real microphone capture, physical global
shortcut capture, exact packaged paste insertion, toggle persistence across an
independently confirmed app switch, and focus-change paste suppression pass.
The final physical shortcut acceptance is recorded below. Stable release
distribution and persistent signing remain roadmap milestone 8.

## Automated coverage

- 213 Rust checks on the staged backend, including 65,536 PCM conversion cases
  and 10,000 native hold-edge patterns (repeat suppression and overflow detection).
  A delayed-poll fixture regression verifies elapsed-time catch-up and exact PCM
  preservation without extending the source at EOF. The first macOS CI run
  exposed fixture truncation under scheduling delays; exact batch parity remains
  required rather than tolerating different transcript hashes.
- Six recording process E2E cases with deterministic capture and a fixture model:
  duration/batch parity, two clips with one worker load, cancellation/export,
  disconnects/invalid transitions, first/second interrupt behavior, and validation
  before model loading. These cover real WAV/FFmpeg and process ownership.
- A model-precedence regression covers saved selection, environment override,
  explicit flag precedence, malformed settings before worker loading, and batch
  parity. GUI acceptance exposed that recording had bypassed the shared saved
  default lookup; the recording action now uses the same resolver as media jobs.
- Clipboard export compares renderer bytes through a literal fixture command,
  rejects invalid/implicit paste targets, and preserves source documents on failure.
- Sixteen native checks include real-CLI repeated recordings versus an
  independent batch document, one worker start, broken/full-duplex pipes, and
  500 fragmented Unicode recording lifecycles. These are fixture-capture tests.
- The app's close observer has a 500-case hosting-window/detachment property
  regression using unshown windows in the test process. Notifications from other
  windows must never finish dictation, and detachment must remove the observer.

## Observed GUI and real-model evidence

On the local macOS app with cached parakeet-v3-int8 ONNX CPU inference and a
prerecorded public 15-second speech fixture selected explicitly through
FSCRIPT_CAPTURE_TEST_WAV:

- Native hold-button clicks of 4.5 seconds produced 4.48 seconds of recorded audio;
  the complete 15-second fixture produced recognized text and automatic copy.
- A short initial cold hold and subsequent warm holds used one loaded model;
  completed metadata reported load_seconds 0 for subsequent clips.
- The user physically held Control-Shift-Space from another application. The
  red/gray recording indicator appeared and capture stopped on release. The saved
  clip had 7.06 seconds of test audio; its text, segments, duration, chunk settings,
  and chunk count matched an independently cropped batch inference result.
- App-targeted Computer Use chord simulations did not trigger that OS global
  shortcut. Physical-keyboard evidence is recorded separately from simulated input.
- Quit with a warm idle controller terminated the app and its two owned recording
  supervisor/worker processes. Their PIDs no longer existed after termination.
- The current release candidate passed Command-Shift-D start/finish, a seven-second
  native button hold/release, automatic copy, and cancellation during capture.
  The resulting transcript was visible in the app; cancellation retained the prior
  completed transcript and returned the controls to idle.
- Resetting the isolated model preference returned to Ready and allowed another
  recording. Quit during that active recording left no app or bundled recording
  helper processes. This checks preference reset, not switching inference engines.
- Short empty transcriptions were compared with equivalent batch intervals and
  matched; the same fixture/model recognized the longer clip. No voice from the
  user's microphone was recorded during these fixture checks.

The physical shortcut check preceded the exclusive-registration/automatic-fallback
improvement. The later live session below checks the selected chord.
Native CLI registration checks on 2026-11-27 confirmed that two live controllers
selected Control-Shift-Space and Control-Shift-D respectively. An explicit attempt
to claim the occupied Space chord reported shortcut_available false and Carbon
error -9878 while control recording/cancellation still worked. After shutdown,
the original chord registered successfully again. These checks used fixture audio
and verify collision/fallback/cleanup, not physical keyboard-event delivery.
The human CLI fallback check reserved Space with a second fixture controller;
the candidate announced Control-Shift-D on stderr before any gesture, then Ctrl-C
exited with status 130 and empty stdout. The GUI caption/help correction compiled,
but its rendered fallback check could not run because the Mac was locked and
Computer Use could not unlock it. The test app and shortcut holder were closed.
The model timings above describe model loading only, not total capture-to-text
latency. No general model latency/quality claim is derived from one speech fixture.

The fixture-permission correction was then checked with the real cached Photon
SDK (moondream 2.4.1, kestrel 0.8.1, torch 2.14.0) on local MPS inference, using
the existing benchmark interpreter and offline model retrieval. Cold Redux
(8.3 seconds of fixture audio), warm Redux (15 seconds), and Ultra after a menu
switch (13.74 seconds) each matched independent batch CLI runs exactly for text,
segments, duration, chunk settings/count, and model metadata. The warm Redux
result reported load_seconds 0. No microphone permission prompt appeared in these
fixture tests. These are real ASR models with prerecorded capture, not microphone
recognition or model performance benchmarks.

With prerecorded public speech and an explicitly controlled SDK worker:

- The app reached Transcribing dictation while the worker was blocked after its
  inference-entry marker. Clicking Cancel returned to Dictation cancelled; the
  supervisor, recording worker, and marked model worker were absent afterward.
  No additional completed transcript was written for the cancelled inference.
- Selecting parakeet-ultra and then parakeet-redux launched distinct model
  workers with the corresponding repository arguments. The Redux result recorded
  model name parakeet-redux and runtime photon, displayed the fixture digest, and
  copied successfully. This verifies routing and lifecycle, not SDK model quality.
- A latched recording remained active while the blank native test window received
  a click, then finished through Command-Shift-D. An app-targeted automation click
  alone is not independent evidence of the system-wide foreground application.

## Permission investigation and remaining checks

System Settings showed Fast Transcript's Device Control and Data Access enabled,
while the running development app's AX trust check returned false. The exact
bundle path was confirmed through Show in Finder. A restart did not reconcile
that check. This was not evidence that the user's switch was disabled.

The app is ad-hoc signed with a version-specific designated code requirement;
no valid persistent signing identity was found in the local public identity
inventory. The authorization log then explicitly reported a failed match against the saved
code requirement for kTCCServiceAccessibility: the saved and rebuilt cdhash values
differed. The new candidate logged both AX and event-posting preflight false for
its actual PID, bundle identifier, and confirmed bundle path. This establishes
the build-identity mismatch; the Settings switch was enabled. The code now uses the operation-specific CoreGraphics
preflight in the delivering process, records app identity/capabilities in the
paste log category, and copies the text if targeted paste fails. No permission
switch is automatically reset by the app. The builder now accepts an existing
--sign-identity (or FSCRIPT_SIGNING_IDENTITY), signs nested code before the app,
and verifies before replacement. No identity is created or imported automatically.

The user authorized automatic-paste validation and completed the OS Touch ID
step. Automatic approval review rejected an attempted permission toggle because
the authorization covered enabling access, not disabling it. The enabled setting
was retained. Validation must proceed through the existing grant and app/build
corrections; do not treat that rejection as permission to reset TCC elsewhere.

A candidate test also found that app deactivation was cancelling the accessible
Start intent during model preparation. Toggle intent is now separate from a
physical hold: it persists across app changes, shows a visible recording indicator,
and finishes when the main window closes. Physical holds still release on focus
loss. The acceptance checks below validate these controls.

Acceptance coverage and limits:

- Physical shortcut capture followed by the packaged GUI's paste decision and
  focus-change fallback pass below. Toggle persistence across an independently
  verified app change, real SDK model switching, and controlled inference
  cancellation pass as recorded separately.
- Physical microphone transport and first-use approval now pass as recorded below.
  Denied-device handling has not been exercised through the real OS dialog;
  recognition quality has not been measured against a spoken reference.
- The fixed shared CLI inserts exact text into a blank native field, both from
  the packaged app's runner and after the physical shortcut through its GUI.
- Physical keyboard delivery for the selected global chord now passes below;
  native exclusive registration, automatic fallback, and release-on-shutdown passed.
- Stable checkpoint tags are created only after the pushed acceptance
  documentation and implementation have green CI.

A disposable native target lives in tests/fixtures/PasteTarget.swift. It creates
only an empty text window, opens no user documents, and saves no files; compile
and run it solely for GUI paste validation. It is not a second product command
and is never bundled with the desktop application or CLI wheel.

The desktop skips microphone authorization when FSCRIPT_CAPTURE_TEST_WAV is
explicitly present: this backend path opens only that WAV, not an audio device.
Earlier GUI fixture builds unnecessarily requested microphone access, which
combined with ad-hoc rebuild identities caused repeated prompts during validation.
Real capture retains the normal AVFoundation authorization flow; denied access is
reported rather than repeatedly requested or automatically changed in Settings.

The user subsequently authorized live microphone validation and handling its
permission request. A native-capture attempt reached the authorization wait, but
Computer Use refused access to the system UserNotificationCenter application for
safety reasons. The owned app was closed without granting that prompt or making
a live recording. This is a tool restriction, not missing user authorization.

After the user unlocked the desktop, the rendered fallback caption and button help
both showed Control-Shift-D while another fixture controller held Space; button
dictation completed and copied. A lifecycle check exposed that the old primary
Window scene quit on close and discarded toggle capture. The app now remains
available after closing its window and has an explicit shared-workspace reopen
action. During development, close-to-finish saved a fixture clip and reopening
displayed that completed transcript.

The window handler is scoped to the exact hosting NSWindow, excluding menu and
auxiliary-window closures. With the scoped candidate, opening and dismissing the
app menu left the toggle visibly Listening; the 500-case regression also passed.
Lifecycle diagnostics record state and intent only, never captured text. The Mac
locked again before the final combined close/reopen/Quit acceptance check. The
later live session below covers close/reopen, Quit, and microphone capture;
effective paste checks remain pending. Native unit and process tests do not
substitute for them.

## Live microphone acceptance, 2026-11-27

The user approved the pending microphone prompt for the existing candidate; the
macOS authorization log recorded the successful decision for that same process.
No bundle rebuild, permission reset, or fixture capture override was used.

- A native 4.5-second hold/release produced a saved 4.3946875-second document with
  input_source microphone and transcript_source local-microphone. The local ONNX
  model produced nonempty text, and the GUI reported Dictation copied. This is
  transport and inference evidence, not a controlled recognition-quality result.
- Command-Shift-D started a second live recording. Closing the main window
  finished and saved 14.144 seconds; the reopened window retained the completed
  result and reported No speech recognized. The lifecycle log confirmed the exact
  hosting-window close intent during recording, followed by the view disappearance
  during processing. The second clip reported load_seconds 0.
- Enabling automatic paste in the isolated test preference logged ax=false and
  post=false for this same app process. The microphone grant did not repair the
  separate Accessibility code-requirement mismatch. The enabled Settings switch
  was retained; effective targeted paste remains unverified.
- Native menu Quit during a third live recording cancelled capture. The app and
  both recorded supervisor/worker PIDs disappeared; the completed-document count
  remained two. Lifecycle logs showed cancel at state recording, shutdown, and
  then main-window close at state off. This distinguishes Quit cancellation from
  an accidental finish caused by menu dismissal. Reopening the unchanged bundle
  and a brief prepare-only button press restored the global-shortcut listener
  without another microphone prompt or additional completed recording.

During this acceptance session, a temporary caffeinate -di -t 7200 process was
started at the user's request. macOS confirmed idle display/system sleep
assertions. It does not unlock the Mac or change its security configuration.

The user then physically held and released Control-Shift-Space while using an
external text field. The user reported the recording indicator and no pasted
text. Two additional live documents contained 1.92 and 3.456 seconds of microphone
audio and nonempty local recognition; the second reused the loaded model. The
GUI displayed Dictation copied · automatic paste unavailable and the helper's
event-posting permission error; the disposable target remained empty. This proves
physical shortcut capture/release and copy fallback, not successful paste. No
private transcript content is included in this validation record.

## Explicit paste setup correction

The shared CLI now offers fscript permissions to inspect event-posting access
without prompting, and only --request paste can ask macOS through
CGRequestPostEventAccess. The GUI uses this same bundled-helper path when Paste
automatically is turned on; saved preferences only inspect at startup. It reports
an absent effective grant immediately. Requests never reset existing permissions,
and a fresh preflight determines the returned state rather than assuming success.
Export --paste remains noninteractive. Apple documents the native request API at
https://developer.apple.com/documentation/coregraphics/cgrequestposteventaccess().

Local validation: all 188 Rust library tests passed, including exhaustive
inspection/request/grant-transition combinations. Nine single-command/export
process E2E checks passed, including JSON versus human permission status, offline
help on the request path, and inspection with broken model settings/missing media
tools. All 16 native process/property/differential checks passed with their real
CLI fixtures. Effective grant and targeted-paste acceptance remain pending.

GUI acceptance then confirmed that a saved Paste automatically preference only
inspected access at startup and showed the missing-grant message. Turning the app
preference off cleared the message; turning it on invoked the bundled request
helper. The macOS audit identified Fast Transcript as the responsible app and its
bundled fscript as the requester. The OS returned not granted without showing a
fresh prompt; the GUI retained the warning. No system permission was toggled.

## Compact recording indicator

The previous 390-by-64 indicator with red text is replaced by a 248-by-42 rounded
pill with white text. Recording uses a small red microphone; processing uses a
spinner. It remains nonactivating and ignores mouse clicks. An observed
close-to-finish bug left Listening visible because closing the window cleared
toggle intent before the processing event. Finish now changes an already-visible
indicator immediately, and the processing event preserves that visible state.

The release build compiled and a native GUI fixture check showed the compact
Listening indicator. With inference deliberately blocked, closing the main window
showed Transcribing and a spinner in both the accessibility tree and screenshot.
The fixture gate holds actual inference, so this checks the waiting state rather
than briefly sampling an already-completed transcription. Capture was explicitly
prerecorded; this check did not request microphone permission.

## Authorized permission refresh follow-up

The user explicitly approved one off/on refresh of Fast Transcript's existing
Device Control and Data Access entry and completed the macOS Touch ID prompt.
The entry was observed off, then restored on. The bundled permission check still
reported no effective grant, including after a full restart of the unchanged app.

A temporary, read-only native diagnostic was launched through the app's normal
custom-notes runner against a public speech fixture. Both AXIsProcessTrusted and
CGPreflightPostEventAccess returned false. The macOS audit attributed both queries
to the exact tested Fast Transcript bundle and explicitly rejected its saved
Accessibility code requirement: current cdhash e2d013ea… did not match saved
cdhash 2f36c293…. Thus the off/on change preserved the stale identity; neither a
restart nor accepting the visible enabled switch established an effective grant.
The isolated notes configuration was restored after the diagnostic, and no
external agent or microphone was used for it.

Removing that stale entry to register the current bundle was rejected by automatic
approval review because the user's confirmation covered off/on, not removal and
re-addition. The entry remains present and enabled. Explicit approval for that
separate operation is pending; no TCC reset or indirect workaround was attempted.
The validated app bundle is unchanged, and milestone 5 remains incomplete until
effective paste and the remaining acceptance checks pass.

The user subsequently explicitly approved remove-and-re-add. The exact tested
bundle was registered again and its entry was observed enabled. The actual bundled
helper's kTCCServicePostEvent preflight then returned authValue 2. This repaired
the identity mismatch without changing other applications' grants.

A later physical hold captured 7.5 seconds of public fixture speech and the app
reported that paste was sent. The original disposable target was then found to
lack the standard Edit menu/Command-V binding; even a normal automation paste
failed there. The corrected target accepts that positive control. A foreground-
verified replay still exposed a separate short-lived sender failure: API exit 0
did not establish receipt. Keeping the unchanged posting process alive for 500 ms
with stdout backpressure caused native insertion in both experiment orders.

The 100 ms post-chord correction was tested with normally exiting CLI processes
and receiver-owned Paste callbacks: three fixed cases inserted exact renderer
text; two interleaved old-binary controls exited 0 without a callback. Foreground
PID was unchanged across all five cases. See [native paste testing](native-paste-testing.md)
for the durable opt-in receiver check and [the reference comparison](reference-apps.md#paste-and-permission-follow-up-2026-11-27).

## Corrected packaged app and focus acceptance

The app built from dc10449 passed signature verification. After the user completed
macOS authentication for the authorized entry refresh, its app-owned acceptance
runner invoked the actual bundled CLI with normal supervisor/worker execution,
normal stdout, and normal process exit. All three cases produced exactly one
native Paste callback and exact expected insertion. The receiver's PID remained
the same before and after each delivery; the visible field contained the three
expected markers. No external agent or microphone was used in these checks.

The unmodified Command-Shift-D action then started latched dictation with the
explicit public WAV fixture. Finder opened Fast Transcript and then the native
receiver. The app's lifecycle log independently recorded deactivation with
toggle=true and state=recording, while its visible control remained Listening.
NSWorkspace identified the native receiver as the foreground application.
Finishing with Command-Shift-D saved the clip and reported Dictation copied;
the clipboard matched the shared compact renderer exactly (206 UTF-8 bytes).
This proves toggle persistence across an actual focus change, not physical hold
release or shortcut-origin paste routing. The final physical shortcut checks
below use the same unchanged bundle.

## Final physical shortcut acceptance

The user physically held Control-Shift-Space for two takes with the public speech
WAV still explicitly selected. The first take kept the native test field active;
the second switched to Fast Transcript's main window before release. These are
real Carbon shortcut gestures with prerecorded capture, not microphone speech or
app-targeted key simulations.

| Take | Saved audio | Observed result | Independent batch comparison |
| --- | --- | --- | --- |
| Original field remains active | 8.42 s | Exactly one new native Paste callback; the field exactly matches the saved transcript, ending in “em casa.” | Exact text, segments, duration, chunk settings, and chunk count |
| Switch to Fast Transcript before release | 9.62 s | The longer result appears in the main window, ending in “os 5.”; the original field retains only the first take, with no additional callback | Exact text, segments, duration, chunk settings, and chunk count |

Each comparison independently cropped the original WAV with FFmpeg, then ran the
real packaged CLI and cached ONNX model in batch mode. Both recording documents
reported load_seconds 0, confirming warm model reuse; this is not an inference
performance benchmark. The second take's different, longer text makes unchanged
receiver contents a meaningful focus-change assertion. The user subsequently
used Copy while reporting results, so a later clipboard snapshot is not treated
as proof of its earlier automatic contents. Automatic copy bytes were checked
independently during the toggle acceptance above; both paths use the same export
action.

The temporary notes runner was removed from the isolated test configuration, its
prior agent selection restored, and the prerecorded validation app and native
receiver were closed. No real-user model or agent preferences were changed.
