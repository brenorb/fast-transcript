# External agent notes

Status: in development. Transcript input normalization is implemented first;
the `fscriptctl notes` CLI and process adapters follow in the same milestone.

The shared notes input layer accepts UTF-8 text, fscript JSON transcripts, completed
version 1 stream event objects, or complete version 1 JSONL streams. Timed JSON
segments retain original source timestamps and normalized speaker labels. JSONL
must contain one job, sequential events, and a successful completed result;
provisional, failed, mixed-job, and truncated streams cannot silently become notes.
Use explicit text input mode when literal transcript text begins with JSON syntax.

The fixed notes instructions ask for English Markdown with a summary, key points,
decisions, action items (task/owner/deadline), and open questions. Missing facts
must not be invented. Transcript content is JSON-quoted as data inside the prompt,
which will be passed through stdin rather than argv. No ASR model or embedded LLM
is involved in this layer.

The installed targets inspected on 2026-11-26 are Codex CLI 0.158.0 (`exec`), Claude
Code 2.1.309 (`--print`), and omp 18.20.2 (`--print`). Their local help and the
[Claude CLI reference](https://code.claude.com/docs/en/cli-reference) and
[omp CLI reference](https://github.com/can1357/oh-my-pi/blob/main/docs/cli-reference.md)
define the adapter interfaces. Agents keep ownership of their existing login.

Core checks compare plain/JSON/JSONL normalization and prompt quoting across 999
reproducible Unicode inputs, verify source timestamps/speakers, and reject broken
stream lifecycles before any agent is started.
