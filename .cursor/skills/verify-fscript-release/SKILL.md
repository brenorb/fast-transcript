---
name: verify-fscript-release
description: Check the fscript release matrix, publication gates, and crate rerun decision after editing release.yml.
---

# Verify the fscript release workflow

Review `.github/workflows/release.yml` at `7562797` and the cited `v1.1.3`. On each platform, build the locked Rust binary, package the archive and checksum, build the Python wheel, check wheel and archive CLI parity, then smoke-test installation before uploading. All three publish jobs depend on successful builds on both platforms and a tag ref. PyPI downloads the macOS wheel alone. At `v1.1.3`, the crates job attempted publication after authentication. Current source checks the registry's newest version before deciding whether to authenticate and publish.

Run the finite model from the repository root.

```sh
dir=.cursor/skills/verify-fscript-release
sh scripts/check-cli-state-machine.sh "$dir/Release.tla"
sh scripts/check-cli-state-machine.sh "$dir/Release.tla" "$dir/Release-mutant.cfg"
python3 "$dir/references/check-release.py"
```

The model advances each of two builds through six ordered milestones, with failure possible before each. It checks the tag and successful matrix gate, the selected wheel, and the skip path when the registry's newest version matches. The mutant must attempt a publish for a matching version. The fixture executes the actual guard shell with fake Cargo and curl and checks the workflow dependencies. Removing version equality must fail the matching case. If the registry has a different newest version or the lookup fails, the current guard proceeds to authentication and a publish attempt. The check compares the newest version alone.

The model stops at publication decisions. Provider outcomes and partial success across the three independent jobs are outside the checked properties.
