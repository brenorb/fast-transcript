use crate::diarization::{DiarizationRequest, SpeakerDiarizationMetadata};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tempfile::TempDir;

#[derive(Debug)]
pub(crate) struct CliArgs {
    pub(crate) model_dir: PathBuf,
    pub(crate) model_package: PathBuf,
    pub(crate) model_url: String,
    pub(crate) input: String,
    pub(crate) output_path: Option<PathBuf>,
    pub(crate) output_to_stdout: bool,
    pub(crate) output_format: OutputFormat,
    pub(crate) clean_output: bool,
    pub(crate) force_local_for_remote: bool,
    pub(crate) chunk_seconds: Option<f64>,
    pub(crate) chunk_overlap_seconds: f64,
    pub(crate) diarization_notice: Option<String>,
    pub(crate) diarization: Option<DiarizationRequest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpeakersFormat {
    Plain,
    Timestamped,
}

impl SpeakersFormat {
    pub(crate) fn from_cli_value(value: &str) -> Option<Self> {
        match value {
            "plain" => Some(Self::Plain),
            "timestamps" => Some(Self::Timestamped),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextFormat {
    Plain,
    Compact,
    Timestamped,
}

impl TextFormat {
    pub(crate) fn from_cli_value(value: &str) -> Option<Self> {
        match value {
            "plain" => Some(Self::Plain),
            "compact" => Some(Self::Compact),
            "timestamps" => Some(Self::Timestamped),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubtitleFormat {
    Srt,
    Vtt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputFormat {
    Json,
    Speakers(SpeakersFormat),
    Text(TextFormat),
    Subtitle(SubtitleFormat),
}

#[derive(Debug)]
pub(crate) struct PreparedAudio {
    pub(crate) wav_path: PathBuf,
    pub(crate) normalized: bool,
    pub(crate) _tempdir: Option<TempDir>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InputSource {
    LocalPath(PathBuf),
    RemoteUrl(String),
}

#[derive(Debug)]
pub(crate) struct DownloadedAudio {
    pub(crate) audio_path: PathBuf,
    pub(crate) _tempdir: TempDir,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FfprobeOutput {
    pub(crate) streams: Vec<FfprobeStream>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FfprobeStream {
    pub(crate) codec_type: Option<String>,
    pub(crate) codec_name: Option<String>,
    pub(crate) sample_rate: Option<String>,
    pub(crate) channels: Option<u64>,
    pub(crate) bits_per_sample: Option<u64>,
    pub(crate) sample_fmt: Option<String>,
}

#[derive(Clone, Serialize)]
pub(crate) struct BenchmarkChunk {
    pub(crate) index: usize,
    pub(crate) start_s: f64,
    pub(crate) end_s: f64,
    pub(crate) audio_seconds: f64,
    pub(crate) transcribe_seconds: f64,
    pub(crate) text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct TranscriptSegment {
    pub(crate) start_s: f64,
    pub(crate) end_s: f64,
    pub(crate) text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) speaker: Option<String>,
}

#[derive(Clone, Serialize)]
pub(crate) struct BenchmarkResult {
    pub(crate) input_source: String,
    pub(crate) model_dir: String,
    pub(crate) audio_path: String,
    pub(crate) prepared_audio_path: String,
    pub(crate) used_ffmpeg_normalization: bool,
    pub(crate) used_local_model: bool,
    pub(crate) transcript_source: String,
    pub(crate) audio_seconds: f64,
    pub(crate) load_seconds: f64,
    pub(crate) transcribe_seconds: f64,
    pub(crate) total_inside_seconds: f64,
    pub(crate) seconds_per_audio_second: f64,
    pub(crate) realtime_speedup: f64,
    pub(crate) text: String,
    pub(crate) chunk_seconds: Option<f64>,
    pub(crate) chunk_overlap_seconds: f64,
    pub(crate) chunk_count: usize,
    pub(crate) chunks: Vec<BenchmarkChunk>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) segments: Option<Vec<TranscriptSegment>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) speaker_diarization: Option<SpeakerDiarizationMetadata>,
}
