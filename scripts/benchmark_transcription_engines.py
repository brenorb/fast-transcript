#!/usr/bin/env python3
"""Compare ONNX, Moondream Parakeet and Fermion Phonon on macOS."""

from __future__ import annotations

import argparse
import csv
import json
import platform
import re
import statistics
import subprocess
import sys
import unicodedata
import wave
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "experiments" / "transcription_benchmarks"
CASES = {
    "15s": (
        ROOT / "audio" / "audio_15s_16k_mono.wav",
        ROOT / "references" / "15s.json",
    ),
    "5min": (
        ROOT / "audio" / "audio_5min_16k_mono.wav",
        ROOT / "references" / "5min-onnx.json",
    ),
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


def normalize(text: str) -> str:
    return " ".join(
        "".join(
            char
            for char in text.lower()
            if not unicodedata.category(char).startswith("P")
        ).split()
    )


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
        check=False,
        capture_output=True,
        text=True,
        timeout=3600,
    )
    if completed.returncode:
        raise RuntimeError(
            f"Command failed ({completed.returncode}): {command}\n{completed.stderr}"
        )
    return json.loads(completed.stdout), time_metrics(completed.stderr)


PHOTON_CODE = r"""
import json, sys, time
from importlib.metadata import version
from pathlib import Path
import moondream as md
import torch
from kestrel.models.parakeet_tdt.weights import _PINNED_REVISIONS
audio = Path(sys.argv[1])
device = sys.argv[2]
model_id = sys.argv[3]
started = time.perf_counter()
with md.photon(model_id, device=device) as model:
    loaded = time.perf_counter()
    output = model.transcribe(audio=audio, timestamps="segment")
    if device == "mps":
        torch.mps.synchronize()
    finished = time.perf_counter()
output["load_seconds"] = loaded - started
output["transcribe_seconds"] = finished - loaded
output["total_inside_seconds"] = finished - started
output["model_id"] = model_id
output["model_revision"] = _PINNED_REVISIONS[model_id]
output["runtime_versions"] = {name: version(name) for name in ("moondream", "kestrel", "torch")}
print(json.dumps(output, ensure_ascii=False))
"""


def main() -> int:
    print(
        "Historical benchmark protocol. Use scripts/benchmark_asr.py for comparable cold/warm results.",
        file=sys.stderr,
    )
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--photon-python",
        "--redux-python",
        dest="photon_python",
        type=Path,
    )
    parser.add_argument(
        "--fermion", type=Path, help="Fermion CLI from its own Python environment"
    )
    parser.add_argument("--five-minute-reference", type=Path, default=CASES["5min"][1])
    parser.add_argument("--fscript", default="fscript")
    parser.add_argument("--out-dir", type=Path, default=ROOT / "runs")
    parser.add_argument(
        "--models",
        nargs="+",
        choices=("parakeet-redux", "parakeet-ultra", "phonon-2"),
        default=["parakeet-redux"],
    )
    parser.add_argument(
        "--devices", nargs="+", choices=("cpu", "mps"), default=["cpu", "mps"]
    )
    parser.add_argument("--repetitions", type=int, default=1)
    parser.add_argument(
        "--warmup",
        action="store_true",
        help="Run each command once before timing to populate download and filesystem caches",
    )
    args = parser.parse_args()
    if args.repetitions < 1:
        parser.error("--repetitions must be positive")
    if any(model != "phonon-2" for model in args.models) and args.photon_python is None:
        parser.error("--photon-python is required for Moondream models")
    if "phonon-2" in args.models and args.fermion is None:
        parser.error("--fermion is required for Phonon")
    args.out_dir.mkdir(parents=True, exist_ok=True)

    metadata = {
        "started_at": datetime.now(timezone.utc).isoformat(),
        "platform": platform.platform(),
        "hardware": subprocess.check_output(
            ["sysctl", "-n", "machdep.cpu.brand_string"], text=True
        ).strip(),
        "memory_bytes": int(
            subprocess.check_output(["sysctl", "-n", "hw.memsize"], text=True)
        ),
        "fscript_version": subprocess.check_output(
            [args.fscript, "--version"], text=True
        ).strip(),
        "arguments": {
            key: str(value) if isinstance(value, Path) else value
            for key, value in vars(args).items()
        },
        "timing_note": "Sequential fresh processes; wall time includes imports, model loading and teardown. Warmup is a separate discarded process, not persistent-model warm inference.",
        "normalized_wer_note": "Lowercase and remove Unicode punctuation; preserve accents and numerals. Raw WER/CER retain original case and punctuation for historical comparisons.",
    }
    (args.out_dir / "environment.json").write_text(
        json.dumps(metadata, indent=2) + "\n"
    )

    rows: list[dict[str, object]] = []
    cases = {**CASES, "5min": (CASES["5min"][0], args.five_minute_reference)}
    for audio_id, (audio_path, reference_path) in cases.items():
        reference_data = json.loads(reference_path.read_text(encoding="utf-8"))
        reference = reference_data["text"]
        reference_note = reference_data.get(
            "reference_type",
            "human ground truth" if audio_id == "15s" else "historical ONNX output",
        )
        with wave.open(str(audio_path), "rb") as audio:
            audio_seconds = audio.getnframes() / audio.getframerate()
        commands = [
            (
                "onnx",
                "cpu",
                [args.fscript, str(audio_path), "--json", "--stdout", "-D", "--raw"],
            )
        ]
        for model in args.models:
            for device in args.devices:
                if model == "phonon-2":
                    backend = "mlx" if device == "mps" else device
                    commands.append(
                        (
                            model,
                            backend,
                            [
                                "env",
                                f"FERMION_DEVICE={backend}",
                                str(args.fermion),
                                "transcribe",
                                str(audio_path),
                                "--model",
                                "phonon-2",
                                "--json",
                            ],
                        )
                    )
                    continue
                commands.append(
                    (
                        model,
                        device,
                        [
                            str(args.photon_python),
                            "-c",
                            PHOTON_CODE,
                            str(audio_path),
                            device,
                            f"moondream/{model}",
                        ],
                    )
                )

        for engine, device, command in commands:
            if args.warmup:
                print(
                    f"Warming {audio_id} {engine} {device}", file=sys.stderr, flush=True
                )
                run(command)
            for repetition in range(1, args.repetitions + 1):
                record = measure(
                    command,
                    audio_id,
                    audio_seconds,
                    reference,
                    reference_note,
                    engine,
                    device,
                    repetition,
                    args,
                )
                rows.append(record)
                print(json.dumps(record, ensure_ascii=False), flush=True)

    fields = list(rows[0])
    with (args.out_dir / "results.csv").open(
        "w", newline="", encoding="utf-8"
    ) as handle:
        writer = csv.DictWriter(handle, fieldnames=fields, lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)
    summary = []
    for audio_id in CASES:
        for engine, device in dict.fromkeys(
            (row["engine"], row["device"]) for row in rows
        ):
            group = [
                row
                for row in rows
                if (row["audio_id"], row["engine"], row["device"])
                == (audio_id, engine, device)
            ]
            entry = {
                "audio_id": audio_id,
                "engine": engine,
                "device": device,
                "runs": len(group),
            }
            for field in (
                "wall_clock_s",
                "realtime_speedup",
                "max_rss_mb",
                "load_seconds",
                "transcribe_seconds",
                "decode_seconds",
                "wer_vs_reference",
                "normalized_wer_vs_reference",
            ):
                values = [row[field] for row in group if row[field] is not None]
                entry[field] = statistics.median(values) if values else None
            entry["wall_clock_min_s"] = min(row["wall_clock_s"] for row in group)
            entry["wall_clock_max_s"] = max(row["wall_clock_s"] for row in group)
            summary.append(entry)
    (args.out_dir / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    return 0


def measure(
    command,
    audio_id,
    audio_seconds,
    reference,
    reference_note,
    engine,
    device,
    repetition,
    args,
):
    payload, timing = run(command)
    if engine == "phonon-2" and (
        payload.get("engine") != device or payload.get("truncated")
    ):
        raise RuntimeError(f"Unexpected Phonon backend or truncated output: {payload}")
    wall = timing.get("wall_clock_s", payload.get("total_inside_seconds", 0.0))
    text = payload.get("text", "")
    record = {
        "engine": engine,
        "device": device,
        "audio_id": audio_id,
        "repetition": repetition,
        "audio_seconds": audio_seconds,
        "wall_clock_s": wall,
        "realtime_speedup": audio_seconds / wall if wall else 0.0,
        "max_rss_mb": timing.get("max_rss_mb"),
        "load_seconds": payload.get("load_seconds"),
        "transcribe_seconds": payload.get("transcribe_seconds"),
        "decode_seconds": payload.get("decode_seconds"),
        "wer_vs_reference": error_rate(reference, text, words=True),
        "cer_vs_reference": error_rate(reference, text, words=False),
        "normalized_wer_vs_reference": error_rate(
            normalize(reference), normalize(text), words=True
        ),
        "reference_note": reference_note,
    }
    stem = f"{audio_id}_{engine}_{device}"
    if args.repetitions > 1:
        stem += f"_r{repetition}"
    (args.out_dir / f"{stem}.json").write_text(
        json.dumps({"metrics": record, "result": payload}, ensure_ascii=False, indent=2)
        + "\n",
        encoding="utf-8",
    )
    return record


if __name__ == "__main__":
    sys.exit(main())
