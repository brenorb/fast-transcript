//! Execute inference on normalized samples, independent of model loading and media I/O.

use crate::engine::SpeechEngine;
use crate::progress::ChunkProgressReporter;
use crate::transcript::{
    merge_chunk_texts, merge_transcript_segments, transcript_segments_from_transcription,
};
use crate::types::{BenchmarkChunk, TranscriptSegment};
use anyhow::{bail, Context, Result};
use std::time::Instant;

pub(crate) struct InferenceOutcome {
    pub(crate) text: String,
    pub(crate) chunks: Vec<BenchmarkChunk>,
    pub(crate) segments: Vec<TranscriptSegment>,
    pub(crate) transcribe_seconds: f64,
}

pub(crate) fn build_chunk_ranges(
    total_samples: usize,
    sample_rate: usize,
    chunk_seconds: f64,
    chunk_overlap_seconds: f64,
) -> Result<Vec<(usize, usize)>> {
    let chunk_samples = (chunk_seconds * sample_rate as f64).round() as usize;
    let overlap_samples = (chunk_overlap_seconds * sample_rate as f64).round() as usize;
    if chunk_samples == 0 {
        bail!("chunk size rounded to zero samples");
    }
    if overlap_samples >= chunk_samples {
        bail!("overlap size rounded to chunk size or larger");
    }
    let mut ranges = Vec::new();
    let mut start = 0usize;
    let step = chunk_samples - overlap_samples;
    while start < total_samples {
        let end = (start + chunk_samples).min(total_samples);
        ranges.push((start, end));
        if end >= total_samples {
            break;
        }
        start += step;
    }
    Ok(ranges)
}

pub(crate) fn transcribe_samples(
    engine: &mut dyn SpeechEngine,
    samples: &[f32],
    chunk_seconds: Option<f64>,
    chunk_overlap_seconds: f64,
) -> Result<InferenceOutcome> {
    let ranges = match chunk_seconds {
        Some(seconds) => build_chunk_ranges(
            samples.len(),
            crate::SAMPLE_RATE,
            seconds,
            chunk_overlap_seconds,
        )?,
        None => vec![(0, samples.len())],
    };
    let progress = chunk_seconds.map(|_| ChunkProgressReporter::start(ranges.len()));
    if progress.is_none() {
        eprintln!("transcribing...");
    }
    let mut outcome = InferenceOutcome {
        text: String::new(),
        chunks: Vec::with_capacity(ranges.len()),
        segments: Vec::new(),
        transcribe_seconds: 0.0,
    };
    for (index, (start, end)) in ranges.into_iter().enumerate() {
        if let Some(progress) = &progress {
            progress.set_current_chunk(index + 1);
        }
        let started = Instant::now();
        let mut transcription = engine.transcribe(&samples[start..end]).with_context(|| {
            if chunk_seconds.is_some() {
                format!("failed chunk {index} ({start}..{end})")
            } else {
                "failed to transcribe audio".to_string()
            }
        })?;
        let transcribe_seconds = started.elapsed().as_secs_f64();
        outcome.transcribe_seconds += transcribe_seconds;
        let start_s = start as f64 / crate::SAMPLE_RATE as f64;
        let end_s = end as f64 / crate::SAMPLE_RATE as f64;
        if chunk_seconds.is_some() {
            transcription.offset_timestamps(start as f32 / crate::SAMPLE_RATE as f32);
        }
        let text = transcription.text.trim().to_string();
        outcome.text = merge_chunk_texts(&outcome.text, &text);
        let segments = transcript_segments_from_transcription(&transcription, start_s, end_s);
        if chunk_seconds.is_some() {
            merge_transcript_segments(&mut outcome.segments, segments);
        } else {
            outcome.segments = segments;
        }
        outcome.chunks.push(BenchmarkChunk {
            index,
            start_s,
            end_s,
            audio_seconds: (end - start) as f64 / crate::SAMPLE_RATE as f64,
            transcribe_seconds,
            text,
        });
    }
    if let Some(progress) = progress {
        progress.finish();
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use transcribe_rs::{TranscriptionResult, TranscriptionSegment};
    #[test]
    fn whole_audio_preserves_backend_cues_without_chunk_overlap_merging() {
        struct Engine;
        impl SpeechEngine for Engine {
            fn transcribe(&mut self, _: &[f32]) -> Result<TranscriptionResult> {
                Ok(TranscriptionResult {
                    text: "Hello hello".into(),
                    segments: Some(vec![
                        TranscriptionSegment {
                            start: 0.0,
                            end: 1.5,
                            text: "Hello".into(),
                        },
                        TranscriptionSegment {
                            start: 1.0,
                            end: 2.0,
                            text: "hello".into(),
                        },
                    ]),
                })
            }
        }
        let result =
            transcribe_samples(&mut Engine, &vec![0.0; 2 * crate::SAMPLE_RATE], None, 0.0).unwrap();
        assert_eq!(result.segments.len(), 2);
        assert_eq!(result.text, "Hello hello");
    }

    #[test]
    fn whole_audio_inference_uses_one_call_and_falls_back_to_audio_duration() {
        struct Engine {
            calls: usize,
        }
        impl crate::engine::SpeechEngine for Engine {
            fn transcribe(&mut self, samples: &[f32]) -> anyhow::Result<TranscriptionResult> {
                assert_eq!(samples.len(), 2 * crate::SAMPLE_RATE);
                self.calls += 1;
                Ok(TranscriptionResult {
                    text: "  Whole audio.  ".into(),
                    segments: None,
                })
            }
        }
        let mut engine = Engine { calls: 0 };
        let outcome =
            super::transcribe_samples(&mut engine, &vec![0.0; 2 * crate::SAMPLE_RATE], None, 0.0)
                .unwrap();
        assert_eq!(engine.calls, 1);
        assert_eq!(outcome.text, "Whole audio.");
        assert_eq!(outcome.chunks.len(), 1);
        assert_eq!(outcome.chunks[0].audio_seconds, 2.0);
        assert_eq!(outcome.segments[0].start_s, 0.0);
        assert_eq!(outcome.segments[0].end_s, 2.0);
        assert_eq!(
            outcome.transcribe_seconds,
            outcome.chunks[0].transcribe_seconds
        );
    }

    #[test]
    fn inference_failure_retains_context_in_both_modes() {
        struct Engine {
            calls: usize,
        }
        impl crate::engine::SpeechEngine for Engine {
            fn transcribe(&mut self, _: &[f32]) -> anyhow::Result<TranscriptionResult> {
                self.calls += 1;
                if self.calls == 2 {
                    anyhow::bail!("backend failed");
                }
                Ok(TranscriptionResult {
                    text: "first".into(),
                    segments: None,
                })
            }
        }
        let samples = vec![0.0; 2 * crate::SAMPLE_RATE];
        for (chunk_seconds, initial_calls, context) in [
            (Some(1.0), 0, "failed chunk 1"),
            (None, 1, "failed to transcribe audio"),
        ] {
            let error = super::transcribe_samples(
                &mut Engine {
                    calls: initial_calls,
                },
                &samples,
                chunk_seconds,
                0.0,
            )
            .err()
            .unwrap();
            let message = format!("{error:#}");
            assert!(message.contains(context), "{message}");
            assert!(message.contains("backend failed"), "{message}");
        }
    }

    #[test]
    fn chunked_inference_reuses_engine_and_offsets_each_result() {
        struct Engine {
            calls: usize,
        }
        impl crate::engine::SpeechEngine for Engine {
            fn transcribe(&mut self, samples: &[f32]) -> anyhow::Result<TranscriptionResult> {
                assert_eq!(samples.len(), 3 * crate::SAMPLE_RATE);
                self.calls += 1;
                Ok(TranscriptionResult {
                    text: format!("chunk {}.", self.calls),
                    segments: Some(vec![TranscriptionSegment {
                        start: 0.25,
                        end: 1.0,
                        text: format!("chunk {}.", self.calls),
                    }]),
                })
            }
        }
        let mut engine = Engine { calls: 0 };
        let samples = vec![0.0; 5 * crate::SAMPLE_RATE];
        let InferenceOutcome {
            text,
            chunks,
            segments,
            ..
        } = super::transcribe_samples(&mut engine, &samples, Some(3.0), 1.0).unwrap();
        assert_eq!(engine.calls, 2);
        assert_eq!(text, "chunk 1. chunk 2.");
        assert_eq!(chunks[1].start_s, 2.0);
        assert_eq!(segments[1].start_s, 2.25);
        assert_eq!(segments[1].end_s, 3.0);
    }

    #[test]
    fn chunked_inference_does_not_return_success_after_backend_failure() {
        struct FailedEngine;
        impl crate::engine::SpeechEngine for FailedEngine {
            fn transcribe(&mut self, _: &[f32]) -> anyhow::Result<TranscriptionResult> {
                anyhow::bail!("model inference failed")
            }
        }
        let error = super::transcribe_samples(&mut FailedEngine, &[0.0; 16000], Some(1.0), 0.0)
            .err()
            .unwrap();
        assert!(format!("{error:#}").contains("failed chunk 0"));
        assert!(format!("{error:#}").contains("model inference failed"));
    }

    #[test]
    fn build_chunk_ranges_splits_audio() {
        let ranges = build_chunk_ranges(5 * 16_000, 16_000, 2.0, 0.0).unwrap();
        assert_eq!(
            ranges,
            vec![(0, 32_000), (32_000, 64_000), (64_000, 80_000)]
        );
    }

    #[test]
    fn build_chunk_ranges_supports_overlap() {
        let ranges = build_chunk_ranges(5 * 16_000, 16_000, 2.0, 1.0).unwrap();
        assert_eq!(
            ranges,
            vec![
                (0, 32_000),
                (16_000, 48_000),
                (32_000, 64_000),
                (48_000, 80_000)
            ]
        );
    }
}
