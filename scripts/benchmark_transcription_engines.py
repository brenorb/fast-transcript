#!/usr/bin/env python3
"""Compare the checked-in ONNX CLI with Moondream's Parakeet Redux."""

from __future__ import annotations

import argparse
import csv
import json
import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1] / "experiments" / "transcription_benchmarks"
CASES = {
    "15s": (ROOT / "audio" / "audio_15s_16k_mono.wav", ROOT / "references" / "15s.json"),
    "5min": (ROOT / "audio" / "audio_5min_16k_mono.wav", ROOT / "references" / "5min-onnx.json"),
}
TIME_RE = re.compile(r"^\s*([0-9.]+)\s+real\s+([0-9.]+)\s+user\s+([0-9.]+)\s+sys\s*$")
RSS_RE = re.compile(r"^\s*(\d+)\s+maximum resident set size\s*$")


def edit_distance(left: list[str], right: list[str]) -> int:
    row = list(range(len(right) + 1))
    for i, item in enumerate(left, 1):
        previous = row[0]
        row[0] = i
        for j, other in enumerate(right, 1):
            current = row[j]
            row[j] = min(row[j] + 1, row[j - 1] + 1, previous + (item != other))
            previous = current
    return row[-1]


def error_rate(reference: str, hypothesis: str, *, words: bool) -> float:
    left = reference.split() if words else list(reference)
    right = hypothesis.split() if words else list(hypothesis)
    return edit_distance(left, right) / len(left) if left else float(bool(right))


def time_metrics(stderr: str) -> dict[str, float]:
    metrics: dict[str, float] = {}
    for line in stderr.splitlines():
        if match := TIME_RE.match(line):
            metrics.update(
                wall_clock_s=float(match.group(1)),
                user_cpu_s=float(match.group(2)),
                system_cpu_s=float(match.group(3)),
            )
        elif match := RSS_RE.match(line):
            metrics["max_rss_mb"] = int(match.group(1)) / 1024 / 1024
    return metrics


def run(command: list[str]) -> tuple[dict, dict[str, float]]:
    completed = subprocess.run(
        ["/usr/bin/time", "-l", *command],
        check=True,
        capture_output=True,
        text=True,
        timeout=3600,
    )
    return json.loads(completed.stdout), time_metrics(completed.stderr)


REDUX_CODE = r'''
import json, sys, time
from pathlib import Path
import moondream as md
audio = Path(sys.argv[1])
device = sys.argv[2]
started = time.perf_counter()
with md.photon("moondream/parakeet-redux", device=device) as model:
    loaded = time.perf_counter()
    output = model.transcribe(audio=audio, timestamps="segment")
    finished = time.perf_counter()
output["load_seconds"] = loaded - started
output["transcribe_seconds"] = finished - loaded
output["total_inside_seconds"] = finished - started
print(json.dumps(output, ensure_ascii=False))
'''


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--redux-python", required=True, type=Path)
    parser.add_argument("--fscript", default="fscript")
    parser.add_argument("--out-dir", type=Path, default=ROOT / "runs")
    args = parser.parse_args()
    args.out_dir.mkdir(parents=True, exist_ok=True)

    rows: list[dict[str, object]] = []
    for audio_id, (audio_path, reference_path) in CASES.items():
        reference = json.loads(reference_path.read_text(encoding="utf-8"))["text"]
        commands = [("onnx", "cpu", [args.fscript, str(audio_path), "--json", "--stdout", "-D", "--raw"])]
        for device in ("cpu", "mps"):
            commands.append(
                (
                    "parakeet-redux",
                    device,
                    [str(args.redux_python), "-c", REDUX_CODE, str(audio_path), device],
                )
            )

        for engine, device, command in commands:
            payload, timing = run(command)
            wall = timing.get("wall_clock_s", payload.get("total_inside_seconds", 0.0))
            audio_seconds = payload.get("audio_seconds", payload.get("source_duration_seconds", 0.0))
            text = payload.get("text", "")
            record = {
                "engine": engine,
                "device": device,
                "audio_id": audio_id,
                "audio_seconds": audio_seconds,
                "wall_clock_s": wall,
                "realtime_speedup": audio_seconds / wall if wall else 0.0,
                "max_rss_mb": timing.get("max_rss_mb"),
                "load_seconds": payload.get("load_seconds"),
                "transcribe_seconds": payload.get("transcribe_seconds"),
                "wer_vs_reference": error_rate(reference, text, words=True),
                "cer_vs_reference": error_rate(reference, text, words=False),
                "reference_note": "human ground truth" if audio_id == "15s" else "current ONNX output",
            }
            stem = f"{audio_id}_{engine}_{device}"
            (args.out_dir / f"{stem}.json").write_text(
                json.dumps({"metrics": record, "result": payload}, ensure_ascii=False, indent=2) + "\n",
                encoding="utf-8",
            )
            rows.append(record)
            print(json.dumps(record, ensure_ascii=False))

    fields = list(rows[0])
    with (args.out_dir / "results.csv").open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields, lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)
    return 0


if __name__ == "__main__":
    sys.exit(main())
