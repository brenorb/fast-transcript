---
name: verify-fscript-manpage
description: Check check-mode behavior and fenced input handling when changing the fscript offline manual generator.
---

# Verify man page generation and freshness checks

The generator renders Markdown as a man page. Normal mode writes the result. Check mode compares it to the checked-in page and returns 1 if the file is missing or stale.

Use Java 11 or later with `TLA2TOOLS` pointing to [TLC 1.7.4](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4).

```sh
skill=.cursor/skills/verify-fscript-manpage
sh scripts/check-cli-state-machine.sh "$skill/ManPage.tla"
python3 scripts/generate_man_page.py --check
sh "$skill/references/check-manpage.sh"
```

## Coverage

Reviewed main `7562797c4462045c2a3edc2ceaf9595ed6a5d626`. `scripts/generate_man_page.py` was added after the cited `v1.1.3` tag at `751f294296a0034360231d6acfe9cf3c700cb0a8`.

| Transition | Source in the generator |
| --- | --- |
| Parse check mode and locate the source and destination | `main` |
| Process a fence, literal line, heading, list or paragraph | `render`, `escape`, `inline` |
| Write the new file or compare without writing | `main` |

The model bounds one invocation. It abstracts the initial destination to missing, stale or current, and distinguishes fenced lines from normal markup. It checks destination freshness and absence of writes in check mode. It omits exact roff text, filesystem errors during comparison and failure partway through a normal write.

TLC checked `CheckIsReadOnly` and `CheckSuccessMeansCurrent` in 126 states. `ManPage-mutant.cfg` accepts stale output. In a disposable checkout, bypass the comparison in `main` and run the fixture. It fails with `Stale man page accepted`. The original fixture rejects stale output, verifies that the existing bytes survive check mode, then writes and checks a current man page. Rerun the fixture and the model before updating the reviewed refs.
