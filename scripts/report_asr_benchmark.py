#!/usr/bin/env python3
"""Render the validated canonical benchmark as a report with fixed timing scopes."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from benchmark_transcription_engines import ROOT

LABELS = {
    ("onnx", "cpu"): "ONNX CPU",
    ("parakeet-redux", "cpu"): "Redux CPU",
    ("parakeet-redux", "mps"): "Redux GPU · Metal/MPS",
    ("parakeet-ultra", "cpu"): "Ultra CPU",
    ("parakeet-ultra", "mps"): "Ultra GPU · Metal/MPS",
    ("phonon-2", "cpu"): "Phonon-2 CPU",
    ("phonon-2", "mlx"): "Phonon-2 GPU · Metal/MLX",
}
CASES = {
    "en-ted-ken-robinson": "English TED — 18m59s, 3,106 reference words",
    "pt-5min": "Portuguese lecture — 5m00s, 988 reference words",
    "en-librispeech-50": "English LibriSpeech — 50 clips / 3m52s, 789 reference words",
    "pt-15s": "Portuguese short clip — 15s, 37 reference words",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--runs-dir", type=Path, default=ROOT / "runs/2026-09-30-unified-v2"
    )
    args = parser.parse_args()
    directory = args.runs_dir
    summary = json.loads((directory / "summary.json").read_text())
    validation = json.loads((directory / "validation.json").read_text())
    environment = json.loads((directory / "environment.json").read_text())
    assert summary["protocol"] == validation["protocol"] == "asr-v2"
    assert len(summary["results"]) == 28 and all(
        s["repetitions"] == 3 for s in summary["results"]
    )
    lines = [
        "# ASR v2 — comparable cold and warm benchmarks",
        "",
        f"Measured {environment['started_at'][:10]} on {environment['hardware']} / macOS 27.0 / 128 GiB.",
        "",
        "Within each table, every model uses the same input, shared audio partitions and normalization. Processes are benchmark workers using native SDK/library APIs, not production CLI timings. Values are medians of **three fresh-worker repetitions**. Each worker transcribes once from startup, then repeats the same input with its model resident.",
        "",
        "- **New process → result:** process launch to receipt of the complete first transcript, including startup/imports/model loading. Excludes downloads, audio preparation and teardown; OS caches are populated.",
        "- **Loaded model:** full second-pass transcription, including WAV reads and GPU synchronization. Excludes startup/loading and result serialization.",
        "- **WER:** word edit distance against the reference, after lowercasing and Unicode punctuation removal. Numbers and spelling variants remain distinct. Lower is better.",
        "",
        "[Protocol and reproduction](../../README.md) · [Machine-readable summary](summary.json) · [Raw error alignments](error-alignments.json) · [Independent validation](validation.json)",
        "",
    ]
    equal = all(s["cold_wer"] == s["warm_wer"] for s in summary["results"])
    lines.extend(
        [
            "Cold and warm WER are identical for all configurations."
            if equal
            else "WER is shown as cold / warm when the two phases differ.",
            "",
        ]
    )
    for case, title in CASES.items():
        lines.extend([f"## {title}", ""])
        if case == "en-ted-ken-robinson":
            lines.extend(
                [
                    "Source: [Sir Ken Robinson — Do schools kill creativity? (TED)](https://www.youtube.com/watch?v=iG9CE55wbtY). The video is 20m03s; the full captioned talk spans 26.603–1165.713 s including 0.5-s margins. Publisher English captions, not automatic captions, are the reference. Only non-speech annotations/speaker labels were removed.",
                    "",
                ]
            )
        lines.extend(
            [
                "| Model / runtime | New process → result (s) | Loaded model (s) | Loaded realtime | WER |",
                "| --- | ---: | ---: | ---: | ---: |",
            ]
        )
        rows = {
            (s["engine"], s["device"]): s
            for s in summary["results"]
            if s["case"] == case
        }
        assert set(rows) == set(LABELS)
        for key, label in LABELS.items():
            s = rows[key]
            wer = f"{s['warm_wer'] * 100:.2f}%"
            if s["cold_wer"] != s["warm_wer"]:
                wer = f"{s['cold_wer'] * 100:.2f}% / {wer}"
            lines.append(
                f"| {label} | {s['cold_result_seconds']:.3f} | {s['warm_transcribe_seconds']:.3f} | {s['warm_realtime_speedup']:.1f}× | {wer} |"
            )
        lines.append("")
    lines.extend(
        [
            "## Scope and validation",
            "",
            "TED captions are edited text, not guaranteed verbatim speech. Filler words, repetitions, spelling and number formatting can contribute to WER; a mismatch is not automatically a recognition error. Per-model insertion/deletion/substitution counts and the exact alignments are saved. The Portuguese lecture reference began as an Ultra draft reviewed by the user. LibriSpeech covers one speaker. No general language or model ranking follows from these few recordings.",
            "",
            "Shared partitioning also differs from historical runs: boundaries are chosen from audio energy between 25–30 s, the last remainder is at most 31 s, and every PCM sample appears exactly once. Models receive the same parts. Therefore compare rows **within this report**, not against older runs with different timing/chunking protocols.",
            "",
            f"Independent jiwer validation checked **{validation['process_repetitions_checked']} process repetitions, {validation['corpus_passes_checked']} corpus passes and {validation['utterances_checked']} utterance transcripts**, plus input hashes, sample-exact reconstruction of all {validation['audio_partitions_checked']} audio parts, coverage, timings, medians and ranges.",
            "",
            f"Exact transcripts are stable across cold/warm phases and all repetitions in {sum(s['transcripts_stable'] for s in summary['results'])}/28 configurations. See raw results for any variation. Process resource logs cover both passes and are not a third competing latency metric.",
            "",
        ]
    )
    (directory / "REPORT.md").write_text("\n".join(lines))
    print(directory / "REPORT.md")


if __name__ == "__main__":
    main()
