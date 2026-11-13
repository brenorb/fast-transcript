---
name: verify-fscript-git-hooks
description: Check Git hook registration and staged Rust formatting gates when changing the fscript hook installer or pre-commit command.
---

# Verify the formatting hook

The installer sets the repository's hook path. The pre-commit hook reads the staged Rust path list and skips formatting when the list is empty. A failed format check blocks the commit.

Use Java 11 or later with `TLA2TOOLS` pointing to [TLC 1.7.4](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4).

```sh
skill=.cursor/skills/verify-fscript-git-hooks
sh scripts/check-cli-state-machine.sh "$skill/Hook.tla"
sh "$skill/references/check-hook.sh"
```

## Coverage

`scripts/install-git-hooks.sh` and `.githooks/pre-commit` are identical at main `7562797c4462045c2a3edc2ceaf9595ed6a5d626` and tag `v1.1.3` at `751f294296a0034360231d6acfe9cf3c700cb0a8`.

| Transition | Source |
| --- | --- |
| Resolve the repository root and register the hook path | `scripts/install-git-hooks.sh`, `git config core.hooksPath` |
| Inspect staged file types and skip the formatter for a non-Rust change | `.githooks/pre-commit`, staged diff and empty-list guard |
| Run the formatter check and propagate failure | `.githooks/pre-commit`, `cargo fmt --check` |

The model covers one successful install and one hook execution with binary outcomes for Rust paths and the formatter. It abstracts Git errors, paths, actual formatting and concurrent staging. The fixture creates a temporary Git repository, runs the real installer and hook, and uses a fake formatter only to control exit status and count invocations.

TLC checked 14 states. `Hook-mutant.cfg` allows a failed format check to succeed. In a disposable checkout, change the failure branch's `exit 1` to `exit 0`. The fixture detects the false success. Restore and rerun both checks before updating the reviewed refs.
