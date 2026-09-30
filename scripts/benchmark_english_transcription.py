#!/usr/bin/env python3
"""Reproducible LibriSpeech benchmark with a resident model and corpus-level WER."""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
import platform
import re
import statistics
import subprocess
import sys
import time
import urllib.parse
import urllib.request
import wave
from datetime import datetime, timezone
from importlib.metadata import version
from pathlib import Path

from benchmark_transcription_engines import ROOT, edit_distance, run

DATASET = "openslr/librispeech_asr"
REVISION = "71cacbfb7e2354c4226d01e70d77d5fca3d04ba1"
REPO = ROOT.parents[1]


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n")


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def normalize(text):
    # Match the historical English report's lowercase alphanumeric words.
    return re.sub(r"[^a-z0-9\s]", "", text.lower()).split()


def prepare(manifest_path):
    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    audio_dir = manifest_path.parent / "audio"
    audio_dir.mkdir(exist_ok=True)
    (manifest_path.parent / ".gitignore").write_text("/audio/\n")
    params = urllib.parse.urlencode(
        {
            "dataset": DATASET,
            "config": "clean",
            "split": "validation",
            "offset": 0,
            "length": 50,
        }
    )
    url = "https://datasets-server.huggingface.co/rows?" + params
    with urllib.request.urlopen(url, timeout=60) as response:
        data = json.load(response)
    assert len(data["rows"]) == 50 and not data["partial"]
    records = []
    for entry in data["rows"]:
        row = entry["row"]
        assert not entry["truncated_cells"]
        src = row["audio"][0]["src"]
        assert f"/--/{REVISION}/--/" in src, (
            "Dataset changed; review the selection before benchmarking"
        )
        flac = audio_dir / (row["id"] + ".flac")
        wav = flac.with_suffix(".wav")
        if not flac.exists():
            with urllib.request.urlopen(src, timeout=60) as response:
                flac.write_bytes(response.read())
        if not wav.exists():
            subprocess.run(
                [
                    "ffmpeg",
                    "-v",
                    "error",
                    "-i",
                    str(flac),
                    "-ar",
                    "16000",
                    "-ac",
                    "1",
                    "-c:a",
                    "pcm_s16le",
                    str(wav),
                ],
                check=True,
            )
        with wave.open(str(wav)) as audio:
            assert (
                audio.getframerate() == 16000
                and audio.getnchannels() == 1
                and audio.getsampwidth() == 2
            )
            seconds = audio.getnframes() / audio.getframerate()
        records.append(
            {
                "id": row["id"],
                "row_index": entry["row_idx"],
                "speaker_id": row["speaker_id"],
                "chapter_id": row["chapter_id"],
                "text": row["text"],
                "audio_path": str(wav.relative_to(manifest_path.parent)),
                "audio_seconds": seconds,
                "wav_sha256": sha256(wav),
                "flac_sha256": sha256(flac),
            }
        )
    manifest = {
        "dataset": DATASET,
        "revision": REVISION,
        "config": "clean",
        "split": "validation",
        "selection": "Dataset Viewer rows 0–49",
        "fetched_at": datetime.now(timezone.utc).isoformat(),
        "audio_seconds": sum(r["audio_seconds"] for r in records),
        "reference_words": sum(len(normalize(r["text"])) for r in records),
        "normalization": "lowercase; remove non-ASCII-alphanumeric punctuation; split whitespace",
        "utterances": records,
    }
    write_json(manifest_path, manifest)
    print(
        f"Prepared {len(records)} utterances / {manifest['audio_seconds']:.3f}s / {manifest['reference_words']} words",
        flush=True,
    )


def worker(args):
    manifest = json.loads(args.manifest.read_text())
    started = time.perf_counter()
    if args.worker == "phonon-2":
        os.environ["FERMION_DEVICE"] = args.device
        from fermion._speech import backends, fetch
        from fermion.transcribe import _resolve

        repo, key, pin, local = _resolve("phonon-2")
        engine = backends.resolve("English benchmark")
        assert engine == args.device
        backends.require_engine_for(engine, pin["backend"])
        model_dir = local if local is not None else fetch.ensure(repo, key, pin)
        model = backends.load(
            engine, model_dir, profile=key, backend=pin["backend"], quiet=True
        )
        metadata = {
            "model": repo,
            "pin": pin,
            "runtime_versions": {
                n: version(n) for n in ("fermion-research", "mlx", "torch")
            },
        }

        def transcribe(path):
            output = model.transcribe_detailed(str(path))
            text, decode_s, duration_s = output.triple()
            assert not output.truncated, f"Truncated: {path}"
            return {
                "text": text,
                "decode_seconds": decode_s,
                "duration_seconds": duration_s,
                "truncated": bool(output.truncated),
            }
    else:
        import moondream as md
        import torch
        from kestrel.models.parakeet_tdt.weights import _PINNED_REVISIONS

        model_id = f"moondream/{args.worker}"
        context = md.photon(model_id, device=args.device)
        model = context.__enter__()
        metadata = {
            "model": model_id,
            "model_revision": _PINNED_REVISIONS[model_id],
            "runtime_versions": {
                n: version(n) for n in ("moondream", "kestrel", "torch")
            },
        }

        def transcribe(path):
            output = model.transcribe(audio=path, timestamps="segment")
            if args.device == "mps":
                torch.mps.synchronize()
            return {"text": output["text"]}

    loaded = time.perf_counter()
    runs = []
    for repetition in range(args.repetitions + 1):
        pass_started = time.perf_counter()
        for row in manifest["utterances"]:
            path = args.manifest.parent / row["audio_path"]
            start = time.perf_counter()
            output = transcribe(path)
            seconds = time.perf_counter() - start
            if repetition:
                runs.append(
                    dict(
                        repetition=repetition,
                        id=row["id"],
                        transcribe_seconds=seconds,
                        **output,
                    )
                )
        if repetition == 0:
            warmup_seconds = time.perf_counter() - pass_started
        print(
            f"{args.worker} {args.device} pass {repetition}/{args.repetitions} complete",
            file=sys.stderr,
            flush=True,
        )
    if args.worker != "phonon-2":
        context.__exit__(None, None, None)
    print(
        json.dumps(
            dict(
                engine=args.worker,
                device=args.device,
                initialization_seconds=loaded - started,
                warmup_seconds=warmup_seconds,
                runs=runs,
                **metadata,
            )
        )
    )


def benchmark(args):
    manifest = json.loads(args.manifest.read_text())
    for row in manifest["utterances"]:
        assert sha256(args.manifest.parent / row["audio_path"]) == row["wav_sha256"]
    args.out_dir.mkdir(parents=True, exist_ok=True)
    env = {
        "started_at": datetime.now(timezone.utc).isoformat(),
        "hardware": subprocess.check_output(
            ["sysctl", "-n", "machdep.cpu.brand_string"], text=True
        ).strip(),
        "platform": platform.platform(),
        "manifest_sha256": sha256(args.manifest),
        "normalization": "lowercase; remove non-ASCII-alphanumeric punctuation; split whitespace",
        "timing": "One process per configuration, one resident model; one discarded full-corpus warmup then the requested number of measured corpus passes. Call timer includes WAV reading and transcription; MPS synchronization included. Process wall includes initialization, warmup, all passes, teardown. Initialization includes Python imports but Rust model load only; do not compare these as pure model-load times.",
        "arguments": {
            k: str(v) if isinstance(v, Path) else v for k, v in vars(args).items()
        },
        "source_commit": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], text=True
        ).strip(),
    }
    write_json(args.out_dir / "environment.json", env)
    configurations = [
        (
            "onnx",
            "cpu",
            [
                str(args.onnx_worker),
                str(args.manifest),
                str(args.onnx_model),
                str(args.repetitions),
            ],
        )
    ]
    for engine in ("parakeet-redux", "parakeet-ultra", "phonon-2"):
        for device in ("cpu", "mlx") if engine == "phonon-2" else ("cpu", "mps"):
            python = args.phonon_python if engine == "phonon-2" else args.photon_python
            configurations.append(
                (
                    engine,
                    device,
                    [
                        str(python),
                        str(Path(__file__).resolve()),
                        "--worker",
                        engine,
                        "--device",
                        device,
                        "--manifest",
                        str(args.manifest),
                        "--repetitions",
                        str(args.repetitions),
                    ],
                )
            )
    summaries = []
    refs = {r["id"]: normalize(r["text"]) for r in manifest["utterances"]}
    words = sum(map(len, refs.values()))
    for engine, device, command in configurations:
        print(f"Running {engine} {device}...", flush=True)
        output, process = run(command)
        assert len(output["runs"]) == len(refs) * args.repetitions
        passes = []
        for rep in range(1, args.repetitions + 1):
            rows = [r for r in output["runs"] if r["repetition"] == rep]
            assert len(rows) == len(refs) and {r["id"] for r in rows} == set(refs)
            for row in rows:
                row["word_edits"] = edit_distance(
                    refs[row["id"]], normalize(row["text"])
                )
            seconds = sum(r["transcribe_seconds"] for r in rows)
            edits = sum(r["word_edits"] for r in rows)
            passes.append(
                {
                    "repetition": rep,
                    "transcribe_seconds": seconds,
                    "realtime_speedup": manifest["audio_seconds"] / seconds,
                    "word_edits": edits,
                    "reference_words": words,
                    "wer": edits / words,
                }
            )
        write_json(
            args.out_dir / f"{engine}-{device}.json",
            dict(process=process, passes=passes, **output),
        )
        summary = {
            "engine": engine,
            "device": device,
            "median_transcribe_seconds": statistics.median(
                p["transcribe_seconds"] for p in passes
            ),
            "median_realtime_speedup": statistics.median(
                p["realtime_speedup"] for p in passes
            ),
            "median_wer": statistics.median(p["wer"] for p in passes),
            "word_edits_by_pass": [p["word_edits"] for p in passes],
            "reference_words": words,
            "peak_rss_mib": process["max_rss_mb"],
            "process_wall_seconds": process["wall_clock_s"],
            "transcripts_stable": all(
                len({r["text"] for r in output["runs"] if r["id"] == id_}) == 1
                for id_ in refs
            ),
        }
        summaries.append(summary)
        write_json(
            args.out_dir / "summary.json",
            {
                "dataset": DATASET,
                "revision": REVISION,
                "audio_seconds": manifest["audio_seconds"],
                "utterances": len(refs),
                "repetitions": args.repetitions,
                "results": summaries,
            },
        )
        print(json.dumps(summary), flush=True)
    with (args.out_dir / "results.csv").open("w") as file:
        writer = csv.DictWriter(file, fieldnames=list(summaries[0]))
        writer.writeheader()
        writer.writerows(summaries)


def main():
    print(
        "Historical benchmark protocol. Use scripts/benchmark_asr.py for comparable cold/warm results.",
        file=sys.stderr,
    )
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--manifest",
        type=Path,
        default=ROOT / "datasets/librispeech-validation-clean-50/manifest.json",
    )
    parser.add_argument("--prepare", action="store_true")
    parser.add_argument(
        "--worker", choices=("parakeet-redux", "parakeet-ultra", "phonon-2")
    )
    parser.add_argument("--device", default="cpu", choices=("cpu", "mps", "mlx"))
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--photon-python", type=Path, default=REPO / ".venv/bin/python")
    parser.add_argument(
        "--phonon-python", type=Path, default=REPO / ".venv/phonon2/bin/python"
    )
    parser.add_argument(
        "--onnx-worker",
        type=Path,
        default=REPO / "target/release/examples/benchmark_resident_onnx",
    )
    parser.add_argument(
        "--onnx-model",
        type=Path,
        default=Path.home()
        / "Library/Application Support/fast-transcript/models/parakeet-tdt-0.6b-v3-int8",
    )
    parser.add_argument(
        "--out-dir", type=Path, default=ROOT / "runs/2026-09-30-m5-max-english"
    )
    args = parser.parse_args()
    if args.repetitions < 1:
        parser.error("--repetitions must be positive")
    if args.prepare:
        prepare(args.manifest)
    elif args.worker:
        worker(args)
    else:
        benchmark(args)


if __name__ == "__main__":
    main()
