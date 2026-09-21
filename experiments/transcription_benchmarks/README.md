# Transcription benchmarks

This directory brings the reusable audio and ONNX reference runs from
`vtsd-fluxo-transcript` into this repository and adds a direct comparison with
Moondream's `moondream/parakeet-redux`.

The two shared inputs are a 15-second Portuguese clip with a human reference
and a 5-minute Portuguese lecture whose reference is the existing ONNX output.
The 5-minute score is therefore a distance-from-baseline metric, not human
ground truth.

Run the comparison with a Python environment that has Moondream 2.4 and its
Photon dependencies installed:

```bash
python3 scripts/benchmark_transcription_engines.py \
  --redux-python /path/to/moondream-python
```

The runner measures the current `fscript` ONNX path and Redux on both CPU and
MPS, writing one JSON file per run plus `runs/results.csv`.

## Result on this MacBook Pro M1

| input | engine | device | wall s | realtime | peak RSS MB | WER |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 15s | current ONNX | CPU | 1.54 | 9.74x | 1,250 | **0.000** |
| 15s | Parakeet Redux | CPU | 5.97 | 2.51x | 1,071 | 0.135 |
| 15s | Parakeet Redux | MPS | 5.40 | 2.78x | 656 | 0.135 |
| 5min | current ONNX | CPU | 24.63 | 12.18x | 2,284 | **0.082** |
| 5min | Parakeet Redux | CPU | 19.86 | 15.11x | 2,225 | 0.160 |
| 5min | Parakeet Redux | MPS | 19.23 | 15.60x | 662 | 0.161 |

Redux is faster and uses less memory on the long clip, but it loses badly on
the short ground-truth clip and is materially farther from the current ONNX
transcript on the long clip. Keep the existing ONNX model as the
`fast-transcript` standard. An ONNX conversion of Redux is not justified by
these results.
