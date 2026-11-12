---
name: verify-fscript-delivery
description: Model-check fscript transcript replacement and stdout delivery when changing output paths, staging, file commit, or broken-pipe handling.
---

# Verify fscript transcript delivery

An existing transcript must remain intact until the replacement is ready to commit. An early truncation can destroy the only saved copy when the new write fails.

Run with Java 11 or later and `TLA2TOOLS` set to the absolute path of the official [TLA+ 1.7.4 release JAR](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4).

```sh
sh scripts/check-cli-state-machine.sh .cursor/skills/verify-fscript-delivery/Delivery.tla
```

## Coverage

Reviewed at `7562797c4462045c2a3edc2ceaf9595ed6a5d626` and the Homebrew release `v1.1.3` at `751f294296a0034360231d6acfe9cf3c700cb0a8`.

| Model | Source |
| --- | --- |
| Resolve the destination, check permissions, and stage a write beside the destination. | `src/delivery.rs`, `resolve_output_symlink` and `replace_output_file` |
| Flush and sync the staged transcript, preserve permissions, rename, and resolve the saved path. | `replace_output_file`, from temporary file creation through `persist` and `canonicalize` |
| Report a file's path and status, or write the transcript directly to stdout. | `OutputDestination::write`, `emit_file_output_completion`, and `write_stdout` |
| The packaged release writes directly to the resolved path. | `src/output.rs` in `v1.1.3`, `write_output_file` and `emit_file_output_completion` |

The model bounds one destination, a missing or existing transcript, and one delivery attempt. It represents file content symbolically and assumes a same-directory rename commits atomically. It distinguishes an output error after file commit from an error before commit. It groups symlink, path, and permission checks into destination preparation. Concurrent path replacement, crash durability, and transcript rendering are outside these bounds.

The baseline exhausted 28 states. The negative configurations reach a partially written destination before commit. The current source fixture caught the equivalent early truncation when `.truncate(true)` was added to the writability check in a disposable build. All 23 delivery tests passed after restoration.

## Verify a change

1. Compare the covered code with the reviewed commit and check which filesystem operation first changes the destination.
2. Run the checker. Supply `.cursor/skills/verify-fscript-delivery/Delivery-broken.cfg` or `Delivery-legacy.cfg` from the same directory as the second argument. Both must fail `OriginalPathChangesOnlyAtCommit`. Interpret counterexamples with pstack's `/statechart`.
3. Run `cargo test --locked --lib 'delivery::'`. To reproduce the source mutation in a disposable checkout, add `.truncate(true)` to the writability probe in `replace_output_file` and run `cargo test --locked --lib failed_staging_preserves_existing_transcript_and_cleans_up`. The fixture must report loss of the previous transcript. Restore the probe and rerun the delivery tests.
4. Advance the reviewed commit once the model matches the code.
