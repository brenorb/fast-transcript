# Prepare audio from a file or a pipe

[Mermaid chart](input.mmd) · [Verification skill](../../.cursor/skills/verify-fscript-input/SKILL.md)

![fscript media preparation](input.svg)

pstack's `/statechart` checked 34 states. Piped audio is saved to temporary storage for tools that need to seek. A terminal is rejected before reading, and an empty pipe fails before probing. Supported audio goes straight to the sample reader. Other formats first pass through conversion. The chart groups error paths and abstracts the details of model loading, transcription and delivery.

To regenerate, copy `Input.tla` and `Input.cfg` from the skill and `input.view.mjs` from this directory to a scratch directory. Use pstack at `5866df557357d005b3c0c9d3da0d9e739b4bdde3` and the same TLC JAR as the skill.

```sh
"$PSTACK_STATECHART" Input.tla --view input.view.mjs --out input
```
