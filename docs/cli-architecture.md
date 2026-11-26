# CLI architecture

Design reference: [Command Line Interface Guidelines](https://clig.dev/), checked
2026-11-26. This document records product decisions; it is not a compliance score.

The guide informs these contracts: use a parser library, put results on stdout
and diagnostics on stderr, fail with nonzero status, provide examples and useful
errors, preserve compatibility, keep unattended paths available, and make
interrupts work. See [basics](https://clig.dev/#the-basics),
[output](https://clig.dev/#output), [interactivity](https://clig.dev/#interactivity),
[subcommands](https://clig.dev/#subcommands),
[future-proofing](https://clig.dev/#future-proofing), and
[signals](https://clig.dev/#signals-and-control-characters).

## Entry points

`fscript <media-or-url>` remains the primary operation. It already accepts an
arbitrary filename, so adding catch-all subcommand routing would steal filenames
such as `record` or `models`. Preserve this contract. New transcription options,
including `--start`, `--end`, `--last`, and `--stream`, belong to this operation.

`fscriptctl` is the separate control entry point with explicit commands, shipped
with external agent notes. It is a second thin binary in the same Rust package,
using the same library. This avoids ambiguous dispatch and preserves the existing
CLI. Other command families below remain planned until their roadmap milestone.

| Surface | Planned command | Default behavior |
| --- | --- | --- |
| Files and URLs | `fscript recording.m4a` | Save a transcript and print its path. |
| Incremental file/URL results | `fscript recording.m4a --stream` | Shipped: live text; `--json` selects versioned events shared with GUI/agents. |
| Dictation | `fscriptctl record` | Microphone capture and live text until stopped. |
| Meetings | `fscriptctl record --source meeting` | Separate mic/system tracks and a durable session. |
| Session control | `fscriptctl sessions list`, `sessions stop ID`, `sessions show ID` | Explicit target; no interactive selection. |
| Models | `fscriptctl models list`, `models set NAME`, `models get`, `models reset` | Shipped: same catalog and saved preference as existing flags; `--json` identifies saved/environment/effective selection. |
| External notes | `fscriptctl notes TRANSCRIPT` | Shipped: save Markdown using the saved/first installed agent; `--agent` overrides, stdin defaults to stdout. |
| Agent preference | `fscriptctl agents list`, `agents set NAME`, `agents add NAME -- COMMAND` | Shipped: inspect or save literal headless commands; no login or model startup during discovery. |
| Completed export | `fscriptctl export TRANSCRIPT --format srt` | Shipped: render saved JSON/JSONL through the same output core without inference; stdin defaults to stdout. |
| Detection | `fscriptctl calls watch`, `calendar list` | Inspect native adapter evidence without recording. |

The current model flags remain supported as compatibility interfaces. Add nested
help to control commands as they ship, with no model loading or permission request
on help paths. Keep Clap as the parser. Do not turn unknown commands into files or
silently start recording on a typo.

## Shared execution and output

Both binaries invoke the same Rust services. The GUI launches those binaries and
consumes their versioned JSONL protocol; native capture/calendar helpers also
provide noninteractive commands. No GUI-specific transcription, chunking,
configuration, or external-agent implementation is allowed.

Existing stdout/file defaults stay unchanged. Streaming is explicit because
silently changing a completed document into event records would break consumers.
Machine events must contain a protocol version, event type, job identity, and
source timestamps. Flush each event. Keep diagnostics off that stream. Define
partial/final semantics and failure termination before implementing it.

Supply all required values in argv, files, or stdin. Transcription never prompts.
Reject two consumers of stdin before reading. External-agent adapters pass
transcript content on stdin and pass argv directly without shell interpolation.
Agent authentication belongs to the installed agent, and its unattended invocation
must not launch a login conversation.

## Partial-transcription contract

Selection is optional and validated before media processing. Times accept seconds
or clock notation; end is exclusive; last-duration selection conflicts with
start/end. Unknown duration blocks only operations that actually need it, such as
`--last`. Crop before inference, keep original source timestamps, and share one
range implementation across local media, captions, and downloader arguments.
Existing output names and explicit file replacement behavior remain documented.

## Robustness work by milestone

Streaming owns incremental decoding, bounded buffering, broken-pipe handling,
interrupt cleanup, and child-process termination. Recording owns durable audio
and finalization after Stop. Session control owns targeted stop/status commands.
Calendar/call detection owns deduplication and permission-state reporting. Do not
claim these contracts are implemented until the corresponding roadmap row is
complete and its process-level tests pass.
