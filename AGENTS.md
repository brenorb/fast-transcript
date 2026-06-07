# Local AGENTS Notes

## External Binaries

- Don't conclude an integration binary is missing just because `which` fails.
- For this repo, also check the real runtime paths used by scripts and env vars such as `FSCRIPT_DIARIZATION_BINARY`.
- Before claiming diarization is unavailable, inspect the repo workflows that call `fscript`, especially site/batch scripts that may use an explicit `FluidAudio` binary path outside `PATH`.

## Homebrew

- When updating `brenorb/homebrew-tap`, run `brew style` on changed formulae before push.
- Homebrew formula metadata order matters: keep `version` before `license`.
- Prefer a checked-in pre-commit hook or equivalent guard for packaging repos when a CI-only failure reveals a mechanical rule.
