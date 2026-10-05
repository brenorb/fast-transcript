use anyhow::{bail, Context, Result};
use std::path::Path;
use std::time::Instant;
use transcribe_rs::audio::read_wav_samples;

use crate::audio::normalize_audio;
use crate::diarization::{maybe_diarize_segments, FluidAudioDiarizer};
use crate::engine::{load_engine, LoadedEngine, SpeechEngine};
use crate::progress::ChunkProgressReporter;
use crate::transcript::{
    merge_chunk_texts, merge_transcript_segments, transcript_segments_from_transcription,
};
use crate::types::{BenchmarkChunk, BenchmarkResult, CliArgs, TranscriptSegment};

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

fn transcribe_chunked(
    model: &mut dyn SpeechEngine,
    samples: &[f32],
    chunk_seconds: f64,
    chunk_overlap_seconds: f64,
) -> Result<(String, Vec<BenchmarkChunk>, Vec<TranscriptSegment>, f64)> {
    let ranges = build_chunk_ranges(
        samples.len(),
        crate::SAMPLE_RATE,
        chunk_seconds,
        chunk_overlap_seconds,
    )?;
    let mut chunks = Vec::with_capacity(ranges.len());
    let mut merged_text = String::new();
    let mut merged_segments = Vec::new();
    let mut total_transcribe_seconds = 0.0;
    let total_chunks = ranges.len();
    let progress = ChunkProgressReporter::start(total_chunks);

    for (index, (start, end)) in ranges.into_iter().enumerate() {
        progress.set_current_chunk(index + 1);
        let transcribe_started = Instant::now();
        let mut transcription = model
            .transcribe(&samples[start..end])
            .with_context(|| format!("failed chunk {index} ({start}..{end})"))?;
        let transcribe_seconds = transcribe_started.elapsed().as_secs_f64();
        total_transcribe_seconds += transcribe_seconds;
        transcription.offset_timestamps(start as f32 / crate::SAMPLE_RATE as f32);

        let text = transcription.text.trim().to_string();
        merged_text = merge_chunk_texts(&merged_text, &text);
        merge_transcript_segments(
            &mut merged_segments,
            transcript_segments_from_transcription(
                &transcription,
                start as f64 / crate::SAMPLE_RATE as f64,
                end as f64 / crate::SAMPLE_RATE as f64,
            ),
        );

        chunks.push(BenchmarkChunk {
            index,
            start_s: start as f64 / crate::SAMPLE_RATE as f64,
            end_s: end as f64 / crate::SAMPLE_RATE as f64,
            audio_seconds: (end - start) as f64 / crate::SAMPLE_RATE as f64,
            transcribe_seconds,
            text,
        });
    }

    progress.finish();
    Ok((
        merged_text,
        chunks,
        merged_segments,
        total_transcribe_seconds,
    ))
}

pub(crate) fn transcribe_audio_input(
    args: &CliArgs,
    input_source: &str,
    audio_path: &Path,
    transcript_source: &str,
) -> Result<BenchmarkResult> {
    let prepared_audio = normalize_audio(audio_path)?;

    let samples = read_wav_samples(&prepared_audio.wav_path).with_context(|| {
        format!(
            "failed to read WAV samples from {}",
            prepared_audio.wav_path.display()
        )
    })?;
    let audio_seconds = samples.len() as f64 / crate::SAMPLE_RATE as f64;

    eprintln!("loading model...");
    let LoadedEngine {
        mut engine,
        load_seconds,
        directory,
        metadata,
    } = load_engine(&args.model)?;
    let (text, chunks, transcript_segments, transcribe_seconds) = {
        if let Some(chunk_seconds) = args.chunk_seconds {
            let (text, chunks, transcript_segments, transcribe_seconds) = transcribe_chunked(
                engine.as_mut(),
                &samples,
                chunk_seconds,
                args.chunk_overlap_seconds,
            )?;
            (text, chunks, transcript_segments, transcribe_seconds)
        } else {
            eprintln!("transcribing...");
            let transcribe_start = Instant::now();
            let transcription = engine
                .transcribe(&samples)
                .context("failed to transcribe audio")?;
            let transcribe_seconds = transcribe_start.elapsed().as_secs_f64();
            let text = transcription.text.trim().to_string();
            let chunks = vec![BenchmarkChunk {
                index: 0,
                start_s: 0.0,
                end_s: audio_seconds,
                audio_seconds,
                transcribe_seconds,
                text: text.clone(),
            }];
            let transcript_segments =
                transcript_segments_from_transcription(&transcription, 0.0, audio_seconds);
            (text, chunks, transcript_segments, transcribe_seconds)
        }
    };
    drop(engine);
    drop(samples);

    let (segments, speaker_diarization) = maybe_diarize_segments(
        &FluidAudioDiarizer::new(),
        &prepared_audio.wav_path,
        transcript_segments,
        args.diarization.as_ref(),
    )?;

    let total_inside_seconds = load_seconds + transcribe_seconds;
    let (seconds_per_audio_second, realtime_speedup) =
        derived_benchmark_speeds(audio_seconds, total_inside_seconds);
    Ok(BenchmarkResult {
        input_source: input_source.to_string(),
        model_dir: directory,
        audio_path: audio_path.display().to_string(),
        prepared_audio_path: prepared_audio.wav_path.display().to_string(),
        used_ffmpeg_normalization: prepared_audio.normalized,
        used_local_model: true,
        transcript_source: transcript_source.to_string(),
        audio_seconds,
        load_seconds,
        transcribe_seconds,
        total_inside_seconds,
        seconds_per_audio_second,
        realtime_speedup,
        text,
        chunk_seconds: args.chunk_seconds,
        chunk_overlap_seconds: args.chunk_overlap_seconds,
        chunk_count: chunks.len(),
        chunks,
        segments: (!segments.is_empty()).then_some(segments),
        speaker_diarization,
        model: Some(metadata),
    })
}

fn derived_benchmark_speeds(audio_seconds: f64, total_inside_seconds: f64) -> (f64, f64) {
    if audio_seconds <= 0.0 || !audio_seconds.is_finite() || total_inside_seconds <= 0.0 {
        return (0.0, 0.0);
    }
    (
        total_inside_seconds / audio_seconds,
        audio_seconds / total_inside_seconds,
    )
}

#[cfg(test)]
mod tests {
    use super::{build_chunk_ranges, derived_benchmark_speeds};
    use transcribe_rs::{TranscriptionResult, TranscriptionSegment};

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
        let (text, chunks, segments, _) =
            super::transcribe_chunked(&mut engine, &samples, 3.0, 1.0).unwrap();
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
        let error = super::transcribe_chunked(&mut FailedEngine, &[0.0; 16000], 1.0, 0.0)
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

    #[test]
    fn derived_benchmark_speeds_stay_finite_for_zero_length_audio() {
        let (seconds_per_audio_second, realtime_speedup) = derived_benchmark_speeds(0.0, 0.25);
        assert_eq!(seconds_per_audio_second, 0.0);
        assert_eq!(realtime_speedup, 0.0);
    }
}
