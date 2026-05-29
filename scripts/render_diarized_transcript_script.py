from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import Iterable


SIMPLE_SPEAKER_RE = re.compile(r"^S(\d+)$")
FORMATTED_SPEAKER_RE = re.compile(r"^SPEAKER_(\d+)$")


def normalize_speaker_label(label: str, unknown_label: str = "UNKNOWN") -> str:
    cleaned = label.strip()
    if not cleaned:
        return unknown_label

    formatted_match = FORMATTED_SPEAKER_RE.fullmatch(cleaned)
    if formatted_match:
        return f"SPEAKER_{int(formatted_match.group(1)):02d}"

    simple_match = SIMPLE_SPEAKER_RE.fullmatch(cleaned)
    if simple_match:
        return f"SPEAKER_{int(simple_match.group(1)):02d}"

    return cleaned


def render_script_lines(
    segments: Iterable[dict[str, object]],
    *,
    merge_consecutive: bool = True,
    unknown_label: str = "UNKNOWN",
) -> list[str]:
    lines: list[str] = []
    current_speaker: str | None = None
    current_parts: list[str] = []

    def flush() -> None:
        nonlocal current_speaker, current_parts
        if current_speaker is None or not current_parts:
            current_speaker = None
            current_parts = []
            return
        text = " ".join(part for part in current_parts if part).strip()
        if text:
            lines.append(f"{current_speaker}: {text}")
        current_speaker = None
        current_parts = []

    for raw_segment in segments:
        if not isinstance(raw_segment, dict):
            continue
        text = str(raw_segment.get("text", "")).strip()
        if not text:
            continue

        speaker = normalize_speaker_label(str(raw_segment.get("speaker", "")), unknown_label)

        if merge_consecutive and current_speaker == speaker:
            current_parts.append(text)
            continue

        flush()
        current_speaker = speaker
        current_parts = [text]

    flush()
    return lines


def default_output_path(input_path: Path) -> Path:
    if input_path.suffix == ".json":
        return input_path.with_suffix(".script.txt")
    return input_path.with_name(f"{input_path.name}.script.txt")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Render a diarized transcript JSON into a screenplay-style text file.",
    )
    parser.add_argument("input_json", type=Path)
    parser.add_argument("output_txt", nargs="?", type=Path)
    parser.add_argument("--no-merge-consecutive", action="store_true")
    parser.add_argument("--unknown-speaker-label", default="UNKNOWN")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    input_json = args.input_json.resolve()
    output_txt = (args.output_txt or default_output_path(input_json)).resolve()

    payload = json.loads(input_json.read_text(encoding="utf-8"))
    segments = payload.get("segments", [])
    if not isinstance(segments, list):
        raise SystemExit(f"`segments` is not a list in {input_json}")

    lines = render_script_lines(
        segments,
        merge_consecutive=not args.no_merge_consecutive,
        unknown_label=args.unknown_speaker_label,
    )

    output_txt.parent.mkdir(parents=True, exist_ok=True)
    output_txt.write_text("\n".join(lines) + ("\n" if lines else ""), encoding="utf-8")
    print(output_txt)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
