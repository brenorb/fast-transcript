# Rendering diarized transcripts as screenplay text

Use [`scripts/render_diarized_transcript_script.py`](../scripts/render_diarized_transcript_script.py) when you already have a diarized JSON transcript and want a simpler speaker-block text file.

It expects the segment-oriented JSON that `fscript --json` writes.
It renders only `segments`; it does not fall back to the top-level `text` field.

## Usage

```bash
python3 scripts/render_diarized_transcript_script.py lecture.transcript.json
python3 scripts/render_diarized_transcript_script.py lecture.transcript.json notes/script.txt
python3 scripts/render_diarized_transcript_script.py lecture.transcript.json --unknown-speaker-label SPEAKER_00
python3 scripts/render_diarized_transcript_script.py lecture.transcript.json --no-merge-consecutive
```

## Defaults

- default output path:
  - `lecture.transcript.json` -> `lecture.transcript.script.txt`
  - `lecture.json` -> `lecture.script.txt`
- consecutive segments from the same speaker are merged into one line
- `S1`, `S12`, and `SPEAKER_2` are normalized to `SPEAKER_01`, `SPEAKER_12`, and `SPEAKER_02`
- unlabeled segments stay as plain text by default; there is no synthetic `UNKNOWN:` prefix unless you opt in with `--unknown-speaker-label`
- parent directories for the output file are created automatically
- the script prints the final output path to stdout after writing the file

## Notes

- `--no-merge-consecutive` keeps every segment as its own line even when the speaker does not change
- segments without text are skipped
- non-dict entries inside `segments` are ignored
