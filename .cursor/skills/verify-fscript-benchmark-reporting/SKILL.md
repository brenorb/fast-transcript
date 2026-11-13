---
name: verify-fscript-benchmark-reporting
description: Verify benchmark reference selection, independent validation, and report artifact gates when changing fscript scoring and reporting scripts.
---

# Verify benchmark validation, rescoring and reports

The validator checks dataset and result inputs before writing verification artifacts. The report generator compares protocols before rendering the benchmark summary. The historical rescoring CLI uses matching saved transcripts and a complete reference, then writes new scores without rerunning inference.

Use Java 11 or later with `TLA2TOOLS` pointing to [TLC 1.7.4](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4). The validator fixture needs Python with jiwer 4.0.0.

```sh
skill=.cursor/skills/verify-fscript-benchmark-reporting
sh scripts/check-cli-state-machine.sh "$skill/Reporting.tla"
python3 "$skill/references/check-reporting.py"
python3 "$skill/references/check-validation-gate.py"
```

## Coverage

Reviewed main `7562797c4462045c2a3edc2ceaf9595ed6a5d626`. These maintenance CLIs were added after tag `v1.1.3` at `751f294296a0034360231d6acfe9cf3c700cb0a8`.

| Transition | Source |
| --- | --- |
| Validate complete reference, select runs with matching audio ID, score and write JSON and CSV | `scripts/rescore_transcription_benchmarks.py`, `main` |
| Check suite protocol, input hashes, reconstructed WAV partitions and caption alignment | `scripts/validate_asr_benchmark.py`, `main` |
| Recompute per-run metrics, require expected case/configuration/repetition coverage, check summary and write verification artifacts | `validate_asr_benchmark.py`, `main` |
| Load benchmark metadata, summary and validation, check compatible results, and render a report | `scripts/report_asr_benchmark.py`, `main` |

The reporting model bounds one invocation and distinguishes its output set and failure while writing. It abstracts the individual metric and protocol checks to input validity and a match flag. It omits output aliasing, incomplete datasets on disk and score arithmetic. The source fixture reproduces the checked-in unified report from its saved inputs apart from the line measuring the temporary directory's current size. It also scores a small saved run, checks its original bytes and tests rejection of a bad reference. A full historical benchmark corpus rerun requires the original audio and worker artifacts. These replays exercise local gates and report data only.

TLC checked 36 states. `Reporting-mutant.cfg` bypasses the input check. In a disposable checkout, remove the protocol comparison in `report_asr_benchmark.py` or the suite protocol assertion in `validate_asr_benchmark.py`. Each fixture catches the relevant bypass. Rerun the source checks and update reviewed refs when changing the artifact gates.

## Empty-suite validation

```sh
sh scripts/check-cli-state-machine.sh "$skill/ValidationCoverage.tla"
sh scripts/check-cli-state-machine.sh "$skill/ValidationCoverage.tla" "$skill/ValidationCoverage-nonempty.cfg"
```

`ValidationHasCases` fails under the current behavior. An empty manifest list with the expected protocol and empty run metadata passes the coverage comparisons and writes `validation.json` with zero checked runs. The validator fixture reproduces this with temporary JSON files. The second config models a gate that requires a nonempty suite.
