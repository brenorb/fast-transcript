use anyhow::{bail, Context, Result};
use std::path::Path;
use std::time::Instant;
use transcribe_rs::audio::read_wav_samples;
use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams, TimestampGranularity};
use transcribe_rs::onnx::Quantization;

use crate::audio::normalize_audio;
use crate::diarization::{maybe_diarize_segments, FluidAudioDiarizer};
use crate::model::ensure_model_dir;
use crate::progress::ChunkProgressReporter;
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

fn transcript_segments_from_transcription(
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

fn transcribe_chunked(
    model: &mut ParakeetModel,
    samples: &[f32],
    chunk_seconds: f64,
    chunk_overlap_seconds: f64,
    params: &ParakeetParams,
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
            .transcribe_with(&samples[start..end], params)
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
    ensure_model_dir(&args.model_dir, &args.model_package, &args.model_url)?;
    let prepared_audio = normalize_audio(audio_path)?;

    let samples = read_wav_samples(&prepared_audio.wav_path).with_context(|| {
        format!(
            "failed to read WAV samples from {}",
            prepared_audio.wav_path.display()
        )
    })?;
    let audio_seconds = samples.len() as f64 / crate::SAMPLE_RATE as f64;

    let (text, chunks, transcript_segments, load_seconds, transcribe_seconds) = {
        let load_start = Instant::now();
        eprintln!("loading model...");
        let mut model = ParakeetModel::load(&args.model_dir, &Quantization::Int8)
            .context("failed to load Parakeet model")?;
        let load_seconds = load_start.elapsed().as_secs_f64();

        let params = ParakeetParams {
            timestamp_granularity: Some(TimestampGranularity::Segment),
            ..Default::default()
        };
        if let Some(chunk_seconds) = args.chunk_seconds {
            let (text, chunks, transcript_segments, transcribe_seconds) = transcribe_chunked(
                &mut model,
                &samples,
                chunk_seconds,
                args.chunk_overlap_seconds,
                &params,
            )?;
            (
                text,
                chunks,
                transcript_segments,
                load_seconds,
                transcribe_seconds,
            )
        } else {
            eprintln!("transcribing...");
            let transcribe_start = Instant::now();
            let transcription = model
                .transcribe_with(&samples, &params)
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
            (
                text,
                chunks,
                transcript_segments,
                load_seconds,
                transcribe_seconds,
            )
        }
    };
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
        model_dir: args.model_dir.display().to_string(),
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
    use super::{
        build_chunk_ranges, derived_benchmark_speeds, merge_chunk_texts, merge_transcript_segments,
        transcript_segments_from_text,
    };
    use crate::types::TranscriptSegment;

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
    fn derived_benchmark_speeds_stay_finite_for_zero_length_audio() {
        let (seconds_per_audio_second, realtime_speedup) = derived_benchmark_speeds(0.0, 0.25);
        assert_eq!(seconds_per_audio_second, 0.0);
        assert_eq!(realtime_speedup, 0.0);
    }
}
