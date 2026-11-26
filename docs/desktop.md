# Desktop application

Fast Transcript has a native macOS file/URL interface around the same Rust CLI.
Open a file, drop media, paste a link, or enter a path/URL and click Transcribe.
Text appears incrementally. Copy is one click; Export defaults to timestamped text,
with plain text, SRT, WebVTT, and JSON available from its menu. Partial selection
and model/agent choices stay outside the primary path.

The shell targets macOS 14 or later, using SwiftUI and Observation
([Apple API availability](https://developer.apple.com/documentation/observation/observable())).
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
checks both command versions, copies the matching fscript/fscriptctl executables,
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
| Model picker | `fscriptctl models list --json`, `fscriptctl models set NAME`. |
| Agent picker | `fscriptctl agents list --json`, `fscriptctl agents set NAME`. |
| Create notes | `fscriptctl notes RESULT.json --output notes.md --json`. |
| Export transcript | `fscriptctl export RESULT.json --format srt --output transcript.srt`. |
| Export notes | `fscriptctl export notes.md --format markdown --output exported.md`. |
| Copy | Read the CLI-provided text; scripts can pipe `fscriptctl export RESULT.json --format compact --stdout` to a clipboard tool. |
| Cancel/Quit during work | Send SIGTERM to the CLI supervisor; owned helpers are reaped. |

Model choice saves the same preference used by the CLI. A FSCRIPT_MODEL_URL
environment override still wins and is identified in the model menu. Notes use
the installed agent's own login and configured model provider. Configure custom
agent commands through fscriptctl; ordinary use requires only Create notes.

The app keeps one current job. Completed JSON transcripts and Markdown notes are
saved under ~/Library/Application Support/fast-transcript/desktop/UUID. Export
chooses another destination through the native Save dialog. The CLI creates these
files atomically. Cancel retains provisional text on screen for Copy, but does
not label it complete or allow a completed-document export. A transcript is only
successful after a completed event and zero process status.

FSCRIPT_APP_DATA_DIR overrides desktop output storage. FSCRIPT_APP_HELPERS points
to a directory containing both commands for development; normal bundles locate
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

Python, FFmpeg, and ffprobe are required. Nine Swift tests cover the native process
client, real CLI/batch parity, cancellation, native file-provider input, malformed
lifecycles, 500 randomized UTF-8 event fragmentation cases, and 500 file-path
transport cases. The real CLI and FFmpeg run with a deterministic ASR worker;
these checks download no model and call no paid agent. Running swift test directly
skips the three fixture-dependent integration cases unless their environment is
supplied. macOS CI runs the full script and verifies the built app signature and
bundled commands. See [desktop-validation.md](desktop-validation.md) for actual
GUI controls and local ONNX inference evidence.

Microphone dictation, live meeting capture, calendar/call detection, session
history/recovery, and stable release distribution remain later roadmap milestones.
