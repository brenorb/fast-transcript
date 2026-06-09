# Local AGENTS Notes

## External Binaries

- Don't conclude an integration binary is missing just because `which` fails.
- For this repo, also check the real runtime paths used by scripts and env vars such as `FSCRIPT_DIARIZATION_BINARY`.
- Before claiming diarization is unavailable, inspect the repo workflows that call `fscript`, especially site/batch scripts that may use an explicit `FluidAudio` binary path outside `PATH`.

## Homebrew

- When updating `brenorb/homebrew-tap`, run `brew style` on changed formulae before push.
- Homebrew formula metadata order matters: keep `version` before `license`.
- Prefer a checked-in pre-commit hook or equivalent guard for packaging repos when a CI-only failure reveals a mechanical rule.

## Local Workflow

- After cloning this repo, run `scripts/install-git-hooks.sh`.
- Keep the checked-in pre-commit hook active; it blocks commits that would fail `cargo fmt --check`.
- Before pushing Rust changes, either run `cargo fmt` yourself or let the hook stop the commit and fix it immediately.
