# External agent notes

`fscriptctl notes meeting.txt` saves `meeting.notes.md` and prints its absolute
path. It uses the saved agent, otherwise the first installed agent in Codex,
Claude Code, omp order. Select once with `fscriptctl agents set claude`, or override
one run with `--agent omp`. Discovery never starts an agent or verifies login.
An unavailable saved agent fails instead of silently switching providers.

```bash
fscript meeting.m4a --json
fscriptctl notes meeting.json
fscript meeting.m4a --stream --json | fscriptctl notes -
fscriptctl notes meeting.json --agent codex --model MODEL --stdout --json
fscriptctl agents add local -- /absolute/path/to/agent --non-interactive
```

The [offline control manual](control-cli.md) documents all flags, settings,
environment overrides, output destinations, and exit behavior. Both release
archives and Python wheels include `fscript` and `fscriptctl`; `cargo install
fast-transcript` installs both. Existing `fscript MEDIA` behavior is preserved.

The shared notes input layer accepts UTF-8 text, fscript JSON transcripts, completed
version 1 stream event objects, or complete version 1 JSONL streams. Timed JSON
segments retain original source timestamps and normalized speaker labels. JSONL
must contain one job, sequential events, and a successful completed result;
provisional, failed, mixed-job, and truncated streams cannot silently become notes.
Use explicit text input mode when literal transcript text begins with JSON syntax.

The fixed notes instructions ask for English Markdown with a summary, key points,
decisions, action items (task/owner/deadline), and open questions. Missing facts
must not be invented. Transcript content is JSON-quoted as data inside the prompt,
which is passed through stdin rather than argv. No ASR model or embedded LLM
is involved in this layer.

One shared Rust job runner supplies stdin, captures diagnostics, enforces the
five-minute default timeout, and writes notes atomically after success. Ctrl-C,
SIGTERM, or SIGHUP returns 130 and removes owned processes and scratch files on
macOS/Linux. Failed, empty, or invalid UTF-8 responses leave existing notes intact;
an output destination cannot replace its source transcript. Custom commands use
literal argv without shell interpolation and run in an isolated temporary cwd.

Built-in adapters disable tools and project context using their provider's flags.
Codex uses ephemeral read-only execution and its final-message file; Claude uses
print/safe mode without tools or MCP; omp uses print mode without tools, extensions,
skills, rules, context files, or session persistence. Custom commands own their
behavior and must read stdin and write only Markdown to stdout. Agent logins and
model defaults remain under the provider CLI's control. `--model` overrides the
language model for built-ins; the JSON result's model field records that requested
override, or null when the provider chooses its default. These external agents
may send transcript content to their configured model provider; transcription
itself remains local.

The installed targets inspected on 2026-11-26 are Codex CLI 0.158.0 (`exec`), Claude
Code 2.1.309 (`--print`), and omp 18.20.2 (`--print`). Their local help and the
[Claude CLI reference](https://code.claude.com/docs/en/cli-reference) and
[omp CLI reference](https://github.com/can1357/oh-my-pi/blob/main/docs/cli-reference.md)
define the adapter interfaces. Agents keep ownership of their existing login.

## Validation

Core checks compare plain/JSON/JSONL normalization and prompt quoting across 999
reproducible Unicode inputs, verify source timestamps/speakers, and reject broken
stream lifecycles before any agent is started. Eleven process E2E tests use
deterministic executable fixtures to compare all three adapters, file/stdin and
JSON/JSONL paths, literal custom arguments, saved selection, failures, timeouts,
and descendant cleanup. They do not use paid models or authenticate in CI.
Packaging checks compare both bundled binaries' versions, help, and offline
manuals with their release counterparts and exercise both Python entrypoints.

Real process validation on Apple M5 Max arm64, 2026-11-26, used a four-line
synthetic planning transcript. Elapsed time includes CLI startup, normalization,
agent startup, provider inference/network time, and result capture, not ASR.

| Installed agent | Result | End-to-end elapsed |
| --- | --- | --- |
| Codex CLI 0.158.0 | Generated English sections with correct owners, deadlines, and source timestamps. | 9.186 s |
| omp 18.20.2 | Generated English sections with correct owners, deadlines, and source timestamps. | 36.277 s |
| Claude Code 2.1.309 | Accepted adapter flags, then rejected its expired OAuth session; real generation remains unverified pending login refresh. | Not measured |

Generated prose is model output, so fixtures validate process contracts and
normalization rather than claiming deterministic semantic equivalence between
live providers. No Claude login was changed by this implementation.
