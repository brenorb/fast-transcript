---
name: verify-fscript-settings
description: Check persistent model defaults and explicit or environment overrides when changing fscript settings, command routing, or model selection.
---

# Verify saved model selection

With no explicit model flag, `FSCRIPT_MODEL_URL` takes precedence over saved preferences, even if the saved JSON is malformed. Otherwise a user relying on the environment override can get a different model or an unrelated settings error.

Run with Java 11 or later and `TLA2TOOLS` pointing to the official [TLC 1.7.4 JAR](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4).

```sh
sh scripts/check-cli-state-machine.sh .cursor/skills/verify-fscript-settings/Settings.tla
cargo test --locked --lib settings::tests
cargo build --locked --bin fscript
python3 .cursor/skills/verify-fscript-settings/references/check-model-precedence.py target/debug/fscript
```

## Coverage

Reviewed at `7562797c4462045c2a3edc2ceaf9595d626`. The Homebrew tag `v1.1.3` at `751f294296a0034360231d6acfe9cf3c700cb0a8` predates the saved preference commands.

| Transition | Source |
| --- | --- |
| Recognize standalone set, get and reset commands, and reject mixed arguments | `src/settings.rs`, `parse_settings_command` and `run_settings_command` |
| Read a saved canonical preference, stage and persist it, or remove it | `DefaultModelStore.load`, `save`, `reset` |
| Prefer explicit flags, then environment, then the saved preference, then the built-in model | `src/cli.rs`, `parse_args_with_diarization_status_and_default`; `src/settings.rs`, `default_model_selection`; `src/model.rs`, `ModelConfig` |

The model bounds two representative preset values and absent or malformed settings. It permits sequential preference commands before selecting one transcription model. It abstracts alias parsing, atomic filesystem errors, simultaneous writers and actual model loading. The get command reads the saved preference even when transcription would use an override.

TLC checked `EnvironmentBeatsSaved` in 104 states. Supply `Settings-mutant.cfg` as the second argument to see saved settings shadow the environment. Use pstack's `/statechart` to interpret the trace. In a disposable checkout, move the call to `DefaultModelStore::configured().load()?` ahead of the environment check, build the binary and rerun the fixture. It catches a malformed preference blocking the override. Restore the order and rerun before advancing the reviewed commit.
