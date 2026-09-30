#!/usr/bin/env python3
"""Prepare the Portuguese TEDx case for the canonical ASR v2 runner."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import html
import json
import re
import subprocess
import wave
from datetime import datetime, timezone
from pathlib import Path

from benchmark_asr import REPO
from benchmark_asr import digest, save
from benchmark_transcription_engines import normalize
from prepare_asr_benchmarks import partition

EXPECTED_ID = "K__5PBtLrgM"
EXPECTED_CHANNEL_ID = "UCsT0YIqwnpJCM-mx7-gSA4Q"
CASE = "pt-tedx-yvonne"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--youtube-info", type=Path, required=True)
    parser.add_argument("--audio", type=Path, required=True)
    parser.add_argument("--captions", type=Path, required=True)
    parser.add_argument(
        "--out-dir",
        type=Path,
        default=REPO / "experiments/transcription_benchmarks/datasets/tedx-yvonne",
    )
    args = parser.parse_args()

    info = json.loads(args.youtube_info.read_text())
    assert info["id"] == EXPECTED_ID and info["channel_id"] == EXPECTED_CHANNEL_ID
    assert not info.get("subtitles"), "Unexpected publisher subtitles; record the exact source track"
    assert info.get("automatic_captions", {}).get("pt-orig") or info.get(
        "automatic_captions", {}
    ).get("pt"), "The reference must be a YouTube-generated Portuguese caption track"

    caption_bytes = args.captions.read_bytes()
    payload = json.loads(caption_bytes)
    media_seconds = float(
        subprocess.check_output(
            [
                "ffprobe", "-v", "error", "-show_entries", "format=duration",
                "-of", "default=noprint_wrappers=1:nokey=1", str(args.audio),
            ],
            text=True,
        ).strip()
    )
    caption_start = min(
        event["tStartMs"] / 1000
        for event in payload["events"]
        if "segs" in event
        and "".join(segment.get("utf8", "") for segment in event["segs"]).strip()
    )
    caption_end = max(
        (event["tStartMs"] + event.get("dDurationMs", 0)) / 1000
        for event in payload["events"]
        if "segs" in event
        and "".join(segment.get("utf8", "") for segment in event["segs"]).strip()
    )
    cues = []
    for event in payload["events"]:
        if "segs" not in event:
            continue
        if event["tStartMs"] / 1000 >= media_seconds:
            continue
        text = html.unescape("".join(segment.get("utf8", "") for segment in event["segs"]))
        text = " ".join(re.sub(r"\[[^\]]+\]", " ", text).split())
        if text:
            cues.append(
                {
                    "start_s": event["tStartMs"] / 1000,
                    "end_s": (event["tStartMs"] + event.get("dDurationMs", 0)) / 1000,
                    "text": text,
                }
            )
    assert cues, "No caption cues found"
    start = max(0.0, caption_start - 0.5)
    end = min(caption_end + 0.5, media_seconds)
    args.out_dir.mkdir(parents=True, exist_ok=True)
    (args.out_dir / ".gitignore").write_text("/source.*\n/audio/\n")
    caption_path = args.out_dir / "caption_track.json3.gz"
    with caption_path.open("wb") as compressed:
        with gzip.GzipFile(fileobj=compressed, mode="wb", mtime=0) as stream:
            stream.write(caption_bytes)
    audio_dir = args.out_dir / "audio"
    audio_dir.mkdir(exist_ok=True)
    wav_path = audio_dir / f"{CASE}.wav"
    subprocess.run(
        [
            "ffmpeg", "-v", "error", "-y", "-i", str(args.audio), "-ss", str(start),
            "-t", str(end - start), "-ar", "16000", "-ac", "1", "-c:a",
            "pcm_s16le", str(wav_path),
        ],
        check=True,
    )
    text = " ".join(cue["text"] for cue in cues)
    with wave.open(str(wav_path), "rb") as audio:
        seconds = audio.getnframes() / audio.getframerate()
    parts, part_seconds = partition(wav_path, audio_dir / CASE)
    assert abs(seconds - part_seconds) < 1e-8
    for part in parts:
        part["path"] = str(Path(part["path"]).relative_to(args.out_dir.resolve()))

    source = {
        "youtube_url": info["webpage_url"],
        "video_id": info["id"],
        "title": info["title"],
        "channel": info["channel"],
        "channel_id": info["channel_id"],
        "video_duration_seconds": info["duration"],
        "subtitle_track": "pt-orig",
        "subtitle_source": "YouTube automatic Portuguese captions; no publisher subtitle tracks were available",
        "caption_file": caption_path.name,
        "caption_file_sha256": digest(caption_path),
        "subtitle_sha256": hashlib.sha256(caption_bytes).hexdigest(),
        "downloaded_audio_sha256": digest(args.audio),
        "crop_start_seconds": start,
        "crop_end_seconds": start + seconds,
        "reference_type": "YouTube automatic captions, not human-edited ground truth",
        "caption_cleanup": "Remove bracketed non-speech labels such as [Aplausos] and [Música]; concatenate remaining non-empty timed cues, preserving words and disfluencies as emitted.",
        "fetched_at": datetime.now(timezone.utc).isoformat(),
    }
    save(args.out_dir / "tedx-source.json", source)
    manifest = {
        "protocol": "asr-v2",
        "case": CASE,
        "language": "pt",
        "reference_type": source["reference_type"],
        "normalization": "lowercase; remove Unicode punctuation; preserve accents and numerals; collapse whitespace",
        "partition": "Contiguous PCM; choose lowest 200 ms energy boundary between 25–30 s; final remainder <=31 s; no overlap, no dropped samples; identical parts for all engines.",
        "reference_words": len(normalize(text).split()),
        "audio_seconds": seconds,
        "utterances": [
            {
                "id": CASE,
                "text": text,
                "audio_seconds": seconds,
                "source_wav_sha256": digest(wav_path),
                "parts": parts,
            }
        ],
    }
    save(args.out_dir / f"{CASE}.json", manifest)
    save(args.out_dir / "suite.json", {"protocol": "asr-v2", "manifests": [f"{CASE}.json"]})
    print(f"{CASE}: {seconds:.3f}s, {manifest['reference_words']} caption words, {len(parts)} parts")


if __name__ == "__main__":
    main()
