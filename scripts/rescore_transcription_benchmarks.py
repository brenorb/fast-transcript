#!/usr/bin/env python3
"""Rescore saved transcripts without modifying runs or rerunning inference."""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
from functools import cache
from pathlib import Path

from benchmark_transcription_engines import ROOT, edit_distance, normalize


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--runs-dir", type=Path, default=ROOT / "runs")
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()
    reference = json.loads(args.reference.read_text(encoding="utf-8"))
    if not reference.get("complete") or not reference["text"].strip():
        parser.error("A complete, nonempty reference is required")
    raw_words = reference["text"].split()
    words = normalize(reference["text"]).split()
    if not words:
        parser.error("Reference is empty after normalization")

    @cache
    def score(text: str) -> dict:
        edits = edit_distance(words, normalize(text).split())
        return {
            "reference_words": len(words),
            "word_edits": edits,
            "normalized_wer": edits / len(words),
            "raw_wer": edit_distance(raw_words, text.split()) / len(raw_words),
        }

    rows = []
    for path in sorted(args.runs_dir.rglob("*.json")):
        payload = json.loads(path.read_text(encoding="utf-8"))
        if not isinstance(payload, dict):
            continue
        metrics = payload.get("metrics", {})
        if metrics.get("audio_id") != reference["audio_id"]:
            continue
        result = payload.get("result", {})
        if not isinstance(result.get("text"), str):
            parser.error(f"Missing transcript in {path}")
        rows.append(
            {
                "source_run": str(path.relative_to(args.runs_dir)),
                "source_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                "engine": metrics["engine"],
                "device": metrics["device"],
                "repetition": metrics.get("repetition", 1),
                **score(result["text"]),
            }
        )
    if not rows:
        parser.error("No matching saved transcripts found")
    args.out_dir.mkdir(parents=True, exist_ok=True)
    report = {
        "audio_id": reference["audio_id"],
        "reference_file": args.reference.name,
        "reference_sha256": hashlib.sha256(args.reference.read_bytes()).hexdigest(),
        "reference_type": reference["reference_type"],
        "review_date": reference["review_date"],
        "normalization": "Lowercase; remove Unicode punctuation; collapse whitespace. Preserve accents and numerals.",
        "note": "Scores recomputed from saved transcripts; inference and timing unchanged. Review started from an Ultra draft, not a blind independent transcription.",
        "runs": rows,
    }
    (args.out_dir / "scores.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    with (args.out_dir / "scores.csv").open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=list(rows[0]), lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)
    for row in rows:
        print(
            f"{row['source_run']}: {row['word_edits']}/{row['reference_words']} = {row['normalized_wer']:.4%}"
        )


if __name__ == "__main__":
    main()
