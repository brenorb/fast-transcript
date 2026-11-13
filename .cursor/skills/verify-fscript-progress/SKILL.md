---
name: verify-fscript-progress
description: Check spinner eligibility, worker cleanup, and scoped quiet status when changing fscript progress and diagnostics.
---

# Verify progress reporting and quiet scope

Interactive terminals can show a background spinner. Redirected or dumb terminals use plain progress, and quiet mode suppresses status. Finish or drop stops and joins an active spinner. Entering a diagnostics scope saves the previous setting and restores it on drop.

Use Java 11 or later with `TLA2TOOLS` pointing to [TLC 1.7.4](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4).

```sh
skill=.cursor/skills/verify-fscript-progress
sh scripts/check-cli-state-machine.sh "$skill/Progress.tla"
sh scripts/check-cli-state-machine.sh "$skill/QuietScope.tla"
cargo test --locked --lib progress::tests
cargo test --locked --lib diagnostics::tests
```

## Coverage

Reviewed main `7562797c4462045c2a3edc2ceaf9595ed6a5d626` and tag `v1.1.3` at `751f294296a0034360231d6acfe9cf3c700cb0a8`. The tag selected animation by terminal presence. Current adds the dumb-terminal and quiet gates and scoped diagnostics.

| Transition | Source |
| --- | --- |
| Select animated or plain output and start the reporter | `src/progress.rs`, `animated_progress`, `ChunkProgressReporter::start` |
| Set the current chunk, stop and join the worker on finish or drop | `progress.rs`, `set_current_chunk`, `finish`, `Drop` |
| Save and restore thread-local verbosity | `src/diagnostics.rs`, `Scope::enter`, `Drop` |
| Spool quiet worker diagnostics and retain a bounded failure tail | `diagnostics.rs`, `capture_stderr`, `diagnostic_tail` |

The progress model bounds three chunks and one worker. It abstracts joining as a completed cleanup transition and omits rendering and scheduling details. The scope model bounds two nested scopes on one thread. The source tests cover separate-thread isolation and a bounded diagnostic tail.

TLC checked 64 progress states and seven scope states. The progress mutant starts animation on any terminal. The scope mutant leaves the prior setting unrestored. In a disposable checkout, return only `terminal` from `animated_progress` or remove the restore call from `Scope::drop`. The corresponding tests above detect both mutations. Restore, rerun the tests and update reviewed refs after changing the lifecycle.
