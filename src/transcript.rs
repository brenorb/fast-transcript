//! Pure transcript assembly: overlap removal and conversion to timed segments.

use crate::types::TranscriptSegment;

fn normalized_words(text: &str) -> Vec<(String, String)> {
    text.split_whitespace()
        .filter_map(|word| {
            let normalized = word
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase();
            if normalized.is_empty() {
                None
            } else {
                Some((word.to_string(), normalized))
            }
        })
        .collect()
}

fn ends_with_sentence_punctuation(text: &str) -> bool {
    text.trim_end()
        .chars()
        .next_back()
        .is_some_and(|c| matches!(c, '.' | '!' | '?' | ':' | ';'))
}

pub(crate) fn merge_chunk_texts(left: &str, right: &str) -> String {
    let left = left.trim();
    let right = right.trim();
    if left.is_empty() {
        return right.to_string();
    }
    if right.is_empty() {
        return left.to_string();
    }

    let left_words = normalized_words(left);
    let right_words = normalized_words(right);
    if left_words.is_empty() || right_words.is_empty() {
        return format!("{left} {right}");
    }

    let max_overlap = left_words.len().min(right_words.len()).min(64);
    let mut best_overlap = 0usize;
    for overlap in (1..=max_overlap).rev() {
        let left_slice = &left_words[left_words.len() - overlap..];
        let right_slice = &right_words[..overlap];
        let matches = left_slice
            .iter()
            .zip(right_slice.iter())
            .all(|((_, left_norm), (_, right_norm))| left_norm == right_norm);
        if !matches {
            continue;
        }
        if overlap >= 2 || (overlap == 1 && !ends_with_sentence_punctuation(left)) {
            best_overlap = overlap;
            break;
        }
    }

    if best_overlap == 0 {
        return format!("{left} {right}");
    }

    let remaining = right_words[best_overlap..]
        .iter()
        .map(|(original, _)| original.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    if remaining.is_empty() {
        left.to_string()
    } else {
        format!("{left} {remaining}")
    }
}

fn normalized_text(text: &str) -> String {
    normalized_words(text)
        .into_iter()
        .map(|(_, normalized)| normalized)
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn transcript_segments_from_text(
    text: &str,
    start_s: f64,
    end_s: f64,
) -> Vec<TranscriptSegment> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    vec![TranscriptSegment {
        start_s,
        end_s,
        text: text.to_string(),
        speaker: None,
    }]
}

pub(crate) fn transcript_segments_from_transcription(
    transcription: &transcribe_rs::TranscriptionResult,
    fallback_start_s: f64,
    fallback_end_s: f64,
) -> Vec<TranscriptSegment> {
    if let Some(segments) = &transcription.segments {
        let collected = segments
            .iter()
            .map(|segment| TranscriptSegment {
                start_s: segment.start as f64,
                end_s: segment.end as f64,
                text: segment.text.trim().to_string(),
                speaker: None,
            })
            .filter(|segment| !segment.text.is_empty() && segment.end_s > segment.start_s)
            .collect::<Vec<_>>();
        if !collected.is_empty() {
            return collected;
        }
    }

    transcript_segments_from_text(&transcription.text, fallback_start_s, fallback_end_s)
}

pub(crate) fn merge_transcript_segments(
    existing: &mut Vec<TranscriptSegment>,
    incoming: Vec<TranscriptSegment>,
) {
    for segment in incoming {
        if let Some(last) = existing.last_mut() {
            let overlap =
                (last.end_s.min(segment.end_s) - last.start_s.max(segment.start_s)).max(0.0);
            if overlap > 0.0 {
                if normalized_text(&last.text) == normalized_text(&segment.text) {
                    last.end_s = last.end_s.max(segment.end_s);
                    continue;
                }

                let merged_text = merge_chunk_texts(&last.text, &segment.text);
                let concatenated = format!("{} {}", last.text.trim(), segment.text.trim())
                    .trim()
                    .to_string();
                if merged_text != concatenated {
                    last.text = merged_text;
                    last.end_s = last.end_s.max(segment.end_s);
                    continue;
                }
            }
        }
        existing.push(segment);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use transcribe_rs::{TranscriptionResult, TranscriptionSegment};

    #[test]
    fn transcript_boundary_preserves_separate_sentences_and_extends_duplicate_cues() {
        use super::{merge_chunk_texts, merge_transcript_segments, transcript_segments_from_text};
        assert_eq!(merge_chunk_texts("Yes.", "Yes, again."), "Yes. Yes, again.");
        let mut segments = transcript_segments_from_text("Hello world!", 0.0, 2.0);
        merge_transcript_segments(
            &mut segments,
            transcript_segments_from_text("hello WORLD", 1.0, 3.0),
        );
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "Hello world!");
        assert_eq!(segments[0].end_s, 3.0);
    }

    #[test]
    fn merge_chunk_texts_dedups_case_insensitive_overlap() {
        let merged = merge_chunk_texts("Não precisa ser um chefe de", "De cozinha pra entender");
        assert_eq!(merged, "Não precisa ser um chefe de cozinha pra entender");
    }

    #[test]
    fn merge_chunk_texts_keeps_text_when_no_overlap() {
        let merged = merge_chunk_texts("Primeira frase.", "Segunda frase.");
        assert_eq!(merged, "Primeira frase. Segunda frase.");
    }

    #[test]
    fn merge_transcript_segments_dedups_overlapping_boundary_segments() {
        let mut segments = transcript_segments_from_text("chefe de", 0.0, 1.0);
        merge_transcript_segments(
            &mut segments,
            vec![TranscriptSegment {
                start_s: 0.8,
                end_s: 1.8,
                text: "de cozinha".to_string(),
                speaker: None,
            }],
        );
        assert_eq!(
            segments,
            vec![TranscriptSegment {
                start_s: 0.0,
                end_s: 1.8,
                text: "chefe de cozinha".to_string(),
                speaker: None,
            }]
        );
    }

    #[test]
    fn transcript_segments_from_transcription_falls_back_to_full_text_when_segments_missing() {
        let transcription = TranscriptionResult {
            text: "Trecho completo".to_string(),
            segments: Some(vec![TranscriptionSegment {
                start: 0.5,
                end: 0.5,
                text: "   ".to_string(),
            }]),
        };

        assert_eq!(
            transcript_segments_from_transcription(&transcription, 1.0, 3.5),
            vec![TranscriptSegment {
                start_s: 1.0,
                end_s: 3.5,
                text: "Trecho completo".to_string(),
                speaker: None,
            }]
        );
    }
}
