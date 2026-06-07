# Local Agent Notes

- When updating `brenorb/homebrew-tap`, run `brew style` on changed formulae before push.
- Homebrew formula metadata order matters: keep `version` before `license`.
- Prefer a checked-in pre-commit hook or equivalent guard for packaging repos when a CI-only failure reveals a mechanical rule.
