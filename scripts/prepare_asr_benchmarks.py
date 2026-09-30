#!/usr/bin/env python3
"""Prepare shared audio partitions and references for the canonical ASR protocol."""

from __future__ import annotations

import argparse
import hashlib
import html
import itertools
import json
import re
import subprocess
import wave
from datetime import datetime, timezone
from pathlib import Path

import numpy as np
from benchmark_transcription_engines import ROOT, normalize


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n")


def partition(path, output_dir):
    """Contiguous sample-exact chunks, with quiet boundaries chosen without captions."""
    with wave.open(str(path)) as audio:
        assert (audio.getframerate(), audio.getnchannels(), audio.getsampwidth()) == (
            16000,
            1,
            2,
        )
        pcm = audio.readframes(audio.getnframes())
    samples = np.frombuffer(pcm, dtype="<i2").astype(np.float64)
    energy = np.concatenate(([0.0], np.cumsum(samples * samples)))
    cuts = [0]
    while len(samples) - cuts[-1] > 31 * 16000:
        positions = np.arange(cuts[-1] + 25 * 16000, cuts[-1] + 30 * 16000 + 1, 160)
        costs = energy[positions + 1600] - energy[positions - 1600]
        cuts.append(int(positions[np.argmin(costs)]))
    cuts.append(len(samples))
    output_dir.mkdir(parents=True, exist_ok=True)
    parts = []
    for index, (start, end) in enumerate(itertools.pairwise(cuts)):
        target = output_dir / f"{index:03d}.wav"
        with wave.open(str(target), "wb") as out:
            out.setnchannels(1)
            out.setsampwidth(2)
            out.setframerate(16000)
            out.writeframes(pcm[start * 2 : end * 2])
        parts.append(
            {
                "path": str(target.resolve()),
                "start_sample": start,
                "end_sample": end,
                "sha256": digest(target),
            }
        )
    assert sum(p["end_sample"] - p["start_sample"] for p in parts) == len(samples)
    return parts, len(samples) / 16000


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--youtube-info", type=Path, required=True)
    parser.add_argument(
        "--ted-audio", type=Path, default=ROOT / "datasets/ted-ken-robinson/source.webm"
    )
    parser.add_argument(
        "--ted-captions",
        type=Path,
        default=ROOT / "datasets/ted-ken-robinson/source.en.json3",
    )
    parser.add_argument("--out-dir", type=Path, default=ROOT / "datasets/unified-v2")
    args = parser.parse_args()
    info = json.loads(args.youtube_info.read_text())
    assert (
        info["id"] == "iG9CE55wbtY" and info["channel_id"] == "UCAuUUnT6oDeKwE6v1NGQxug"
    )
    assert info["subtitles"].get("en"), (
        "Require publisher subtitles, not automatic captions"
    )
    captions = json.loads(args.ted_captions.read_text())
    cues = []
    for event in captions["events"]:
        if "segs" not in event:
            continue
        text = html.unescape("".join(s["utf8"] for s in event["segs"]))
        text = re.sub(r"\((?:Laughter|Applause|Audience)\)", "", text)
        cues.append(
            {
                "start_s": event["tStartMs"] / 1000,
                "end_s": (event["tStartMs"] + event["dDurationMs"]) / 1000,
                "text": " ".join(text.split()),
            }
        )
    start = max(0, cues[0]["start_s"] - 0.5)
    end = max(c["end_s"] for c in cues) + 0.5
    base = args.out_dir.resolve()
    base.mkdir(parents=True, exist_ok=True)
    (base / ".gitignore").write_text("/audio/\n")
    audio_dir = base / "audio"
    audio_dir.mkdir(exist_ok=True)
    ted_wav = audio_dir / "ted-full.wav"
    subprocess.run(
        [
            "ffmpeg",
            "-v",
            "error",
            "-y",
            "-i",
            str(args.ted_audio),
            "-ss",
            str(start),
            "-t",
            str(end - start),
            "-ar",
            "16000",
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
            str(ted_wav),
        ],
        check=True,
    )
    ted_text = " ".join(c["text"] for c in cues if c["text"])
    source = {
        "youtube_url": info["webpage_url"],
        "video_id": info["id"],
        "title": info["title"],
        "channel": info["channel"],
        "channel_id": info["channel_id"],
        "video_duration_seconds": info["duration"],
        "subtitle_source": "YouTube publisher subtitles.en, not automatic_captions",
        "subtitle_sha256": digest(args.ted_captions),
        "downloaded_audio_sha256": digest(args.ted_audio),
        "crop_start_seconds": start,
        "crop_end_seconds": end,
        "caption_cleanup": "Remove only (Laughter), (Applause), (Audience); keep spoken audience responses; preserve wording, spelling and numbers.",
        "reference_type": "publisher-edited captions; may differ from verbatim speech",
        "fetched_at": datetime.now(timezone.utc).isoformat(),
    }
    save(base / "ted-source.json", source)
    save(base / "ted-captions.json", {"source": source, "cues": cues})
    lib_path = ROOT / "datasets/librispeech-validation-clean-50/manifest.json"
    lib = json.loads(lib_path.read_text())
    cases = [
        (
            "pt-15s",
            "pt",
            "original human reference",
            [
                (
                    ROOT / "audio/audio_15s_16k_mono.wav",
                    json.loads((ROOT / "references/15s.json").read_text())["text"],
                    "pt-15s",
                )
            ],
        ),
        (
            "pt-5min",
            "pt",
            "human-reviewed Ultra draft",
            [
                (
                    ROOT / "audio/audio_5min_16k_mono.wav",
                    json.loads((ROOT / "references/5min-human.json").read_text())[
                        "text"
                    ],
                    "pt-5min",
                )
            ],
        ),
        (
            "en-librispeech-50",
            "en",
            "LibriSpeech human dataset references",
            [
                (lib_path.parent / r["audio_path"], r["text"], r["id"])
                for r in lib["utterances"]
            ],
        ),
        (
            "en-ted-ken-robinson",
            "en",
            source["reference_type"],
            [(ted_wav, ted_text, "ted-ken-robinson")],
        ),
    ]
    suite = []
    for case, language, reference_type, items in cases:
        utterances = []
        for path, text, id_ in items:
            parts, seconds = partition(path, audio_dir / case / id_)
            for part in parts:
                part["path"] = str(Path(part["path"]).relative_to(base))
            utterances.append(
                {
                    "id": id_,
                    "text": text,
                    "audio_seconds": seconds,
                    "source_wav_sha256": digest(path),
                    "parts": parts,
                }
            )
        manifest = {
            "protocol": "asr-v2",
            "case": case,
            "language": language,
            "reference_type": reference_type,
            "normalization": "lowercase; remove Unicode punctuation; preserve accents and numerals; collapse whitespace",
            "partition": "Contiguous PCM; choose lowest 200 ms energy boundary between 25–30 s; final remainder <=31 s; no overlap, no dropped samples; identical parts for all engines.",
            "reference_words": sum(
                len(normalize(r["text"]).split()) for r in utterances
            ),
            "audio_seconds": sum(r["audio_seconds"] for r in utterances),
            "utterances": utterances,
        }
        dest = base / f"{case}.json"
        save(dest, manifest)
        suite.append(dest.name)
        print(
            case,
            manifest["audio_seconds"],
            manifest["reference_words"],
            "words",
            sum(len(r["parts"]) for r in utterances),
            "parts",
        )
    save(base / "suite.json", {"protocol": "asr-v2", "manifests": suite})


if __name__ == "__main__":
    main()
