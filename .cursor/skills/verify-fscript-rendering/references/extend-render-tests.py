import sys
from pathlib import Path


source = Path(sys.argv[1] if len(sys.argv) > 1 else "src/output.rs")
original = source.read_text()
test = """
    #[test]
    fn first_unlabeled_turn_keeps_its_start_time() {
        let segments = vec![TranscriptSegment {
            start_s: 5.0,
            end_s: 6.0,
            text: "Opening line".to_string(),
            speaker: None,
        }];
        assert_eq!(
            render_speaker_lines(&segments, SpeakersFormat::Timestamped, true),
            vec!["00:00:05 - Opening line"]
        );
    }
"""
assert "fn first_unlabeled_turn_keeps_its_start_time" not in original
source.write_text(original.rstrip()[:-1] + test + "}\n")
