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
conflict with output paths. Paste requires macOS Accessibility permission and waits
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
