---
name: verify-fscript-build-discovery
description: Verify Apple target gating, developer root enumeration, and compiler runtime discovery when editing build.rs.
---

# Verify compiler runtime discovery

The reviewed `build.rs` is identical at `7562797` and the cited `v1.1.3`. `main` skips runtime discovery for non-Apple targets. `developer_roots` combines the environment, a successful nonempty `xcode-select -p`, and two default paths. It sorts and deduplicates roots. `runtime_dirs_for_clang_base` skips unavailable directories and accepts a runtime path only when `libclang_rt.osx.a` is a file. `clang_runtime_dirs` sorts and deduplicates the accepted paths before emitting linker directives.

Use Java 11 or later and set `TLA2TOOLS` to the absolute path of the official [TLC 1.7.4 JAR](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4).

```sh
dir=.cursor/skills/verify-fscript-build-discovery
sh scripts/check-cli-state-machine.sh "$dir/Discovery.tla"
sh scripts/check-cli-state-machine.sh "$dir/Discovery.tla" "$dir/Discovery-mutant.cfg"
python3 "$dir/references/check-runtime-discovery.py"
```

The model bounds the search to two candidate paths and four root roles. Its checks cover the non-Apple bypass, sound reporting, and completeness after the search. The archive guard mutant must violate sound reporting. The source fixture compiles the real build script and runs it with two temporary candidates, one archive, and a local fake selector that returns the environment path. Removing the archive guard must add an unexpected directive. The fixture filters host linker directives to the temporary directory. Neither check compiles the application or tests linker compatibility.

Use pstack’s `/statechart` to inspect the shortest path for a failing rule or visualize the checked flow. When the source changes, update the reviewed revision after rechecking the model and source fixture.
