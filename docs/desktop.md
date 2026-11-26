# Desktop application

Status: the desktop milestone is in development. The native process/protocol
client and SwiftUI build scaffold are implemented; the file/URL interface follows.
This is not yet a packaged, usable desktop release.

The macOS shell targets macOS 14 or later, using SwiftUI and Observation
([Apple API availability](https://developer.apple.com/documentation/observation/observable())).
Rust remains responsible for transcription, media selection, models, chunk
merging, transcript storage/rendering, and external notes. The native client
only launches the CLI, reads events, and presents state. Its child environment
retains inherited settings and adds standard user/Homebrew/system binary paths
when Finder or a test runner supplies a minimal PATH.

## Process and protocol contract

Each CLIProcess owns one invocation with literal argv, closed stdin, separate
stdout/stderr, and asynchronous pipe reads. Cancellation signals the CLI
supervisor, which owns model/media worker cleanup. One POSIX pipe read delivers
available bytes immediately; FileHandle.read(upToCount:) was found to wait for
more bytes in a blocked-producer E2E test and is deliberately not used.

The JSONL decoder accepts arbitrary byte boundaries, including split UTF-8.
It validates version, job identity, sequence, event type, required snapshots,
terminal state, and final process status. A completed event with a nonzero exit,
an incomplete final line, or EOF without completion cannot appear as success.
The complete protocol lives in [incremental-transcription.md](incremental-transcription.md).

## Development checks

Use Xcode's developer toolchain for XCTest. Command Line Tools can compile the
application but do not supply XCTest on the development machine. Set
DEVELOPER_DIR for a command when Xcode is installed but not selected; no global
xcode-select change is required.

```bash
cargo build --locked
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer python3 scripts/test_desktop_client.py
```

Python, FFmpeg, and ffprobe are required. The script invokes the real Rust CLI
and FFmpeg with a deterministic ASR worker, then runs seven Swift tests: literal
argv and diagnostics, cancellation, pre-EOF delivery, actual CLI-to-client
streaming versus batch parity, cancellation before inference finishes, invalid
protocol lifecycles, and 500 randomized Unicode fragmentation cases. No speech
model is downloaded or paid agent invoked. Running swift test directly runs the
core checks and skips the two integration cases unless their fixtures are supplied.
The macOS CI job runs the full script. Computer Use is reserved for actual GUI
buttons after the interface is implemented.
