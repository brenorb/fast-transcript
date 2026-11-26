# fscriptctl

## Name

fscriptctl - noninteractive transcription controls and external-agent notes.

## Synopsis

```text
fscriptctl notes TRANSCRIPT [destination] [options]
fscriptctl agents list [--json]
fscriptctl agents set NAME
fscriptctl agents add NAME -- COMMAND [ARGS...]
fscriptctl agents remove NAME
fscriptctl agents reset
fscriptctl models list [--json]
fscriptctl models get [--json]
fscriptctl models set NAME
fscriptctl models reset
```

## Description

Notes use an installed Codex, Claude Code, omp, or saved custom command. The agent
receives transcript data on stdin and returns Markdown. Default selection is the
saved agent, otherwise the first available built-in in codex/claude/omp order.
Agent availability does not verify login. Sign in using the agent's own CLI;
Fast Transcript never starts an interactive login flow.

## Options

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

## Output

File input saves INPUT.notes.md and prints its absolute path. Piped input defaults
to stdout. A JSON transcript or complete JSONL stream retains source timestamps
and speakers; a provisional or failed stream is rejected. With --json, stdout is
one completion object instead of a path; the default Markdown file is still saved
unless --stdout or piped-input defaults select stdout. Failed generation leaves
an existing destination unchanged and cannot overwrite the source transcript.

## Agent settings

Use agents set NAME to save a built-in or custom agent, or agents set auto to clear
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

Models list uses the same catalog as fscript --list-models. Models set accepts
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

## Errors and cancellation

Success is 0; errors are nonzero. Diagnostics use stderr. Notes jobs share the
streaming CLI's process-group supervisor on macOS/Linux. Ctrl-C, SIGTERM, or SIGHUP
returns 130, stops owned helpers, and removes job scratch files. Timeouts fail
without replacing existing notes. The caller must provide a complete transcript;
literal JSON-like prose can be forced with --input-format text.
