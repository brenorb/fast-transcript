---
name: verify-fscript-benchmark-preparation
description: Verify sample-exact partitioning and trace benchmark dataset preparation when changing the fscript ASR manifests.
---

# Verify benchmark WAV partitioning

Benchmark preparation converts source material to 16 kHz mono PCM, chooses quiet cut points, emits contiguous WAV parts and records their ranges and hashes. Both dataset preparation commands share the partitioner.

Use Java 11 or later with `TLA2TOOLS` pointing to [TLC 1.7.4](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4). The replay needs Python with NumPy installed.

```sh
skill=.cursor/skills/verify-fscript-benchmark-preparation
sh scripts/check-cli-state-machine.sh "$skill/Partition.tla"
python3 "$skill/references/check-partitions.py"
```

## Coverage

Reviewed main `7562797c4462045c2a3edc2ceaf9595ed6a5d626`. Both preparation CLIs were added after the cited `v1.1.3` tag at `751f294296a0034360231d6acfe9cf3c700cb0a8`.

| Transition | Source |
| --- | --- |
| Select publisher caption input or automatic Portuguese caption input, normalize references and crop audio | `scripts/prepare_asr_benchmarks.py`, `scripts/prepare_tedx_pt_benchmark.py`, respective `main` functions |
| Read PCM, choose cut boundaries and write each WAV | `prepare_asr_benchmarks.py`, `partition` |
| Hash parts, write manifests and publish the suite list | `prepare_asr_benchmarks.py`, `prepare_tedx_pt_benchmark.py`, respective `main` functions |

The model bounds positive input lengths up to eight units and uses a maximum part length of four units. It abstracts the production 31-second limit and quiet-cut search to nondeterministic bounded cuts. It checks contiguous, nonempty ranges that exhaust the input. It omits caption retrieval and alignment, PCM encoding details and failures partway through writing a suite. The fixture uses real WAVs and compares the exact reconstructed PCM, including inputs at and just above the production split threshold.

TLC checked 52 states. `Partition-mutant.cfg` offsets the start of every part after the first. In a disposable checkout, add one sample to the PCM slice start for nonzero starts in `partition`. The WAV frame-count assertion catches the mutation. Rerun the model and fixture after restoring and update the reviewed refs on a change.

Use pstack’s `/statechart` to inspect the shortest path for a failing rule or visualize the checked flow. When the source changes, update the reviewed revision after rechecking the model and source fixture.
