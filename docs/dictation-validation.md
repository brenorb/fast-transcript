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
The model timings above describe model loading only, not total capture-to-text
latency. No general model latency/quality claim is derived from one speech fixture.

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

- Exact current candidate: cancellation during inference, switching inference
  engines, and toggle behavior across actual app focus changes. Button/toggle,
  capture cancellation, preference reset, and active Quit checks passed above.
- Physical microphone transport with first-use/denied-device behavior distinguished
  from the deterministic fixture; no claim of microphone recognition yet.
- Automatic paste into a blank native test field, focus-change fallback, and actual
  effective permission behavior for the helper doing the paste.
- Latest selected global chord with exclusive collision detection/fallback.
- All CI green after the implementation/documentation commits are pushed.

A disposable native target lives in tests/fixtures/PasteTarget.swift. It creates
only an empty text window, opens no user documents, and saves no files; compile
and run it solely for GUI paste validation. It is not a second product command
and is never bundled with the desktop application or CLI wheel.
