# CLI conventions and guideline review

This records the applicable [Command Line Interface Guidelines](https://clig.dev/)
for `fscript`. The executable transcribes media; its ordinary invocation does not
publish data or prompt for input. The [offline manual](cli.md) documents all
options, installation requirements, configuration, output, and recovery.

| Area | Current behavior and evidence |
| --- | --- |
| Basics | Clap parses flags and operands; `app.rs` handles informational requests before loading settings or models. Success is 0 and operational errors are 1. Transcript data or a result path uses stdout; status and warnings use stderr. |
| Help and discovery | Bare invocation shows a short introduction. `-h`, `--help`, `help`, and clustered `-qh` display examples, defaults, documentation, and support links. Help takes precedence over invalid arguments; `--` protects literal filenames. |
| Documentation | README provides workflows and distribution instructions. `--manual` and `help manual` provide complete installed documentation offline; `--man-page` exports the generated roff manual. CI checks the generated page. |
| Output | Speaker-aware/text formats serve humans. Explicit JSON includes transcript data, segments, and timings. `-` input supports binary media pipelines, and `--stdout`/`--output -` select transcript stdout. File output retains the existing absolute-path contract. |
| Errors | Invalid flags suggest nearby options, malformed settings offer reset guidance, and missing dependencies identify installation steps. Ordinary errors print their causes without a backtrace. Worker failures retain diagnostics even with `--quiet`. |
| Arguments and compatibility | Long options are preferred while existing short aliases remain. Conflicting formats, destinations, model choices, and nonfinite numeric values fail before processing. All routing respects the end-of-options separator, including settings-like filenames. |
| Interactivity and terminal behavior | Commands do not prompt. Explicit stdin input rejects a terminal immediately with a pipeline example. Animation requires interactive stderr, is disabled by `TERM=dumb` or quiet mode, and stops on failure. Color controls are unnecessary because fscript emits no ANSI color. |
| Robustness | Transcript writes stage and sync beside the destination before atomic replacement. Failed staging preserves previous contents; file permissions and symlink destinations remain effective. Model settings already save atomically. Tests inject partial-write failure and exercise real subprocess streams. |
| Signals and pipes | Closed stdout consumers succeed quietly; other stream failures remain errors. Ctrl-C retains the normal process signal behavior. Abrupt termination can leave temporary staging/media files; output recovery is documented. |
| Configuration and environment | Explicit model flags override environment, saved preference, then built-in defaults. Platform per-user configuration, application data, and caches are separate. `FSCRIPT_*` overrides and setup requirements are listed in the manual. |
| Naming, distribution, and analytics | Existing `fscript` executable and fast-transcript repository/package names remain. Homebrew, PyPI/uv, Cargo, and source installation are documented. No telemetry or automatic update check is added. |

## Verification and limits

`cargo test --locked` covers parsing, settings, rendering, delivery, workers, stdin,
quiet mode, closed stdout, and actual CLI exit behavior. Regression tests first
failed for atomic replacement, ordinary backtraces, closed pipes, unfinished
progress workers, clustered help/version flags, output write access, dangling
symlinks, and settings names following `--`.

Run `cargo fmt --check`, `python3 scripts/generate_man_page.py --check`, and
`cargo publish --dry-run --locked` before pushing. CI also builds the release
binary and Python wheel, checks the wrapper/worker/settings suites, and invokes
the installed wheel entry point. Hardware transcription accuracy and optional
model/FluidAudio runtimes require their existing benchmark and integration
workflows; CLI fixture tests do not establish model quality.

Subcommands beyond the existing help alias, shell completions, a pager, and an
interactive overwrite prompt are not required by the current single-operation
interface. Existing result files are deliberately replaced on reruns, as described
in the manual; callers can choose a different output path to retain earlier runs.
