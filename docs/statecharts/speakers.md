# Add speaker labels to an fscript transcript

[Mermaid chart](speakers.mmd) · [Verification skill](../../.cursor/skills/verify-fscript-diarization/SKILL.md)

![fscript speaker detection](speakers.svg)

pstack's `/statechart` checked eight states. The chart starts after transcription. The helper can return segments for speaker assignment, report no speech and preserve the unlabeled transcript, or fail the command with another error.

To regenerate, copy `Speakers.tla` and `Speakers.cfg` from the skill and `speakers.view.mjs` from this directory to a scratch directory. Use pstack at `5866df557357d005b3c0c9d3da0d9e739b4bdde3` and the same TLC JAR as the skill.

```sh
"$PSTACK_STATECHART" Speakers.tla --view speakers.view.mjs --out speakers
```
