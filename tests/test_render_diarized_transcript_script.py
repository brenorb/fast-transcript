from __future__ import annotations

import sys
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

from render_diarized_transcript_script import normalize_speaker_label, render_script_lines


class RenderDiarizedTranscriptScriptTests(unittest.TestCase):
    def test_normalize_speaker_label_formats_simple_labels(self) -> None:
        self.assertEqual(normalize_speaker_label("S1"), "SPEAKER_01")
        self.assertEqual(normalize_speaker_label("S12"), "SPEAKER_12")

    def test_normalize_speaker_label_preserves_formatted_labels(self) -> None:
        self.assertEqual(normalize_speaker_label("SPEAKER_2"), "SPEAKER_02")
        self.assertEqual(normalize_speaker_label("SPEAKER_09"), "SPEAKER_09")

    def test_render_script_lines_merges_consecutive_speaker_turns(self) -> None:
        segments = [
            {"speaker": "S1", "text": "Oi."},
            {"speaker": "S1", "text": "Tudo bem?"},
            {"speaker": "S2", "text": "Tudo."},
            {"speaker": "S2", "text": "E você?"},
            {"speaker": "S1", "text": "Também."},
        ]
        self.assertEqual(
            render_script_lines(segments),
            [
                "SPEAKER_01: Oi. Tudo bem?",
                "SPEAKER_02: Tudo. E você?",
                "SPEAKER_01: Também.",
            ],
        )

    def test_render_script_lines_omits_unknown_label_by_default(self) -> None:
        segments = [{"text": "Sem speaker."}]
        self.assertEqual(
            render_script_lines(segments),
            ["Sem speaker."],
        )

    def test_render_script_lines_supports_unknown_speaker_label(self) -> None:
        segments = [{"text": "Sem speaker."}]
        self.assertEqual(
            render_script_lines(segments, unknown_label="SPEAKER_00"),
            ["SPEAKER_00: Sem speaker."],
        )

    def test_render_script_lines_can_keep_each_segment_separate(self) -> None:
        segments = [
            {"speaker": "S1", "text": "Primeira."},
            {"speaker": "S1", "text": "Segunda."},
        ]
        self.assertEqual(
            render_script_lines(segments, merge_consecutive=False),
            [
                "SPEAKER_01: Primeira.",
                "SPEAKER_01: Segunda.",
            ],
        )


if __name__ == "__main__":
    unittest.main()
