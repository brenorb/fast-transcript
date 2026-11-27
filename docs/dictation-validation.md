# Dictation validation

Roadmap milestone 5 remains in progress. The GUI implementation is present; real
microphone and automatic-paste acceptance checks are not yet proven complete.

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
- Fifteen native client checks include real-CLI repeated recordings versus an
  independent batch document, one worker start, broken/full-duplex pipes, and
  500 fragmented Unicode recording lifecycles. These are fixture-capture tests.

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
improvement. The latest candidate must still be checked with the selected chord.
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
loss. The current candidate must validate these controls.

Still required before marking milestone 5 complete:

- Exact current candidate: toggle behavior across independently verified app focus
  changes. Real SDK model switching and controlled inference cancellation passed
  above; real microphone checks remain separate.
- Physical microphone transport with first-use/denied-device behavior distinguished
  from the deterministic fixture; no claim of microphone recognition yet.
- Automatic paste into a blank native test field, focus-change fallback, and actual
  effective permission behavior for the helper doing the paste.
- Physical keyboard delivery for the latest selected global chord; native
  exclusive registration, automatic fallback, and release-on-shutdown passed.
- All CI green after the implementation/documentation commits are pushed.

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
