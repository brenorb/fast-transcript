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

## Short LibriSpeech validation

The first 50 `validation.clean` utterances from LibriSpeech were run as
separate short files (232.005 seconds total). WER uses the dataset text after
lowercasing and removing punctuation. The warm column excludes model load;
the CLI wall column includes the current `fscript` process startup for every
utterance.

| engine | device | warm inference | CLI/process wall | WER |
| --- | --- | ---: | ---: | ---: |
| current ONNX | CPU | 20.61x | 4.87x | 0.0152 |
| Parakeet Redux | CPU | 25.50x | 12.82x | 0.0114 |
| Parakeet Redux | MPS | 9.95x | 8.93x | 0.0114 |

Redux wins this short-set warm-inference test by 24%, with a small WER edge.
That is not enough to replace the current standard: the ONNX model remains
more accurate on the checked-in ground-truth clip, and the MPS Redux path is
slower on this M1 for short calls.

## Underdog ASR check

Underdog's **Husky** package is an inference engine for the text-only Woof
model, so it cannot be compared as a transcription engine. The closest valid
test is Underdog's separate 4-bit MLX ASR model,
`ConwayResearch/Underdog-Bark-0.8B-1.0`.

| input | engine | wall realtime | WER |
| --- | --- | ---: | ---: |
| LibriSpeech, 50 short utterances | Bark MLX | 11.00x warm | 0.0177 |
| Portuguese, 15s | Bark MLX | 5.74x | 0.150 |
| Portuguese, 5min | Bark MLX | 7.82x | 0.185 |

Bark is slower and less accurate than the current ONNX path on both
Portuguese clips, and slower than Redux on the short English set. It does not
replace the current standard.
