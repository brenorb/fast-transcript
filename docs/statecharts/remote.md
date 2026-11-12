# Transcribe a URL

[Mermaid chart](remote.mmd) · [Verification skill](../../.cursor/skills/verify-fscript-remote/SKILL.md)

![fscript transcription from a URL](remote.svg)

pstack's `/statechart` checked all 68 reachable states of the remote-routing model. The chart follows one URL through the manual-caption shortcut, the fallback to downloaded audio, or the local route requested by `--local` or an enabled speaker-label backend. It groups missing and failed captions into one fallback and hides stuttering after the command exits.

To regenerate in a scratch directory, copy `Remote.tla` and `Remote.cfg` from the verification skill and copy `remote.view.mjs` from this directory. Use pstack at `5866df557357d005b3c0c9d3da0d9e739b4bdde3` and the same pinned TLC JAR as the verification skill.

```sh
"$PSTACK_STATECHART" Remote.tla --view remote.view.mjs --out fscript-remote
```
