# Local AGENTS Notes

## External Binaries

- Don't conclude an integration binary is missing just because `which` fails.
- For this repo, also check the real runtime paths used by scripts and env vars such as `FSCRIPT_DIARIZATION_BINARY`.
- Before claiming diarization is unavailable, inspect the repo workflows that call `fscript`, especially site/batch scripts that may use an explicit `FluidAudio` binary path outside `PATH`.
