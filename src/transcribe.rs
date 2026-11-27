//! Media preparation, model lifecycle, inference, and diarization orchestration.

use anyhow::{Context, Result};
use std::path::Path;
use transcribe_rs::audio::read_wav_samples;

use crate::audio::prepare_audio;
use crate::diarization::{maybe_diarize_segments, FluidAudioDiarizer};
use crate::engine::{load_engine, LoadedEngine};
use crate::inference::{transcribe_samples, InferenceOutcome};
use crate::types::{BenchmarkResult, CliArgs, ModelMetadata, PreparedAudio};

pub(crate) fn transcribe_audio_input(
    args: &CliArgs,
    input_source: &str,
    audio_path: &Path,
    transcript_source: &str,
) -> Result<BenchmarkResult> {
    let prepared_audio = prepare_audio(audio_path, args.media_range)?;
    transcribe_prepared_audio(
        args,
        input_source,
        audio_path,
        transcript_source,
        prepared_audio,
    )
}

pub(crate) fn transcribe_prepared_audio(
    args: &CliArgs,
    input_source: &str,
    audio_path: &Path,
    transcript_source: &str,
    prepared_audio: PreparedAudio,
) -> Result<BenchmarkResult> {
    let samples = read_wav_samples(&prepared_audio.wav_path).with_context(|| {
        format!(
            "failed to read WAV samples from {}",
            prepared_audio.wav_path.display()
        )
    })?;
    let audio_seconds = samples.len() as f64 / crate::SAMPLE_RATE as f64;
    if samples.is_empty() {
        anyhow::bail!("selected media contains no audio samples");
    }

    eprintln!("loading model...");
    let LoadedEngine {
        mut engine,
        load_seconds,
        directory,
        metadata,
    } = load_engine(&args.model)?;
    let outcome = transcribe_samples(
        engine.as_mut(),
        &samples,
        args.chunk_seconds,
        args.chunk_overlap_seconds,
    )?;
    drop(engine);
    drop(samples);

    finalize_transcription(
        args,
        input_source,
        audio_path,
        transcript_source,
        prepared_audio,
        CompletedInference {
            outcome,
            load_seconds,
            directory,
            metadata,
            audio_seconds,
        },
    )
}

pub(crate) struct CompletedInference {
    pub(crate) outcome: InferenceOutcome,
    pub(crate) load_seconds: f64,
    pub(crate) directory: String,
    pub(crate) metadata: ModelMetadata,
    pub(crate) audio_seconds: f64,
}

pub(crate) fn finalize_transcription(
    args: &CliArgs,
    input_source: &str,
    audio_path: &Path,
    transcript_source: &str,
    prepared_audio: PreparedAudio,
    completed: CompletedInference,
) -> Result<BenchmarkResult> {
    let CompletedInference {
        outcome,
        load_seconds,
        directory,
        metadata,
        audio_seconds,
    } = completed;
    let InferenceOutcome {
        text,
        mut chunks,
        segments: transcript_segments,
        transcribe_seconds,
    } = outcome;
    let (mut segments, speaker_diarization) = maybe_diarize_segments(
        &FluidAudioDiarizer::new(),
        &prepared_audio.wav_path,
        transcript_segments,
        args.diarization.as_ref(),
    )?;

    // Diarization uses clip-relative audio. Shift only after speaker assignment.
    for segment in &mut segments {
        segment.start_s += prepared_audio.source_start_s;
        segment.end_s += prepared_audio.source_start_s;
    }
    for chunk in &mut chunks {
        chunk.start_s += prepared_audio.source_start_s;
        chunk.end_s += prepared_audio.source_start_s;
    }

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
        preview_transcribe_seconds: None,
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
    use super::derived_benchmark_speeds;

    #[test]
    fn derived_benchmark_speeds_stay_finite_for_zero_length_audio() {
        let (seconds_per_audio_second, realtime_speedup) = derived_benchmark_speeds(0.0, 0.25);
        assert_eq!(seconds_per_audio_second, 0.0);
        assert_eq!(realtime_speedup, 0.0);
    }
}
