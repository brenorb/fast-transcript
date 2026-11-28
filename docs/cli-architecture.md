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

## One command and everyday defaults

There is one executable: `fscript`. The common operation stays `fscript video.mp4`
or `fscript "https://example.com/video"`, with the saved/default model and normal
output delivery. Users do not need a subcommand or extra flags to transcribe.
The desktop invokes this same executable for every operation.

Distinct actions use subcommands; flags modify their behavior. This follows the
[clig.dev subcommand guidance](https://clig.dev/#subcommands), which recommends
combining closely related tools for discoverability and shared configuration.
Flags are also valid; this is a product choice, not a rule that clig.dev forbids
flag-based actions. We do not add a second flag spelling for each new action.

| Surface | Command | Default behavior |
| --- | --- | --- |
| Files and URLs | `fscript recording.m4a` | Save a transcript and print its path. |
| Incremental results | `fscript recording.m4a --stream` | Live text; add --json for versioned events. |
| External notes | `fscript notes TRANSCRIPT` | Save Markdown using the saved/first available agent. |
| Agent instructions | `fscript skill`, `fscript skill path` | Print bundled Markdown, or materialize it and print its path. |
| Models | `fscript models`, `fscript models set NAME` | Show catalog/default or save a preference without model loading. |
| Agents | `fscript agents`, `fscript agents set NAME` | Inspect available agents or save the preferred one. |
| Export | `fscript export TRANSCRIPT --format srt` | Render a completed document without inference. |
| Dictation | `fscript record` | Capture the default microphone; Ctrl-C finishes. `--duration` or `--control` supports agents. |
| Warm dictation shortcut | `fscript record --hotkey` | First available exclusive hold chord; explicit chord override supported. |
| Clipboard | `fscript export transcript.json --copy` | Copy the shared rendering; optional paste requires an explicit target PID. |
| Paste access | `fscript permissions` | Inspect without prompting. Only explicit `--request paste` asks macOS for access. |
| Meetings | `fscript meeting` | Live preview on stderr, final transcript path on stdout; Ctrl-C finishes capture and final recognition. Native acceptance passed on built-in devices. |
| Meeting control | `fscript meeting --control`, `meeting status DIR`, `meeting recover DIR`, `meeting transcribe DIR` | Explicit JSONL start/stop/cancel; durable inspection/recovery and shared transcription of retained audio. |
| Automatic meetings | `fscript meeting watch` | Foreground opt-in; shared call detector and existing capture; optional admission protocol. |
| Call inspection | `fscript calls`, `fscript calls --watch --json` | Passive activity and confirmed transitions without capture. |
| Calendar context | `fscript calendar`, `fscript calendar connect` | Passive local inventory; explicit permission and optional meeting titles. |

Action names are reserved in command position. Files named notes, models, agents,
export, skill, record, meeting, calls, calendar, or permissions use explicit paths such as `./notes`, or `fscript -- notes`. This is
an intentional breaking choice for this young app, avoiding a separate command
and filesystem-dependent guessing. Normal filenames and URLs remain unchanged.
Root --help discovers actions; `fscript notes --help` and `fscript help notes`
show nested help. One --manual and one --man-page cover the whole executable.
Existing model preference flags remain compatibility aliases to the same store;
the canonical new interface is fscript models. No fscriptctl wrapper is installed.

## Shared execution and output

The executable invokes the shared Rust services. The GUI launches it and
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
