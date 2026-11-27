# Desktop application

Fast Transcript has a native macOS file/URL interface around the same Rust CLI.
Open a file, drop media, paste a link, or enter a path/URL and click Transcribe.
Text appears incrementally. Copy is one click; Export defaults to timestamped text,
with plain text, SRT, WebVTT, and JSON available from its menu. Partial selection
and model/agent choices stay outside the primary path.

The shell targets macOS 14.2 or later, using SwiftUI and Observation
([Apple API availability](https://developer.apple.com/documentation/observation/observable())).
The bundled capture transport also imports Core Audio tap APIs introduced in
14.2; the standalone native CLI has the same minimum.
Rust owns media selection, inference, merging, preferences, rendering, atomic
transcript delivery, and external notes. The Swift client launches the bundled
commands and presents their results. It contains no speech model or LLM client.

## Build and run

From a checkout with Rust, Swift, and macOS developer tools:

```bash
scripts/build_macos_app.sh
open 'target/desktop/Fast Transcript.app'
```

Use --debug for a faster development build or --output APP_PATH to choose another
bundle location. The builder stages a complete app beside its destination,
checks the command version, copies the matching fscript executable,
and applies an ad-hoc signature. It replaces only a prior Fast Transcript bundle.
This is a local development build; signing/notarization and release distribution
are a separate roadmap milestone.

Install FFmpeg/ffprobe and yt-dlp (or uv for the downloader fallback) as for the
CLI. SDK models also use the CLI's existing runtime setup. The native child
environment retains inherited settings and adds standard user/Homebrew/system
binary directories when Finder supplies a minimal PATH. Models are downloaded on
first transcription when needed, not during application startup or model selection.

## Shared operations

| GUI action | Noninteractive equivalent |
| --- | --- |
| Open/drop/paste/Transcribe | `fscript MEDIA --stream --json --output RESULT.json` |
| Last duration | Add `--last 5:00`. |
| Start/end | Add `--start 10:00 --end 15:00`. |
| Model picker | `fscript models --json`, `fscript models set NAME`. |
| Agent picker | `fscript agents --json`, `fscript agents set NAME`. |
| Create notes | `fscript notes RESULT.json --output notes.md --json`. |
| Export transcript | `fscript export RESULT.json --format srt --output transcript.srt`. |
| Export notes | `fscript export notes.md --format markdown --output exported.md`. |
| Copy | Read the CLI-provided text; scripts can pipe `fscript export RESULT.json --format compact --stdout` to a clipboard tool. |
| Cancel/Quit during work | Send SIGTERM to the CLI supervisor; owned helpers are reaped. |

Model choice saves the same preference used by the CLI. A FSCRIPT_MODEL_URL
environment override still wins and is identified in the model menu. Notes use
the installed agent's own login and configured model provider. Configure custom
agent commands through fscript; ordinary use requires only Create notes.

The app keeps one current job. Completed JSON transcripts and Markdown notes are
saved under ~/Library/Application Support/fast-transcript/desktop/UUID. Export
chooses another destination through the native Save dialog. The CLI creates these
files atomically. Cancel retains provisional text on screen for Copy, but does
not label it complete or allow a completed-document export. A transcript is only
successful after a completed event and zero process status.

FSCRIPT_APP_DATA_DIR overrides desktop output storage. FSCRIPT_APP_HELPERS points
to a directory containing fscript for development; normal bundles locate
Contents/Helpers. Existing CLI configuration/cache/runtime environment overrides
remain supported. Test settings must be isolated with FSCRIPT_CONFIG_FILE and
FSCRIPT_NOTES_CONFIG_FILE rather than changing the user's saved defaults.

## Process and protocol contract

Each CLIProcess owns one invocation with literal argv, closed stdin, separate
stdout/stderr, and asynchronous pipe reads. Cancellation signals the CLI
supervisor. One POSIX read delivers available bytes immediately; FileHandle's
read(upToCount:) waited for more bytes in the blocked-producer E2E check.

The JSONL decoder accepts arbitrary byte boundaries, including split UTF-8, and
checks version, job identity, sequence, event type, required snapshots, terminal
state, and process status. EOF alone cannot appear as success. The full protocol
is in [incremental-transcription.md](incremental-transcription.md).

Native drag transport accepts URL objects, URI strings, encoded ASCII URI bytes,
and plain text paths/links. File URLs become local paths before reaching the CLI.
Malformed URL bytes are rejected; Apple requires legal URL wire data
([URL initialization contract](https://developer.apple.com/documentation/foundation/url/init(datarepresentation:relativeto:isabsolute:))).

## Development checks

Use Xcode's developer toolchain for XCTest. Command Line Tools can build the app
but do not supply XCTest on the development machine. Setting DEVELOPER_DIR for a
command does not change the system's selected developer directory.

```bash
cargo build --locked
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer python3 scripts/test_desktop_client.py
```

Python, FFmpeg, and ffprobe are required. Sixteen native checks cover the process
client, real CLI/batch parity, cancellation, native file-provider input, malformed
lifecycles, 500 randomized UTF-8 event fragmentation cases, and 500 file-path
transport cases, and 500 hosting-window close/detachment cases. The window
regression creates only unshown test-process windows. The real CLI and FFmpeg run with a deterministic ASR worker;
these checks download no model and call no paid agent. Running swift test directly
skips the four fixture-dependent integration cases unless their environment is
supplied. macOS CI runs the full script and verifies the built app signature and
bundled command. See [desktop-validation.md](desktop-validation.md) for actual
GUI controls and local ONNX inference evidence.

Dictation is under acceptance validation. Live meeting capture, calendar/call
detection, session history/recovery, and stable distribution remain later milestones.

## Dictation (under validation)

Hold the microphone button to dictate; release to transcribe and copy the result.
First use prepares the model and requests microphone authorization if needed;
speech is captured only while the app says Listening. Releasing during preparation
warms the model without starting a later unattended recording. Subsequent clips
reuse that model until cancellation, a model change, file/URL transcription, or
Quit. File/URL jobs remain the primary default flow.

After preparation, hold the global chord shown next to the button from any app.
The shared CLI chooses an available exclusive shortcut. A compact dark pill near
the bottom of the screen shows white status text, a small red microphone during
capture, and a spinner during processing. It never takes focus or intercepts
mouse clicks.
The indicator switches to Transcribing immediately on finish, including when
closing a toggle recording's main window, and disappears on completion/cancel.
The caption and hold-button help use the chord actually selected by the CLI; when no chord is
available, the UI directs you to the hold button. The File
menu's Start/Finish dictation (Command-Shift-D) also supports a toggle interaction;
Accessibility activation or Return on the focused hold button does the same.
Toggle recording survives changing apps and keeps its recording indicator visible;
closing the main window finishes that toggle recording. A physical hold ends on
focus loss so it cannot lose its release event.

Closing the window keeps the app and its prepared global shortcut available.
Reopening the app restores the shared workspace and completed transcript. Quit
cancels active work and shuts down owned workers. A native observer watches the
hosting window only: closing a menu or another panel must not finish recording.
This uses a primary window group and an explicit reopen action; a primary
[single Window scene normally quits when closed](https://developer.apple.com/documentation/swiftui/window).

Paste automatically is off by default. Shortcut dictation can optionally paste
into the originally active app when it is still active, otherwise the result is
copied. The shared CLI checks actual event-posting permission; Settings appearing
enabled is not by itself proof that this build/process has that permission.
Turning on Paste automatically explicitly requests access through the bundled
`fscript permissions --request paste --json` action. Startup only inspects with
`fscript permissions --json` and never prompts for paste access. The GUI reports
an absent effective grant immediately, rather than waiting for a failed paste.
The request uses macOS's normal event-posting API and never resets an enabled
Settings entry. Quit owns and cancels the permission helper as well.
Failures preserve the transcript and attempt clipboard delivery. Copy/export/notes
use the same existing completed-transcript commands. Cancel stops the owned
recording/model process; Quit also waits for an idle warm helper to exit.
The shared macOS paste function stays alive for 100 ms after posting the complete
chord so a short-lived CLI cannot exit before queued events are delivered. Copy
alone has no such wait. A sent status describes posting events, not readback from
an arbitrary text field; [native acceptance](native-paste-testing.md) verifies real
Paste callbacks and exact inserted text in a disposable receiver.

FSCRIPT_APP_PREFERENCES_SUITE isolates the small set of presentation preferences
for local GUI testing. It does not change CLI model or agent settings. See
[dictation validation](dictation-validation.md) for evidence and remaining gaps.

## Build identity and macOS grants

For development builds that retain permission identity across recompilation, use
an existing signing certificate, for example:

```sh
scripts/build_macos_app.sh --sign-identity 'Apple Development: Your Name (TEAMID)'
```

FSCRIPT_SIGNING_IDENTITY supplies the same option; an explicit flag overrides it.
The same identity signs the bundled CLI and the app, with nested code signed first
and verification completed before replacing the prior app. The script does not
create certificates or change any macOS permission. Without an identity it uses
ad-hoc signing, whose designated requirement is tied to that exact build; an
existing enabled permission entry may then refer to a previous code hash.
Do not diagnose that mismatch as a disabled user setting. See [Apple's code
identity explanation](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements/).
