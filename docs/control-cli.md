# fscript actions

## Additional actions

These subcommands are part of the same fscript executable as file/URL transcription.


```text
fscript notes TRANSCRIPT [destination] [options]
fscript export TRANSCRIPT [destination] [--format FORMAT] [options]
fscript agents [--json]
fscript agents set NAME
fscript agents add NAME -- COMMAND [ARGS...]
fscript agents remove NAME
fscript agents reset
fscript models [--json]
fscript models get [--json]
fscript models set NAME
fscript models reset
fscript skill
fscript skill path
fscript record
fscript record --duration 10
fscript record --control --json
fscript record --hotkey
fscript meeting
fscript meeting --duration 60 --json
fscript meeting --control
fscript meeting status SESSION [--json]
fscript meeting recover SESSION [--json]
fscript permissions [--json]
fscript permissions --request paste [--json]
```

## Action defaults

Notes use an installed Codex, Claude Code, omp, or saved custom command. The agent
receives transcript data on stdin and returns Markdown. Default selection is the
saved agent, otherwise the first available built-in in codex/claude/omp order.
Agent availability does not verify login. Sign in using the agent's own CLI;
Fast Transcript never starts an interactive login flow.

## Action options

- `--agent NAME`: override the selected note agent; auto uses automatic detection.
- `--model NAME`: override a built-in agent's language model. Custom commands own their model arguments.
- `--input-format auto|text|json|jsonl`: transcript format; defaults to automatic detection.
- `-o, --output PATH`: notes file or directory, instead of the optional destination operand.
- `--stdout`: print notes without saving a file. A destination of - is equivalent.
- `--timeout SECONDS`: positive agent execution timeout; defaults to 300 seconds.
- `--json`: print completion metadata including the Markdown notes and optional saved path.
- `-q, --quiet`: suppress status; keep results and errors.
- `-h, --help`: command help and examples. Nested commands support help.
- `-V, --version`: installed control CLI version.
- `--manual`: complete offline control manual.
- `--man-page`: export this manual in roff format.

## Action output

File input saves INPUT.notes.md and prints its absolute path. Piped input defaults
to stdout. A JSON transcript or complete JSONL stream retains source timestamps
and speakers; a provisional or failed stream is rejected. With --json, stdout is
one completion object instead of a path; the default Markdown file is still saved
unless --stdout or piped-input defaults select stdout. Failed generation leaves
an existing destination unchanged and cannot overwrite the source transcript.

## Agent skill

Run fscript skill to print the bundled SKILL.md as Markdown. It needs no model,
login, network, or configuration and does not install anything. For tools that
need a file path, fscript skill path materializes the same bundled content in
the application configuration directory under skills/fast-transcript/SKILL.md.
FSCRIPT_SKILL_DIR overrides that cache directory. With --json, skill returns
version, name, content, and an optional path. This follows Say It's skill/path
convention while keeping bare skill useful with no additional arguments.

## Paste permissions

`fscript permissions` inspects automatic-paste access for the running process.
It never prompts, opens Settings, posts keys, or starts capture/inference.
With --json, it returns version 1 and paste.supported/paste.granted booleans;
inspection succeeds even when access is absent or the platform is unsupported.
Only the explicit --request paste action asks macOS for event-posting access and
may show its permission prompt. The response reports the effective grant after
the request; requesting access does not imply success. Unsupported requests fail.
No permission is reset, disabled, or automatically granted. Grants depend on the
responsible app/terminal and code identity, so inspect/request from the same
execution path that will deliver text. An enabled Settings entry for an older
ad-hoc build may not authorize the current build. Export --paste stays
noninteractive and fails clearly if access is absent; --copy remains available.

## Microphone dictation

Run fscript record to capture the default macOS microphone and print the finished
plain transcript. Ctrl-C finishes capture and transcribes; a second Ctrl-C cancels.
SIGTERM/SIGHUP cancel immediately with status 130. For unattended use, supply
--duration SECONDS, or --control for explicit JSONL start/stop/cancel/shutdown
requests on stdin. Control mode keeps the loaded model across recordings.
--json selects version 1 lifecycle events; control mode always emits those events.
--output-dir DIR saves completed JSON documents in unique child directories.
Microphone audio is temporary and is removed on completion or cancellation.
Model/device overrides reuse the file-transcription configuration. Help never
opens the microphone or loads a model. See docs/recording.md for the protocol,
permissions, platform coverage, and validation.

With --hotkey, record stays warm and captures while the selected global shortcut
is held. Auto chooses the first available exclusive registration: Control-Shift-
Space, Control-Shift-D, then Control-Shift-Period. The ready event reports
shortcut_label and shortcut_available. Supply --hotkey ctrl+shift+d (or the other
listed chords) to choose explicitly. Shortcut mode may be combined with control;
a failed registration then leaves control recording usable and reports
shortcut_error. Without control, registration failure exits nonzero. Ctrl-C
cancels shortcut mode; it does not finalize an ongoing clip.

## Agent settings

Use fscript agents set NAME to save a built-in or custom agent, or agents set auto to clear
the preference. Agents add stores a literal program/argument array; no shell is
used. Custom commands read the prompt on stdin and write only Markdown to stdout.
They run in an isolated temporary working directory, so use absolute argument
paths. Agents remove deletes a custom profile and clears its default if selected.
Agents reset removes only the note-agent settings, leaving ASR models and agent
logins untouched. Put global --json before the custom command's -- separator.

Settings live in agents.json in the same platform configuration directory as the
ASR preference. FSCRIPT_NOTES_CONFIG_FILE overrides that filename. FSCRIPT_AGENT_PATH
replaces executable search paths; otherwise PATH and standard user/Homebrew/nvm
locations are searched. FSCRIPT_CODEX_BINARY, FSCRIPT_CLAUDE_BINARY, and
FSCRIPT_OMP_BINARY override built-in executables.

## Models

Models uses the same catalog as fscript --list-models. Models set accepts
the same names and aliases as fscript --set-default-model and saves to the same
configuration file. Models reset clears only the saved preference and can recover
malformed settings. These commands never download a model or start inference.

Models get prints the effective model: FSCRIPT_MODEL_URL, otherwise the saved
preference, otherwise parakeet-v3-int8. This differs from the compatibility flag
fscript --get-default-model, which reports the saved or built-in choice only.
With --json, each models command returns version 1, saved_default (or null),
environment_override (or null), effective_model, and the shared models array.
Each catalog entry includes name, repository, url, revision, runtime, and advice.
Explicit per-transcription --model still takes precedence over these defaults.

## Transcript export

Export reads a full fscript JSON transcript, a completed event, or a complete
version 1 JSONL stream from a file or stdin. It uses the same Rust renderer as
transcription and never loads a model or decodes media. Plain text input cannot
recover segment timing and is rejected by transcript formats. Incomplete or
failed streams are rejected.

The default format is text (timestamped); --format also accepts plain, compact,
speakers (timestamped), srt, vtt, and json. Repetition cleanup follows the usual
transcription default. File input saves a sibling with the selected extension and
prints its absolute path. Stdin defaults to stdout; --stdout or destination - also
prints the content. A destination file/directory or -o overrides the default.
Existing destinations are replaced atomically after successful rendering; source
transcripts cannot be overwritten. With --json, stdout is version 1 completion
metadata with content and output_path. To obtain a transcript JSON document,
use --format json --stdout; --json itself selects the metadata envelope.
Content is the renderer's string; file/stdout delivery adds a trailing newline.

Export validation compares all seven formats with batch output or saved JSON,
compares document/completed-event/JSONL input, and checks failed writes preserve
existing files. A 500-case Unicode property test checks that serializing and
reloading transcripts preserves all seven renderings.
JSON float parsing preserves round-trip timestamp precision.

Use --format markdown to export existing UTF-8 notes without calling an agent or
parsing transcript JSON. It preserves the notes, including existing final blank
lines, and adds a final newline when absent. Its default destination is
INPUT.export.md. The same atomic writer, source-overwrite guard, stdout, and JSON
metadata options apply. Empty Markdown is rejected.

## Clipboard delivery

Export --copy copies the chosen rendering instead of saving another export file.
The rendered content remains on stdout (or in the --json completion envelope),
and output_path is null. --paste additionally sends Command-V to the positive
--target-pid process ID; specifying that explicit target is required. Both flags
conflict with output paths. Paste preflights macOS event-posting access for the process performing delivery
(CGPreflightPostEventAccess) and waits
up to one second for held modifiers to be released. It reports key delivery,
not an acknowledgement that the receiving application accepted the paste.

Clipboard delivery uses /usr/bin/pbcopy on macOS. FSCRIPT_CLIPBOARD_BINARY may
point to a literal stdin-to-clipboard executable (also on other platforms),
without shell evaluation. Failure returns nonzero and never modifies the source
transcript. The existing transcript remains available if copying/pasting fails.
Clipboard export jobs use the shared cancellation supervisor. The desktop checks
that the original app is still active before requesting automatic paste; CLI
callers supplying target-pid own that focus decision. Default automatic paste is
off. These APIs are also usable noninteractively by agents.

## Errors and cancellation

Success is 0; errors are nonzero. Diagnostics use stderr. Notes jobs share the
streaming CLI's process-group supervisor on macOS/Linux. Ctrl-C, SIGTERM, or SIGHUP
returns 130, stops owned helpers, and removes job scratch files. Timeouts fail
without replacing existing notes. The caller must provide a complete transcript;
literal JSON-like prose can be forced with --input-format text.

## Live meeting sessions

`fscript meeting` records the default microphone and system output into separate,
private tracks under a new child of ./meetings and shows channel-attributed live
preview on stderr. On success, stdout prints only the absolute completed transcript
path. --quiet hides preview/status without removing this path. It uses the shared
saved model. The capture service requires macOS 14.2+
and OS microphone/system-audio access. Native delivery and the meeting GUI remain
under validation; see docs/live-meetings.md for the implemented protocol and limits.

--duration SECONDS stops capture without a terminal. --output-dir DIR chooses the
parent session directory; each recording creates a new child and never overwrites
an existing session. --model, --model-dir, and --device use the media CLI's shared
selection. --json emits flushed version 1 session events. Ctrl-C finishes ordinary
capture and drains pending recognition; a second interrupt cancels. In --control
mode, Ctrl-C cancels immediately.

--control waits for one newline-terminated JSON start request on stdin. Send stop
to stop capture and finish pending transcription, or cancel/shutdown to exit
without a final transcript. Closing stdin before stop interrupts the session;
closing it after stop permits normal completion. Requests use the same action
names as dictation, but one meeting process handles exactly one session. A start
request during an active session is an error. Each line is limited to 64 KiB.

Audio is written/checkpointed independently of model loading and inference.
Recognition failure reports recognition_error and keeps capture running until
stop/duration; saved confirms that audio is durable, not that recognition finished.
Live snapshots use transcript.partial.json with short context windows. Final
recognition uses 30-second windows with 2-second overlap on the same loaded model,
including windows processed during capture. It may revise earlier preview words.
Only successful final recognition publishes transcript.json and a completed event,
using the existing document
schema so export and notes need no meeting-specific rendering logic. Microphone
and System are source channels, not inferred identities of meeting participants.

meeting status SESSION inspects the last durable manifest without loading a model
or opening devices. meeting recover SESSION retains every complete saved sample
from an interrupted, unowned recording, marks it interrupted, and refuses a live
owner. Recovery does not resume capture or claim a complete transcript.

Completed meeting documents include optional preview_transcribe_seconds, which is
already included in transcribe_seconds but is not represented by the final chunk
list. Final chunk times plus preview time equal total inference time on both
tracks. Backend-reported model load is counted once. Engine calls include their
runtime
audio-file/IPC work; capture waiting, capture-side normalization/storage, and
external notes are excluded. The completed document replaces preview text rather than extending its prefix.
