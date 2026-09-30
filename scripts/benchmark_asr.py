#!/usr/bin/env python3
"""Canonical ASR protocol: identical data, cold result latency and warm call time."""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
import platform
import shutil
import signal
import statistics
import subprocess
import threading
import time
from datetime import datetime, timezone
from functools import cache
from importlib.metadata import version
from pathlib import Path

from benchmark_transcription_engines import ROOT, edit_distance, normalize, time_metrics

REPO = ROOT.parents[1]
PROTOCOL = "asr-v2"


def storage_bytes(paths):
    """Count resident bytes once per inode, following model-cache symlinks."""
    seen = set()
    total = 0
    for root in paths:
        if not root.exists():
            continue
        items = [root]
        if root.is_dir():
            items.extend(p for p in root.rglob("*") if not p.is_dir())
        for path in items:
            try:
                stat = path.stat()
            except OSError:
                continue
            identity = (stat.st_dev, stat.st_ino)
            if identity not in seen:
                seen.add(identity)
                total += stat.st_size
    return total


def resource_snapshot(data_root):
    home = Path.home()
    models = {
        "onnx-parakeet-tdt-0.6b-v3-int8": [
            home
            / "Library/Application Support/fast-transcript/models/parakeet-tdt-0.6b-v3-int8"
        ],
        "parakeet-redux": [
            home / ".cache/huggingface/hub/models--moondream--parakeet-redux"
        ],
        "parakeet-ultra": [
            home / ".cache/huggingface/hub/models--moondream--parakeet-ultra"
        ],
        "phonon-2": [
            home / ".cache/fermion/speech/FermionResearch__Phonon-2",
            home / ".cache/huggingface/hub/models--FermionResearch--Phonon-2",
        ],
    }
    memory_bytes = int(
        subprocess.check_output(["sysctl", "-n", "hw.memsize"], text=True)
    )
    disk = shutil.disk_usage(REPO)
    return {
        "captured_at": datetime.now(timezone.utc).isoformat(),
        "physical_memory_bytes": memory_bytes,
        "disk_volume_total_bytes": disk.total,
        "disk_volume_free_bytes": disk.free,
        "model_storage_bytes": {
            name: storage_bytes(paths) for name, paths in models.items()
        },
        "benchmark_dataset_bytes": storage_bytes([data_root]),
        "method": "Persistent model-cache files only; symlinks followed and duplicate inodes counted once per model family. Peak worker RSS is collected separately by /usr/bin/time -l for each fresh process across both passes.",
    }


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def save(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n")


@cache
def score(reference, text):
    return edit_distance(normalize(reference).split(), normalize(text).split())


def worker(args):
    manifest = json.loads(args.manifest.read_text())
    if args.worker == "phonon-2":
        os.environ["FERMION_DEVICE"] = args.device
        from fermion._speech import backends, fetch
        from fermion.transcribe import _resolve

        repo, key, pin, local = _resolve("phonon-2")
        device = backends.resolve("ASR benchmark")
        assert device == args.device
        backends.require_engine_for(device, pin["backend"])
        directory = local if local is not None else fetch.ensure(repo, key, pin)
        model = backends.load(
            device, directory, profile=key, backend=pin["backend"], quiet=True
        )
        metadata = {
            "model": repo,
            "pin": pin,
            "runtime_versions": {
                name: version(name) for name in ("fermion-research", "mlx", "torch")
            },
        }

        def transcribe(path):
            result = model.transcribe_detailed(str(path))
            text, decode, seconds = result.triple()
            if result.truncated:
                raise RuntimeError(f"Truncated output for {path}")
            return {
                "text": text,
                "decode_seconds_diagnostic": decode,
                "audio_seconds": seconds,
                "truncated": False,
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
                name: version(name) for name in ("moondream", "kestrel", "torch")
            },
        }

        def transcribe(path):
            result = model.transcribe(audio=path, timestamps="segment")
            if args.device == "mps":
                torch.mps.synchronize()
            return {"text": result["text"]}

    for phase in ("cold", "warm"):
        started = time.perf_counter()
        utterances = []
        for row in manifest["utterances"]:
            parts = []
            for part in row["parts"]:
                start = time.perf_counter()
                result = transcribe(args.manifest.parent / part["path"])
                result["call_seconds"] = time.perf_counter() - start
                parts.append(result)
            utterances.append(
                {
                    "id": row["id"],
                    "text": " ".join(p["text"].strip() for p in parts),
                    "parts": parts,
                }
            )
        seconds = time.perf_counter() - started
        print(
            json.dumps(
                {
                    "phase": phase,
                    "corpus_seconds": seconds,
                    "utterances": utterances,
                    **metadata,
                }
            ),
            flush=True,
        )
    if args.worker != "phonon-2":
        context.__exit__(None, None, None)


def execute(command, log_path):
    """Measure process launch to first complete result, without conflating teardown."""
    events = []
    with log_path.open("w") as log:
        start = time.perf_counter()
        proc = subprocess.Popen(
            ["/usr/bin/time", "-l", *command],
            stdout=subprocess.PIPE,
            stderr=log,
            text=True,
            start_new_session=True,
        )
        timer = threading.Timer(1800, lambda: os.killpg(proc.pid, signal.SIGKILL))
        timer.start()
        try:
            for line in proc.stdout:
                received = time.perf_counter() - start
                event = json.loads(line)
                event["parent_received_seconds"] = received
                events.append(event)
            code = proc.wait()
        except BaseException:
            if proc.poll() is None:
                os.killpg(proc.pid, signal.SIGKILL)
            proc.wait()
            raise
        finally:
            timer.cancel()
            proc.stdout.close()
        end = time.perf_counter() - start
    if code or [e["phase"] for e in events] != ["cold", "warm"]:
        raise RuntimeError(f"Worker failed: {command}\n{log_path.read_text()}")
    return {
        "cold": events[0],
        "warm": events[1],
        "process_two_pass_wall_seconds": end,
        "process_resources": time_metrics(log_path.read_text()),
    }


def summarize(records, out_dir):
    groups = {}
    for row in records:
        groups.setdefault((row["case"], row["engine"], row["device"]), []).append(row)
    summaries = []
    for (case, engine, device), rows in groups.items():
        fields = {
            "cold_result_seconds": [r["cold"]["parent_received_seconds"] for r in rows],
            "warm_transcribe_seconds": [r["warm"]["corpus_seconds"] for r in rows],
            "cold_wer": [r["cold"]["wer"] for r in rows],
            "warm_wer": [r["warm"]["wer"] for r in rows],
        }
        entry = {
            "case": case,
            "engine": engine,
            "device": device,
            "repetitions": len(rows),
            "audio_seconds": rows[0]["audio_seconds"],
            "reference_words": rows[0]["reference_words"],
            "peak_rss_mb": statistics.median(
                r["process_resources"]["max_rss_mb"] for r in rows
            ),
            "peak_rss_mb_range": [
                min(r["process_resources"]["max_rss_mb"] for r in rows),
                max(r["process_resources"]["max_rss_mb"] for r in rows),
            ],
        }
        for name, values in fields.items():
            entry[name] = statistics.median(values)
            entry[name + "_range"] = [min(values), max(values)]
        entry["cold_realtime_speedup"] = (
            entry["audio_seconds"] / entry["cold_result_seconds"]
        )
        entry["warm_realtime_speedup"] = (
            entry["audio_seconds"] / entry["warm_transcribe_seconds"]
        )
        entry["cold_word_edits"] = [r["cold"]["word_edits"] for r in rows]
        entry["warm_word_edits"] = [r["warm"]["word_edits"] for r in rows]
        entry["transcripts_stable"] = all(
            len(
                {
                    u["text"]
                    for r in rows
                    for phase in ("cold", "warm")
                    for u in r[phase]["utterances"]
                    if u["id"] == id_
                }
            )
            == 1
            for id_ in [u["id"] for u in rows[0]["cold"]["utterances"]]
        )
        summaries.append(entry)
    save(out_dir / "summary.json", {"protocol": PROTOCOL, "results": summaries})
    if summaries:
        with (out_dir / "results.csv").open("w") as file:
            writer = csv.DictWriter(
                file, fieldnames=list(summaries[0]), lineterminator="\n"
            )
            writer.writeheader()
            writer.writerows(summaries)


def benchmark(args):
    suite = json.loads(args.suite.read_text())
    assert suite["protocol"] == PROTOCOL
    manifests = [(args.suite.parent / name).resolve() for name in suite["manifests"]]
    if args.cases:
        manifests = [m for m in manifests if m.stem in args.cases]
        assert {m.stem for m in manifests} == set(args.cases)
    assert manifests
    # Start with the new long-form benchmark, then rerun existing cases.
    manifests.sort(key=lambda p: (p.stem != "en-ted-ken-robinson", p.stem))
    args.out_dir.mkdir(parents=True, exist_ok=True)
    fingerprint = {
        str(p.relative_to(REPO)): digest(p)
        for p in [
            Path(__file__).resolve(),
            REPO / "examples/benchmark_asr_onnx.rs",
            REPO / "scripts/prepare_asr_benchmarks.py",
            REPO / "scripts/prepare_tedx_pt_benchmark.py",
            REPO / "scripts/benchmark_transcription_engines.py",
            args.onnx_worker,
            *manifests,
        ]
    }
    env_path = args.out_dir / "environment.json"
    if args.resume and env_path.exists():
        assert json.loads(env_path.read_text())["input_sha256"] == fingerprint, (
            "Inputs changed; choose a new output directory"
        )
    else:
        assert not list(args.out_dir.glob("*-r*.json")), (
            "Existing measurements: use --resume or a new output directory"
        )
        save(
            env_path,
            {
                "protocol": PROTOCOL,
                "started_at": datetime.now(timezone.utc).isoformat(),
                "hardware": subprocess.check_output(
                    ["sysctl", "-n", "machdep.cpu.brand_string"], text=True
                ).strip(),
                "platform": platform.platform(),
                "input_sha256": fingerprint,
                "source_commit": subprocess.check_output(
                    ["git", "rev-parse", "HEAD"], text=True
                ).strip(),
                "repetitions": args.repetitions,
                "cold_definition": "Parent perf_counter from spawning worker until receiving complete first corpus result. Includes imports/model initialization/read/decode/result serialization; excludes downloads, preprocessing and process teardown. Filesystem/model caches populated.",
                "warm_definition": "Same worker, same resident model, immediate second complete corpus pass. Full corpus call timer includes WAV reads, all parts, text concatenation and GPU synchronization; excludes load and serialization. First pass is warmup.",
                "score_definition": "Unicode punctuation removal + lowercase; preserve accents/numerals. Sum utterance edit distances / sum reference words. Same references and parts for every engine.",
                "ordering": "Sequential cases, then configurations, then three fresh-worker repetitions; no concurrent model workers.",
                "system_resources": resource_snapshot(args.suite.parent),
            },
        )
    configurations = [("onnx", "cpu")] + [
        (model, device)
        for model in ("parakeet-redux", "parakeet-ultra", "phonon-2")
        for device in (("cpu", "mlx") if model == "phonon-2" else ("cpu", "mps"))
    ]
    if args.engines:
        configurations = [c for c in configurations if c[0] in args.engines]
    records = []
    for path in manifests:
        manifest = json.loads(path.read_text())
        refs = {r["id"]: r for r in manifest["utterances"]}
        words = sum(len(normalize(r["text"]).split()) for r in refs.values())
        assert words == manifest["reference_words"]
        for row in refs.values():
            for part in row["parts"]:
                assert digest(path.parent / part["path"]) == part["sha256"]
        for engine, device in configurations:
            if engine == "onnx":
                command = [str(args.onnx_worker), str(path), str(args.onnx_model)]
            else:
                python = (
                    args.phonon_python if engine == "phonon-2" else args.photon_python
                )
                command = [
                    str(python),
                    str(Path(__file__).resolve()),
                    "--worker",
                    engine,
                    "--device",
                    device,
                    "--manifest",
                    str(path),
                ]
            for rep in range(1, args.repetitions + 1):
                name = f"{manifest['case']}-{engine}-{device}-r{rep}"
                dest = args.out_dir / (name + ".json")
                if args.resume and dest.exists():
                    result = json.loads(dest.read_text())
                else:
                    print(f"Running {name}", flush=True)
                    result = execute(command, args.out_dir / (name + ".log"))
                    result.update(
                        protocol=PROTOCOL,
                        case=manifest["case"],
                        engine=engine,
                        device=device,
                        repetition=rep,
                        audio_seconds=manifest["audio_seconds"],
                        reference_words=words,
                        manifest_sha256=digest(path),
                    )
                    for phase in ("cold", "warm"):
                        data = result[phase]
                        assert len(data["utterances"]) == len(refs) and {
                            r["id"] for r in data["utterances"]
                        } == set(refs)
                        for row in data["utterances"]:
                            ref = refs[row["id"]]
                            assert len(row["parts"]) == len(ref["parts"])
                            for output, part in zip(row["parts"], ref["parts"]):
                                if "audio_seconds" in output:
                                    assert (
                                        abs(
                                            output["audio_seconds"]
                                            - (
                                                part["end_sample"]
                                                - part["start_sample"]
                                            )
                                            / 16000
                                        )
                                        < 0.002
                                    )
                            row["word_edits"] = score(ref["text"], row["text"])
                        data["word_edits"] = sum(
                            r["word_edits"] for r in data["utterances"]
                        )
                        data["wer"] = data["word_edits"] / words
                    save(dest, result)
                    print(
                        f"  cold {result['cold']['parent_received_seconds']:.3f}s; warm {result['warm']['corpus_seconds']:.3f}s; WER {100 * result['warm']['wer']:.2f}%",
                        flush=True,
                    )
                records.append(result)
                summarize(records, args.out_dir)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--suite", type=Path, default=ROOT / "datasets/unified-v2/suite.json"
    )
    parser.add_argument(
        "--out-dir", type=Path, default=ROOT / "runs/2026-09-30-unified-v2"
    )
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--cases", nargs="+")
    parser.add_argument(
        "--engines",
        nargs="+",
        choices=("onnx", "parakeet-redux", "parakeet-ultra", "phonon-2"),
    )
    parser.add_argument(
        "--worker", choices=("parakeet-redux", "parakeet-ultra", "phonon-2")
    )
    parser.add_argument("--device", choices=("cpu", "mps", "mlx"), default="cpu")
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--photon-python", type=Path, default=REPO / ".venv/bin/python")
    parser.add_argument(
        "--phonon-python", type=Path, default=REPO / ".venv/phonon2/bin/python"
    )
    parser.add_argument(
        "--onnx-worker",
        type=Path,
        default=REPO / "target/release/examples/benchmark_asr_onnx",
    )
    parser.add_argument(
        "--onnx-model",
        type=Path,
        default=Path.home()
        / "Library/Application Support/fast-transcript/models/parakeet-tdt-0.6b-v3-int8",
    )
    args = parser.parse_args()
    if args.repetitions < 1:
        parser.error("--repetitions must be positive")
    if args.worker:
        if args.manifest is None:
            parser.error("--worker requires --manifest")
        worker(args)
    else:
        benchmark(args)


if __name__ == "__main__":
    main()
