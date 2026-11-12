---
name: verify-fscript-cache
description: Check fscript ONNX bundle selection, download and model-directory installation when changing model cache or extraction code.
---

# Verify ONNX model cache installation

Installing a selected model in its own directory must preserve the existing default model. Otherwise selecting a new model can destroy another installed model.

Run with Java 11 or later and `TLA2TOOLS` pointing to the official [TLC 1.7.4 JAR](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4).

```sh
sh scripts/check-cli-state-machine.sh .cursor/skills/verify-fscript-cache/Cache.tla
cargo test --locked --lib model::tests
```

## Coverage

Reviewed at `7562797c4462045c2a3edc2ceaf9595ed6a5d626` and `v1.1.3` at `751f294296a0034360231d6acfe9cf3c700cb0a8`.

| Transition | Source in `src/model.rs` |
| --- | --- |
| Choose selected cache paths, reuse a complete model, otherwise reuse or fetch a bundle | `ModelConfig`, `model_cache_paths` and `ensure_model_dir` |
| Download to a partial path, then rename into the package cache | `download_model_package` |
| Unpack, remove staging artifacts, move or copy the required files and verify the selected directory | `extract_model_package`, `install_required_model_files`, `has_required_model_files`, `ensure_model_dir` |

The model has one install, a selected directory distinct from the existing default directory, and one bundle with the standard top-level layout. It groups byte-level archive and I/O failures, including successful unpacking that leaves the target incomplete. It treats the staged install and final completeness check separately. Concurrent writers and caller-supplied overlapping paths are outside the model.

TLC checked `DefaultDirectoryPreserved` in 13 states. `Cache-legacy.cfg` models the cited release unpacking into the shared destination parent. It fails because the standard archive directory is also the existing default model directory. Use pstack's `/statechart` to read the path.

In a disposable current checkout, replace only `extract_model_package` with that function from `git show v1.1.3:src/model.rs`. Run `cargo test --locked --lib model::tests::ensuring_selected_bundle_preserves_other_model_directories`. The fixture found the existing default files missing after the old extraction and cleanup. The current source passed all five cache tests after restoring the function. Recheck before advancing the reviewed commit.
