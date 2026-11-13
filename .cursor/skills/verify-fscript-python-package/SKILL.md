---
name: verify-fscript-python-package
description: Check executable staging, launcher discovery, and child exit status when changing fscript wheel packaging or its Python entrypoints.
---

# Verify wheel staging and the Python launcher

The wheel builder checks the native executable's reported version before copying it into the package. The wrapper locates that executable, makes it user-executable if needed, and returns the native process's exit code.

Run with Java 11 or later and `TLA2TOOLS` pointing to the official [TLC 1.7.4 JAR](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4).

```sh
skill=.cursor/skills/verify-fscript-python-package
sh scripts/check-cli-state-machine.sh "$skill/Wrapper.tla"
sh scripts/check-cli-state-machine.sh "$skill/Wheel.tla"
python3 -m unittest discover -s tests -p test_pypi_wrapper.py
python3 "$skill/references/check-wheel-version.py"
```

## Coverage

Reviewed at `7562797c4462045c2a3edc2ceaf9595ed6a5d626` and tag `v1.1.3` at `751f294296a0034360231d6acfe9cf3c700cb0a8`. The Python launcher, build script and setuptools entrypoint are identical across the two refs.

| Transition | Source |
| --- | --- |
| Start the wrapper through the console script or `python -m fscript` | `setup.py`, `python/fscript/__main__.py` |
| Locate the bundled binary or report it missing | `python/fscript/_cli.py`, `bundled_binary_name`, `bundled_binary_path`, `main` |
| Make the binary executable, pass through arguments, and forward the exit code | `python/fscript/_cli.py`, `ensure_executable`, `main` |
| Resolve `--binary`, verify its version, and reject missing or invalid binaries | `scripts/build_pypi_wheel.py`, `parse_args`, `cargo_version`, `binary_version`, `main` |
| Stage the package and native binary, invoke the wheel build, and return its outcome | `scripts/build_pypi_wheel.py`, `copy_stage_tree`, `main`; `setup.py`, `bdist_wheel` |

`Wrapper.tla` bounds one invocation and distinguishes missing binary, executable bit, startup exceptions, and zero or nonzero child status. `Wheel.tla` bounds one build and distinguishes file presence, version, whether the basename matches the launcher, and success or failure of staging and building. The models abstract exact file modes, exit codes and package metadata.

TLC checked the wrapper in 13 states and the version gate in 24 states. Supply `Wrapper-mutant.cfg` or `Wheel-mutant.cfg` as the second checker argument to see a nonzero child exit turned into success or a mismatched binary reach staging. In a disposable checkout, returning `0` in `_cli.main` fails `test_pypi_wrapper.py`, and bypassing the comparison in `build_pypi_wheel.main` fails `check-wheel-version.py`.

## Existing basename mismatch

The builder accepts a matching binary under any filename and stages that filename verbatim. The wrapper expects `fscript` or `fscript.exe`, depending on the platform. `Wheel-name.cfg` shows a matching binary with another name passing the check, reaching staging and completing the build step without the expected launcher. `check-wheel-name.py` runs the actual build script's validation and staging, intercepts the wheel build subprocess, then runs the actual wrapper from the staged package. The wrapper reports that its binary is missing and exits 1.

```sh
sh scripts/check-cli-state-machine.sh "$skill/Wheel.tla" "$skill/Wheel-name.cfg"
python3 "$skill/references/check-wheel-name.py"
```

The checker exits with an invariant violation for the known basename mismatch. Use pstack's `/statechart` with the same config to regenerate the counterexample. On updates, rerun the model and source checks against the revised launcher and builder, then update the source refs and expected outcomes.
