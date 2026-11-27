# Agent connection assessment

Research date: 2026-11-27. This records a recommendation, not an implemented
transport. The existing notes command remains unchanged. Finish the dictation
milestone before starting an agent transport migration.

## Recommendation

Use ACP as the common agent connection in the Rust backend, shared by the CLI and
desktop app. Preserve `fscript notes transcript.json` and the Create notes button.
ACP must not add an everyday command, repeated flag, or GUI-specific agent client.
Keep the generic stdin-to-Markdown command transport for other programs.

| Application | Verified approach | Applicable lesson |
| --- | --- | --- |
| [Zed](https://zed.dev/docs/ai/external-agents) | Common ACP UI, registry or command/args/env definitions; agent-owned credentials and configuration. | Share the client and expose capabilities reported by each agent. |
| [JetBrains AI Assistant](https://www.jetbrains.com/help/ai-assistant/acp.html) | ACP agents inside AI Chat, registry setup and custom acp.json definitions. | Make initial setup explicit and retain the selected agent for subsequent one-click use. |
| [Toad](https://github.com/batrachianai/toad) | Standalone terminal UI with agent definitions and a shared streaming ACP client. | ACP is useful beyond editors; retain our literal argv launching rather than copying shell-based launching. |

Toad's separation is visible in its [agent schema](https://github.com/batrachianai/toad/blob/main/src/toad/agent_schema.py)
and [client implementation](https://github.com/batrachianai/toad/blob/main/src/toad/acp/agent.py).

## Requested agents

| Agent | ACP route | Integration detail |
| --- | --- | --- |
| Codex | [codex-acp](https://github.com/agentclientprotocol/codex-acp) | Maintained adapter bridges Codex App Server; CODEX_PATH can select an installed executable. |
| Claude Code | [claude-agent-acp](https://github.com/agentclientprotocol/claude-agent-acp) | Maintained adapter uses Anthropic's Agent SDK; CLAUDE_CODE_EXECUTABLE can select an installed executable. |
| omp | [Native omp acp](https://github.com/can1357/oh-my-pi/blob/main/README.md) | Verified against actual upstream and the installed omp/18.20.2 help command. |

This is documentation/source verification, not successful authenticated ACP
summary execution. No agent session or credential refresh was performed for the
assessment. An adapter is still an installed dependency with its own version and
runtime requirements; ACP does not remove those requirements.

## Shared implementation boundary

The [official Rust SDK](https://agentclientprotocol.com/libraries/rust) can provide
the protocol client. Keep transcript normalization, summary instructions, atomic
output delivery, deadlines and owned-process cleanup in the existing notes job.
One client handles initialization, sessions, streamed text, errors and cancellation.
Agent definitions hold executable, literal argv, environment and restriction
settings. Persistent selection keeps the everyday interaction unchanged.

Target [stable ACP v1](https://agentclientprotocol.com/protocol/v1/overview).
[ACP v2 entered Preview on November 20, 2026](https://agentclientprotocol.com/announcements/acp-v2-preview)
and changes prompt completion semantics; submission acknowledgement must not be
treated as completed output. Reassess protocol maturity before implementation.

## Preserve the summary job's restrictions

The [ACP security specification](https://github.com/agentclientprotocol/agent-client-protocol/blob/main/docs/protocol/v1/security.mdx)
does not provide an agent sandbox. Refusing client permission requests does not
disable an agent's own tools or access. Preserve the current restricted summary
behavior with small provider-specific launch/session profiles and isolated job
context. Do not assume an empty client capability list enforces those restrictions.

In particular, the [Claude adapter](https://github.com/agentclientprotocol/claude-agent-acp/blob/main/src/v1/acp-agent.ts)
normally loads user/project/local configuration and tools; settings must be
explicitly constrained. [omp approval configuration](https://github.com/can1357/oh-my-pi/blob/main/docs/approval-mode.md#acp-sessions)
can bypass ACP permission gates. ACP generalizes the wire protocol, not every
provider's execution policy.

Unattended jobs must reject interactive questions and tool permission requests,
fail clearly on expired authentication, and cancel without orphaning children.
Credentials remain agent-owned; ACP cannot fix an expired OAuth login.

Validate fragmented Unicode, interleaved updates, malformed messages, deadlines,
cancellation, child cleanup, authentication failures, denied requests and atomic
output preservation. Compare normalized inputs and delivery semantics between
transports. Live summaries need factual acceptance checks rather than exact prose
equality. Prove restricted execution before replacing a working provider route.
