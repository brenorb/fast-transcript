# Save a transcript or write it to a pipeline

[Mermaid chart](delivery.mmd) · [Verification skill](../../.cursor/skills/verify-fscript-delivery/SKILL.md)

![fscript output to a file or pipeline](delivery.svg)

pstack's `/statechart` checked 28 states. The file route commits the transcript before reporting its location, so the file can be present even if completion reporting fails. The pipeline route treats an early close by the consumer as successful completion. The chart groups staging, writing, flushing and syncing into preparation and hides steps with no visible change.

To regenerate in a scratch directory, copy `Delivery.tla` and `Delivery.cfg` from the verification skill and copy `delivery.view.mjs` from this directory. Use pstack at `5866df557357d005b3c0c9d3da0d9e739b4bdde3` and the same pinned TLC JAR as the verification skill.

```sh
"$PSTACK_STATECHART" Delivery.tla --view delivery.view.mjs --out fscript-delivery
```
