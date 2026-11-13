# fscript CLI state machines

Reviewed main at `7562797c4462045c2a3edc2ceaf9595ed6a5d626` and Homebrew's `v1.1.3` at `751f294296a0034360231d6acfe9cf3c700cb0a8`. The public Rust binary, Python console script and `python -m fscript` reach the same application. The tap's release checks are indexed in [homebrew-tap](https://github.com/brenorb/homebrew-tap/blob/codex/cli-state-machines/docs/state-machines.md).

| Subsystem | Commands and source | Model and charts |
| --- | --- | --- |
| Arguments and dispatch | `src/main.rs`, `src/lib.rs`, `src/app.rs`, `src/cli.rs`. Transcribe media, no-argument introduction, `help`, `help manual`, `--help`, `--version`, `--manual`, `--man-page`, `--list-models` and settings dispatch. | Main uses Clap after normalizing optional values. The cited release uses a manual scanner. [Arguments](../.cursor/skills/verify-fscript-cli-arguments/SKILL.md) |
| Local media and stdin | `src/input.rs`, `src/audio.rs` and `src/app.rs`. Open a path or spool stdin, probe and normalize, clean up temporary media. | Main recognizes stdin and gives helper programs absolute paths. [Input](../.cursor/skills/verify-fscript-input/SKILL.md) · [Piped media chart](statecharts/input.md) |
| Remote media | `src/app.rs` and `src/remote.rs`. Read metadata, try manual subtitles when permitted, or download audio and transcribe. | [Remote input](../.cursor/skills/verify-fscript-remote/SKILL.md) · [Remote media chart](statecharts/remote.md) |
| Saved defaults and model selection | `--set-default-model`, `--get-default-model`, `--reset-default-model`. `src/settings.rs`, `src/model_catalog.rs`, `src/cli.rs` and `src/model.rs`. | Main adds saved preferences below explicit flags and the environment override. Catalog listing is a one-shot read of model metadata. [Settings](../.cursor/skills/verify-fscript-settings/SKILL.md) |
| Model cache and engine startup | `src/model.rs` and `src/engine.rs`. Select an installed model, download, stage and install a bundle, or launch an SDK worker. | The cited release can remove an installed bundle before its replacement is validated. [Cache](../.cursor/skills/verify-fscript-cache/SKILL.md) |
| Python ASR worker | `src/engine.rs` and `src/python_worker.py`. Process startup, readiness, request/reply ordering, temporary request media and teardown. | Main adds Python SDK inference beside native ONNX. [Worker](../.cursor/skills/verify-fscript-worker/SKILL.md) |
| Audio chunks and transcript stitching | `src/transcribe.rs`, `src/inference.rs` and `src/transcript.rs`. Prepare ranges, infer, offset timestamps, merge overlap, and complete or abort. | [Chunking](../.cursor/skills/verify-fscript-chunking/SKILL.md) |
| Speaker detection and assignment | `src/diarization.rs` and `src/transcribe.rs`. Invoke optional FluidAudio, handle failure and no speech, or assign speakers. | [Diarization](../.cursor/skills/verify-fscript-diarization/SKILL.md) · [Speaker detection chart](statecharts/speakers.md) |
| Transcript rendering | `src/output.rs` and `src/types.rs`. Clean repeated words, accumulate speaker turns, and choose JSON, text, SRT or VTT. | An initial unlabeled turn can inherit a zero timestamp on both revisions. [Rendering](../.cursor/skills/verify-fscript-rendering/SKILL.md) · [Timestamp chart](statecharts/speaker-timestamps.md) |
| Transcript delivery | `src/app.rs` and `src/delivery.rs`. Choose the file or stdout, stage and commit output, report its path, handle closed pipes. | Main stages replacement. The cited release writes directly to the destination. [Delivery](../.cursor/skills/verify-fscript-delivery/SKILL.md) · [Save and stdout chart](statecharts/delivery.md) |
| Diagnostics and progress | `src/progress.rs`, `src/diagnostics.rs` and `src/app.rs`. Select a progress mode, stop the spinner, and restore the previous quiet scope. | [Progress](../.cursor/skills/verify-fscript-progress/SKILL.md) |
| Python CLI and wheel | `python/fscript/{__main__,_cli}.py`, `setup.py`, `pyproject.toml`, and `scripts/build_pypi_wheel.py`. Locate and execute the binary, propagate its exit, stage the wheel. | [Python package](../.cursor/skills/verify-fscript-python-package/SKILL.md) |

`src/types.rs` defines the application state and result payloads used by these flows. The compiled Rust binary is the Cargo entrypoint. The package exports the same binary through Homebrew and uses a small Python wrapper on PyPI.

## Maintainer commands

| Entrypoints | State and verification |
| --- | --- |
| `scripts/render_diarized_transcript_script.py` | Skip invalid turns, group consecutive speakers, flush the final turn. [Renderer](../.cursor/skills/verify-fscript-script-renderer/SKILL.md) |
| `scripts/generate_man_page.py` | Parse offline manual sections and fences, then generate or check roff. [Man page](../.cursor/skills/verify-fscript-manpage/SKILL.md) |
| `scripts/prepare_asr_benchmarks.py`, `scripts/prepare_tedx_pt_benchmark.py` | Prepare references and metadata, partition audio, and construct a suite. [Benchmark preparation](../.cursor/skills/verify-fscript-benchmark-preparation/SKILL.md) |
| `scripts/benchmark_asr.py` | Run worker handshake and cold/warm measurements, save or resume measured cases. [Benchmark runner](../.cursor/skills/verify-fscript-benchmark-runner/SKILL.md) |
| `scripts/validate_asr_benchmark.py`, `scripts/rescore_transcription_benchmarks.py`, `scripts/report_asr_benchmark.py` | Select references, validate and rescore runs, and gate the report. [Benchmark reporting](../.cursor/skills/verify-fscript-benchmark-reporting/SKILL.md) |
| `scripts/benchmark_english_transcription.py`, `scripts/benchmark_transcription_engines.py`, `examples/benchmark_asr_onnx.rs`, `examples/benchmark_resident_onnx.rs` | Stage historical benchmark warmup, measurement, and output acceptance. [Historical benchmarks](../.cursor/skills/verify-fscript-historical-benchmarks/SKILL.md) |
| `scripts/install-git-hooks.sh` and `.githooks/pre-commit` | Register the formatter hook and reject staged Rust changes that fail formatting. [Git hooks](../.cursor/skills/verify-fscript-git-hooks/SKILL.md) |
| `build.rs` | Gate Apple linker search, enumerate developer roots and report paths with the runtime archive. [Build discovery](../.cursor/skills/verify-fscript-build-discovery/SKILL.md) |
| `.github/workflows/release.yml` and `.github/workflows/ci.yml` | Build archives and wheels, check and smoke-test them, then gate publication on tags and successful matrix jobs. CI checks formatting, the man page, the crate dry run and wheel parity. [Release](../.cursor/skills/verify-fscript-release/SKILL.md) · [Wheel staging](../.cursor/skills/verify-fscript-python-package/SKILL.md) |
| `fuzz/fuzz_targets/cli_args.rs` and `.github/workflows/fuzz.yml` | On manual dispatch, the test target converts input into bounded argument lists. [Parser harness](../.cursor/skills/verify-fscript-parser-harness/SKILL.md) |

The source comparison covers tracked Rust and Python entrypoints, scripts, manifests, Cargo examples, hooks and workflow commands. The checks exercise bounded models with fixture files, child-process doubles or existing tests. Model inference quality, hosted media, registries, external tools and operating-system primitives remain outside the checked properties.
