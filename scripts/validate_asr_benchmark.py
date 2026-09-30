#!/usr/bin/env python3
"""Independently validate the canonical ASR measurements with jiwer 4.0.0."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import html
import json
import math
import re
import statistics
import wave
from collections import Counter
from pathlib import Path

import jiwer
from benchmark_transcription_engines import ROOT, normalize

REPO = ROOT.parents[1]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def pcm(path):
    with wave.open(str(path)) as audio:
        assert (audio.getframerate(), audio.getnchannels(), audio.getsampwidth()) == (
            16000,
            1,
            2,
        )
        return audio.readframes(audio.getnframes())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--suite", type=Path, default=ROOT / "datasets/unified-v2/suite.json"
    )
    parser.add_argument(
        "--runs-dir", type=Path, default=ROOT / "runs/2026-09-30-unified-v2"
    )
    args = parser.parse_args()
    suite = json.loads(args.suite.read_text())
    assert suite["protocol"] == "asr-v2"
    manifests = {}
    lib_path = ROOT / "datasets/librispeech-validation-clean-50/manifest.json"
    lib = {
        r["id"]: lib_path.parent / r["audio_path"]
        for r in json.loads(lib_path.read_text())["utterances"]
    }
    sources = {
        "pt-15s": ROOT / "audio/audio_15s_16k_mono.wav",
        "pt-5min": ROOT / "audio/audio_5min_16k_mono.wav",
        "en-ted-ken-robinson": args.suite.parent / "audio/ted-full.wav",
        "pt-tedx-yvonne": args.suite.parent / "audio/pt-tedx-yvonne.wav",
    }
    parts_checked = 0
    for name in suite["manifests"]:
        path = args.suite.parent / name
        m = json.loads(path.read_text())
        manifests[m["case"]] = (path, m)
        assert math.isclose(
            sum(r["audio_seconds"] for r in m["utterances"]), m["audio_seconds"]
        )
        assert (
            sum(len(normalize(r["text"]).split()) for r in m["utterances"])
            == m["reference_words"]
        )
        for row in m["utterances"]:
            combined = []
            offset = 0
            for part in row["parts"]:
                p = path.parent / part["path"]
                assert digest(p) == part["sha256"]
                data = pcm(p)
                assert part["start_sample"] == offset
                assert len(data) // 2 == part["end_sample"] - part["start_sample"]
                assert 0 < len(data) / 2 / 16000 <= 31
                offset = part["end_sample"]
                combined.append(data)
                parts_checked += 1
            assert offset / 16000 == row["audio_seconds"]
            original = (
                lib[row["id"]]
                if m["case"] == "en-librispeech-50"
                else sources[m["case"]]
            )
            assert digest(original) == row["source_wav_sha256"]
            assert b"".join(combined) == pcm(original), (
                "Audio partition drops or duplicates samples"
            )
    if "en-ted-ken-robinson" in manifests:
        captions = json.loads((args.suite.parent / "ted-captions.json").read_text())
        assert (
            " ".join(c["text"] for c in captions["cues"] if c["text"])
            == manifests["en-ted-ken-robinson"][1]["utterances"][0]["text"]
        )
    if "pt-tedx-yvonne" in manifests:
        source = json.loads((args.suite.parent / "tedx-source.json").read_text())
        caption_path = args.suite.parent / source["caption_file"]
        assert digest(caption_path) == source["caption_file_sha256"]
        caption_bytes = gzip.decompress(caption_path.read_bytes())
        assert hashlib.sha256(caption_bytes).hexdigest() == source["subtitle_sha256"]
        payload = json.loads(caption_bytes)
        caption_parts = []
        for event in payload["events"]:
            if "segs" not in event:
                continue
            text = html.unescape(
                "".join(segment.get("utf8", "") for segment in event["segs"])
            )
            text = " ".join(re.sub(r"\[[^\]]+\]", " ", text).split())
            if text:
                caption_parts.append(text)
        caption_text = " ".join(caption_parts)
        assert caption_text == manifests["pt-tedx-yvonne"][1]["utterances"][0]["text"]
    env = json.loads((args.runs_dir / "environment.json").read_text())
    for path, sha in env["input_sha256"].items():
        assert digest(REPO / path) == sha
    summary = json.loads((args.runs_dir / "summary.json").read_text())
    assert summary["protocol"] == "asr-v2"
    records = []
    seen = set()
    scores = []
    errors = {}
    utterances_checked = 0
    for p in sorted(args.runs_dir.glob("*-r*.json")):
        x = json.loads(p.read_text())
        assert x["protocol"] == "asr-v2"
        key = (x["case"], x["engine"], x["device"], x["repetition"])
        assert key not in seen
        seen.add(key)
        path, m = manifests[x["case"]]
        assert x["process_resources"]["max_rss_mb"] > 0
        refs = {r["id"]: r for r in m["utterances"]}
        assert x["manifest_sha256"] == digest(path)
        for phase in ("cold", "warm"):
            data = x[phase]
            rows = data["utterances"]
            assert len(rows) == len(refs) and {r["id"] for r in rows} == set(refs)
            reference = [normalize(refs[r["id"]]["text"]) for r in rows]
            hypothesis = [normalize(r["text"]) for r in rows]
            score = jiwer.process_words(reference, hypothesis)
            assert (
                score.substitutions + score.deletions + score.insertions
                == data["word_edits"]
            )
            assert math.isclose(score.wer, data["wer"], abs_tol=1e-12)
            assert sum(len(r.split()) for r in reference) == x["reference_words"]
            assert data["corpus_seconds"] > 0
            assert (
                sum(part["call_seconds"] for row in rows for part in row["parts"])
                <= data["corpus_seconds"] + 0.0001
            )
            for row in rows:
                a = jiwer.process_words(
                    normalize(refs[row["id"]]["text"]), normalize(row["text"])
                )
                assert a.substitutions + a.deletions + a.insertions == row["word_edits"]
                utterances_checked += 1
                assert len(row["parts"]) == len(refs[row["id"]]["parts"])
                for actual, part in zip(row["parts"], refs[row["id"]]["parts"]):
                    assert actual["call_seconds"] > 0
                    if "truncated" in actual:
                        assert actual["truncated"] is False
                    if "audio_seconds" in actual:
                        assert (
                            abs(
                                actual["audio_seconds"]
                                - (part["end_sample"] - part["start_sample"]) / 16000
                            )
                            < 0.002
                        )
            scores.append(
                {
                    "case": x["case"],
                    "engine": x["engine"],
                    "device": x["device"],
                    "repetition": x["repetition"],
                    "phase": phase,
                    "substitutions": score.substitutions,
                    "deletions": score.deletions,
                    "insertions": score.insertions,
                    "wer": score.wer,
                }
            )
            if phase == "warm" and x["repetition"] == 1:
                edits = []
                for idx, alignments in enumerate(score.alignments):
                    for a in alignments:
                        if a.type != "equal":
                            edits.append(
                                {
                                    "id": rows[idx]["id"],
                                    "type": a.type,
                                    "reference": " ".join(
                                        score.references[idx][
                                            a.ref_start_idx : a.ref_end_idx
                                        ]
                                    ),
                                    "hypothesis": " ".join(
                                        score.hypotheses[idx][
                                            a.hyp_start_idx : a.hyp_end_idx
                                        ]
                                    ),
                                }
                            )
                errors["/".join(key[:3])] = edits
        assert x["cold"]["parent_received_seconds"] >= x["cold"]["corpus_seconds"]
        assert (
            x["warm"]["parent_received_seconds"] > x["cold"]["parent_received_seconds"]
        )
        assert (
            x["process_two_pass_wall_seconds"] >= x["warm"]["parent_received_seconds"]
        )
        records.append(x)
    expected_configs = {
        ("onnx", "cpu"),
        ("parakeet-redux", "cpu"),
        ("parakeet-redux", "mps"),
        ("parakeet-ultra", "cpu"),
        ("parakeet-ultra", "mps"),
        ("phonon-2", "cpu"),
        ("phonon-2", "mlx"),
    }
    assert seen == {
        (case, engine, device, rep)
        for case in manifests
        for engine, device in expected_configs
        for rep in range(1, env["repetitions"] + 1)
    }
    assert len(summary["results"]) == len(manifests) * len(expected_configs)
    for s in summary["results"]:
        rows = [
            r
            for r in records
            if (r["case"], r["engine"], r["device"])
            == (s["case"], s["engine"], s["device"])
        ]
        assert len(rows) == s["repetitions"] == env["repetitions"]
        fields = {
            "cold_result_seconds": [r["cold"]["parent_received_seconds"] for r in rows],
            "warm_transcribe_seconds": [r["warm"]["corpus_seconds"] for r in rows],
            "cold_wer": [r["cold"]["wer"] for r in rows],
            "warm_wer": [r["warm"]["wer"] for r in rows],
            "peak_rss_mb": [r["process_resources"]["max_rss_mb"] for r in rows],
        }
        for field, values in fields.items():
            assert (
                statistics.median(values) == s[field]
                and [min(values), max(values)] == s[field + "_range"]
            )
        assert (
            s["audio_seconds"] / s["cold_result_seconds"] == s["cold_realtime_speedup"]
        )
        assert (
            s["audio_seconds"] / s["warm_transcribe_seconds"]
            == s["warm_realtime_speedup"]
        )
        stable = all(
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
        assert stable == s["transcripts_stable"]
    for name, value in [
        ("verified-scores.json", scores),
        ("error-alignments.json", errors),
    ]:
        (args.runs_dir / name).write_text(
            json.dumps(value, indent=2, ensure_ascii=False) + "\n"
        )
    report = {
        "validator": "jiwer 4.0.0",
        "protocol": "asr-v2",
        "process_repetitions_checked": len(records),
        "corpus_passes_checked": 2 * len(records),
        "utterances_checked": utterances_checked,
        "audio_partitions_checked": parts_checked,
        "reference_counts": {
            case: m["reference_words"] for case, (_, m) in manifests.items()
        },
        "checks": [
            "input/script/binary fingerprints",
            "original audio hashes",
            "sample-exact partition reconstruction",
            "caption/reference equality",
            "all case/device/repetition combinations present",
            "per-utterance and corpus WER",
            "per-part duration and truncation",
            "cold/warm timing ordering",
            "timing sums, medians and ranges",
            "per-process peak RSS and its median/range",
            "reported transcript stability",
        ],
        "validator_sha256": digest(Path(__file__)),
    }
    (args.runs_dir / "validation.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    print("WER by case:", dict(Counter(r["case"] for r in scores)))


if __name__ == "__main__":
    main()
