mod diarization;

use anyhow::{bail, Context, Result};
use diarization::{
    maybe_diarize_segments, DiarizationBackend, DiarizationRequest, FluidAudioDiarizer,
    SpeakerDiarizationMetadata,
};
use directories::ProjectDirs;
use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::fs::File;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tar::Archive;
use tempfile::TempDir;
use transcribe_rs::audio::read_wav_samples;
use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams, TimestampGranularity};
use transcribe_rs::onnx::Quantization;

const SAMPLE_RATE: usize = 16_000;
const DEFAULT_DATA_DIR_FALLBACK: &str = ".fast-transcript";
const DEFAULT_CACHE_DIR_FALLBACK: &str = ".fast-transcript-cache";
const DEFAULT_MODEL_SUBDIR: &str = "models";
const DEFAULT_MODEL_PACKAGE_NAME: &str = "parakeet-v3-int8.tar.gz";
const DEFAULT_MODEL_URL: &str = "https://huggingface.co/brenorb/parakeet-tdt-0.6b-v3-int8-onnx-bundle/resolve/main/parakeet-v3-int8.tar.gz?download=1";
const DEFAULT_MODEL_BASENAME: &str = "parakeet-tdt-0.6b-v3-int8";
const DEFAULT_CHUNK_SECONDS: f64 = 120.0;
const DEFAULT_CHUNK_OVERLAP_SECONDS: f64 = 2.0;
const PROGRESS_BAR_WIDTH: usize = 20;
const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const REQUIRED_MODEL_FILES: [&str; 4] = [
    "encoder-model.int8.onnx",
    "decoder_joint-model.int8.onnx",
    "nemo128.onnx",
    "vocab.txt",
];

#[derive(Debug)]
struct CliArgs {
    model_dir: PathBuf,
    model_package: PathBuf,
    model_url: String,
    input: String,
    output_path: Option<PathBuf>,
    output_to_stdout: bool,
    output_format: OutputFormat,
    prefer_local_for_remote: bool,
    chunk_seconds: Option<f64>,
    chunk_overlap_seconds: f64,
    diarization: Option<DiarizationRequest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptFormat {
    Plain,
    Timestamped,
}

impl ScriptFormat {
    fn from_cli_value(value: &str) -> Option<Self> {
        match value {
            "plain" => Some(Self::Plain),
            "timestamps" => Some(Self::Timestamped),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextFormat {
    Plain,
    Timestamped,
}

impl TextFormat {
    fn from_cli_value(value: &str) -> Option<Self> {
        match value {
            "plain" => Some(Self::Plain),
            "timestamps" => Some(Self::Timestamped),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubtitleFormat {
    Srt,
    Vtt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Json,
    Script(ScriptFormat),
    Text(TextFormat),
    Subtitle(SubtitleFormat),
}

#[derive(Debug)]
struct PreparedAudio {
    wav_path: PathBuf,
    normalized: bool,
    _tempdir: Option<TempDir>,
}

#[derive(Debug, PartialEq, Eq)]
enum InputSource {
    LocalPath(PathBuf),
    RemoteUrl(String),
}

#[derive(Debug, Deserialize)]
struct YtDlpVideoInfo {
    id: Option<String>,
    title: Option<String>,
    subtitles: Option<BTreeMap<String, Vec<YtDlpSubtitleTrack>>>,
}

#[derive(Debug, Deserialize)]
struct YtDlpSubtitleTrack {
    ext: Option<String>,
}

#[derive(Debug)]
struct DirectTranscript {
    text: String,
    segments: Vec<TranscriptSegment>,
}

#[derive(Debug)]
struct DownloadedAudio {
    audio_path: PathBuf,
    _tempdir: TempDir,
}

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    streams: Vec<FfprobeStream>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u64>,
    bits_per_sample: Option<u64>,
    sample_fmt: Option<String>,
}

#[derive(Serialize)]
struct BenchmarkChunk {
    index: usize,
    start_s: f64,
    end_s: f64,
    audio_seconds: f64,
    transcribe_seconds: f64,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct TranscriptSegment {
    start_s: f64,
    end_s: f64,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    speaker: Option<String>,
}

#[derive(Serialize)]
struct BenchmarkResult {
    input_source: String,
    model_dir: String,
    audio_path: String,
    prepared_audio_path: String,
    used_ffmpeg_normalization: bool,
    used_local_model: bool,
    transcript_source: String,
    audio_seconds: f64,
    load_seconds: f64,
    transcribe_seconds: f64,
    total_inside_seconds: f64,
    seconds_per_audio_second: f64,
    realtime_speedup: f64,
    text: String,
    chunk_seconds: Option<f64>,
    chunk_overlap_seconds: f64,
    chunk_count: usize,
    chunks: Vec<BenchmarkChunk>,
    #[serde(skip_serializing_if = "Option::is_none")]
    segments: Option<Vec<TranscriptSegment>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    speaker_diarization: Option<SpeakerDiarizationMetadata>,
}

struct ChunkProgressReporter {
    total_chunks: usize,
    current_chunk: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

fn usage() -> String {
    format!(
        "usage: fscript <audio-or-url> [output-path | - | --stdout] [--script [plain|timestamps] | --text [plain|timestamps] | --srt | --vtt] [--prefer-local-for-remote] [-d [{}|{}] | --diarize [{}|{}]] [-n N | --num-speakers N] [-t N | --threshold N] [--model-dir PATH] [--model-package PATH] [--model-url URL] [--chunk-seconds N] [--chunk-overlap-seconds N]\n\
aliases:\n\
  -d [{}|{}]\n\
  --script [plain|timestamps]\n\
  --text [plain|timestamps]\n\
  --srt (experimental subtitle output)\n\
  --vtt (experimental subtitle output)\n\
  -n, --num-speakers <count>\n\
  -t, --threshold <value>\n\
notes:\n\
  subtitle output via --srt/--vtt is experimental and may change\n\
defaults:\n\
  --script timestamps\n\
  --text plain\n\
  --model-dir {}\n\
  --model-package {}\n\
  --chunk-seconds 120\n\
  --chunk-overlap-seconds 2",
        DiarizationBackend::Coreml.cli_name(),
        DiarizationBackend::LseendDihard3.cli_name(),
        DiarizationBackend::Coreml.cli_name(),
        DiarizationBackend::LseendDihard3.cli_name(),
        DiarizationBackend::Coreml.cli_name(),
        DiarizationBackend::LseendDihard3.cli_name(),
        default_model_dir().display(),
        default_model_package().display()
    )
}

fn version_string() -> String {
    format!("fscript {}", env!("CARGO_PKG_VERSION"))
}

fn default_app_data_dir() -> PathBuf {
    if let Some(project_dirs) = ProjectDirs::from("", "", "fast-transcript") {
        return project_dirs.data_local_dir().to_path_buf();
    }

    env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".local").join("share").join("fast-transcript"))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR_FALLBACK))
}

fn default_app_cache_dir() -> PathBuf {
    if let Some(project_dirs) = ProjectDirs::from("", "", "fast-transcript") {
        return project_dirs.cache_dir().to_path_buf();
    }

    env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".cache").join("fast-transcript"))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CACHE_DIR_FALLBACK))
}

fn default_model_dir() -> PathBuf {
    env::var_os("FSCRIPT_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            default_app_data_dir()
                .join(DEFAULT_MODEL_SUBDIR)
                .join(DEFAULT_MODEL_BASENAME)
        })
}

fn default_model_package() -> PathBuf {
    env::var_os("FSCRIPT_MODEL_PACKAGE")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_app_cache_dir().join(DEFAULT_MODEL_PACKAGE_NAME))
}

fn default_model_url() -> String {
    env::var("FSCRIPT_MODEL_URL").unwrap_or_else(|_| DEFAULT_MODEL_URL.to_string())
}

fn default_output_path(audio_path: &Path, output_format: OutputFormat) -> PathBuf {
    let stem = audio_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("transcript");
    let file_name = match output_format {
        OutputFormat::Json => format!("{stem}.transcript.json"),
        OutputFormat::Script(_) => format!("{stem}.script.txt"),
        OutputFormat::Text(_) => format!("{stem}.transcript.txt"),
        OutputFormat::Subtitle(SubtitleFormat::Srt) => format!("{stem}.srt"),
        OutputFormat::Subtitle(SubtitleFormat::Vtt) => format!("{stem}.vtt"),
    };
    audio_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(file_name)
}

fn is_remote_url(value: &str) -> bool {
    value.starts_with("https://") || value.starts_with("http://")
}

fn infer_input_source(value: &str) -> InputSource {
    if is_remote_url(value) {
        InputSource::RemoteUrl(value.to_string())
    } else {
        InputSource::LocalPath(PathBuf::from(value))
    }
}

fn sanitize_file_stem(value: &str) -> String {
    let mut sanitized = String::new();
    let mut last_was_separator = false;
    for ch in value.chars() {
        let normalized = if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-') {
            last_was_separator = false;
            ch
        } else {
            if last_was_separator {
                continue;
            }
            last_was_separator = true;
            '_'
        };
        sanitized.push(normalized);
    }
    let sanitized = sanitized.trim_matches('_').to_string();
    if sanitized.is_empty() {
        "transcript".to_string()
    } else {
        sanitized
    }
}

fn default_output_path_for_input(
    source: &InputSource,
    video_title: Option<&str>,
    output_format: OutputFormat,
) -> PathBuf {
    match source {
        InputSource::LocalPath(path) => default_output_path(path, output_format),
        InputSource::RemoteUrl(url) => {
            let stem = video_title
                .filter(|value| !value.trim().is_empty())
                .map(sanitize_file_stem)
                .unwrap_or_else(|| sanitize_file_stem(url));
            match output_format {
                OutputFormat::Json => PathBuf::from(format!("{stem}.transcript.json")),
                OutputFormat::Script(_) => PathBuf::from(format!("{stem}.script.txt")),
                OutputFormat::Text(_) => PathBuf::from(format!("{stem}.transcript.txt")),
                OutputFormat::Subtitle(SubtitleFormat::Srt) => PathBuf::from(format!("{stem}.srt")),
                OutputFormat::Subtitle(SubtitleFormat::Vtt) => PathBuf::from(format!("{stem}.vtt")),
            }
        }
    }
}

fn resolve_absolute_output_path(output_path: &Path, current_dir: &Path) -> Result<PathBuf> {
    let candidate = if output_path.is_absolute() {
        output_path.to_path_buf()
    } else {
        current_dir.join(output_path)
    };

    fs::canonicalize(&candidate).with_context(|| {
        format!(
            "failed to resolve absolute path for {}",
            candidate.display()
        )
    })
}

fn write_output_file(contents: &str, output_path: &Path, current_dir: &Path) -> Result<PathBuf> {
    let candidate = if output_path.is_absolute() {
        output_path.to_path_buf()
    } else {
        current_dir.join(output_path)
    };

    if let Some(parent) = candidate.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
    }
    fs::write(&candidate, format!("{contents}\n"))
        .with_context(|| format!("failed to write {}", candidate.display()))?;

    resolve_absolute_output_path(&candidate, current_dir)
}

fn normalize_speaker_label(label: &str, unknown_label: &str) -> String {
    let cleaned = label.trim();
    if cleaned.is_empty() {
        return unknown_label.to_string();
    }

    let formatted = cleaned
        .strip_prefix("SPEAKER_")
        .or_else(|| cleaned.strip_prefix("S"));
    if let Some(suffix) = formatted {
        if !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()) {
            if let Ok(number) = suffix.parse::<usize>() {
                return format!("SPEAKER_{number:02}");
            }
        }
    }

    cleaned.to_string()
}

fn format_hhmmss(seconds: f64) -> String {
    let total_seconds = if seconds.is_finite() {
        seconds.max(0.0).floor() as u64
    } else {
        0
    };
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn format_subtitle_timestamp(seconds: f64, millis_separator: char) -> String {
    let total_millis = if seconds.is_finite() {
        (seconds.max(0.0) * 1000.0).round() as u64
    } else {
        0
    };
    let hours = total_millis / 3_600_000;
    let minutes = (total_millis % 3_600_000) / 60_000;
    let seconds = (total_millis % 60_000) / 1_000;
    let millis = total_millis % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02}{millis_separator}{millis:03}")
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn segment_display_text(segment: &TranscriptSegment, include_speaker: bool) -> String {
    let text = segment.text.trim();
    if !include_speaker {
        return text.to_string();
    }

    match segment
        .speaker
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(speaker) => format!("{}: {text}", normalize_speaker_label(speaker, "UNKNOWN")),
        None => text.to_string(),
    }
}

fn render_speaker_script_lines(
    segments: &[TranscriptSegment],
    script_format: ScriptFormat,
    merge_consecutive: bool,
    unknown_label: &str,
) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current_speaker: Option<String> = None;
    let mut current_start_s = 0.0;
    let mut current_parts: Vec<&str> = Vec::new();

    let flush = |lines: &mut Vec<String>,
                 current_speaker: &mut Option<String>,
                 current_start_s: &mut f64,
                 current_parts: &mut Vec<&str>| {
        let Some(speaker) = current_speaker.take() else {
            current_parts.clear();
            return;
        };
        let text = current_parts.join(" ").trim().to_string();
        current_parts.clear();
        if text.is_empty() {
            return;
        }
        let line = match script_format {
            ScriptFormat::Plain => format!("{speaker}: {text}"),
            ScriptFormat::Timestamped => {
                format!("{} - {speaker}: {text}", format_hhmmss(*current_start_s))
            }
        };
        lines.push(line);
    };

    for segment in segments {
        let text = segment.text.trim();
        if text.is_empty() {
            continue;
        }

        let speaker =
            normalize_speaker_label(segment.speaker.as_deref().unwrap_or(""), unknown_label);
        if merge_consecutive && current_speaker.as_deref() == Some(speaker.as_str()) {
            current_parts.push(text);
            continue;
        }

        flush(
            &mut lines,
            &mut current_speaker,
            &mut current_start_s,
            &mut current_parts,
        );
        current_start_s = segment.start_s;
        current_speaker = Some(speaker);
        current_parts.push(text);
    }

    flush(
        &mut lines,
        &mut current_speaker,
        &mut current_start_s,
        &mut current_parts,
    );
    lines
}

fn render_timestamped_text_lines(segments: &[TranscriptSegment]) -> Vec<String> {
    segments
        .iter()
        .filter_map(|segment| {
            let text = segment.text.trim();
            if text.is_empty() {
                None
            } else {
                Some(format!("{} - {text}", format_hhmmss(segment.start_s)))
            }
        })
        .collect()
}

fn estimate_subtitle_duration_seconds(text: &str) -> f64 {
    let word_count = text.split_whitespace().count() as f64;
    let char_count = text.chars().count() as f64;
    (word_count * 0.42).max(char_count * 0.065).clamp(1.2, 6.0)
}

const SUBTITLE_MAX_CHARS: usize = 84;
const SUBTITLE_MIN_DURATION_SECONDS: f64 = 0.2;
const SUBTITLE_GAP_SECONDS: f64 = 0.05;

fn split_subtitle_phrase(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = Vec::new();
    let mut current_len = 0usize;

    for word in text.split_whitespace() {
        let projected_len = if current.is_empty() {
            word.len()
        } else {
            current_len + 1 + word.len()
        };
        if !current.is_empty() && projected_len > SUBTITLE_MAX_CHARS {
            parts.push(current.join(" "));
            current.clear();
            current_len = 0;
        }
        current.push(word);
        current_len = if current_len == 0 {
            word.len()
        } else {
            current_len + 1 + word.len()
        };
    }

    if !current.is_empty() {
        parts.push(current.join(" "));
    }

    parts
}

fn split_subtitle_text(text: &str) -> Vec<String> {
    let mut phrases = Vec::new();
    let mut phrase_start = 0usize;
    let sentence_breaks = ['.', '!', '?', ';', ':'];

    for (index, ch) in text.char_indices() {
        if !sentence_breaks.contains(&ch) {
            continue;
        }

        let next_index = index + ch.len_utf8();
        let phrase = text[phrase_start..next_index].trim();
        if !phrase.is_empty() {
            phrases.push(phrase.to_string());
        }
        phrase_start = next_index;
    }

    let trailing = text[phrase_start..].trim();
    if !trailing.is_empty() {
        phrases.push(trailing.to_string());
    }
    if phrases.is_empty() {
        phrases.push(text.trim().to_string());
    }

    let mut cues = Vec::new();
    let mut current = String::new();
    for phrase in phrases {
        if phrase.len() > SUBTITLE_MAX_CHARS {
            if !current.is_empty() {
                cues.push(current);
                current = String::new();
            }
            cues.extend(split_subtitle_phrase(&phrase));
            continue;
        }

        let projected_len = if current.is_empty() {
            phrase.len()
        } else {
            current.len() + 1 + phrase.len()
        };
        if !current.is_empty() && projected_len > SUBTITLE_MAX_CHARS {
            cues.push(current);
            current = phrase;
        } else if current.is_empty() {
            current = phrase;
        } else {
            current.push(' ');
            current.push_str(&phrase);
        }
    }

    if !current.is_empty() {
        cues.push(current);
    }

    cues
}

fn subtitle_segment_available_end(segment: &TranscriptSegment, next_start: Option<f64>) -> f64 {
    let mut end_s = segment.end_s;
    if let Some(start) = next_start {
        end_s = end_s.min((start - SUBTITLE_GAP_SECONDS).max(segment.start_s));
    }
    if end_s <= segment.start_s {
        segment.start_s + SUBTITLE_MIN_DURATION_SECONDS
    } else {
        end_s
    }
}

fn subtitle_segments_for_segment(
    segment: &TranscriptSegment,
    next_start: Option<f64>,
) -> Vec<TranscriptSegment> {
    let text = segment.text.trim();
    if text.is_empty() || segment.end_s <= segment.start_s {
        return Vec::new();
    }

    let cues = split_subtitle_text(text);
    if cues.is_empty() {
        return Vec::new();
    }

    let available_end = subtitle_segment_available_end(segment, next_start);
    if cues.len() == 1 {
        let mut end_s = available_end;
        if end_s <= segment.start_s {
            end_s = segment.start_s + SUBTITLE_MIN_DURATION_SECONDS;
        }

        return vec![TranscriptSegment {
            start_s: segment.start_s,
            end_s,
            text: text.to_string(),
            speaker: segment.speaker.clone(),
        }];
    }

    let weights = cues
        .iter()
        .map(|cue| estimate_subtitle_duration_seconds(cue))
        .collect::<Vec<_>>();
    let total_weight = weights.iter().sum::<f64>();
    let available_duration =
        (available_end - segment.start_s).max(SUBTITLE_MIN_DURATION_SECONDS * cues.len() as f64);
    let mut elapsed_weight = 0.0;
    let mut current_start = segment.start_s;
    let mut normalized = Vec::with_capacity(cues.len());

    for (index, cue_text) in cues.into_iter().enumerate() {
        let window_start = segment.start_s + available_duration * (elapsed_weight / total_weight);
        elapsed_weight += weights[index];
        let window_end = if index + 1 == weights.len() {
            available_end
        } else {
            segment.start_s + available_duration * (elapsed_weight / total_weight)
        };

        let start_s = current_start.max(window_start);
        if start_s >= available_end {
            break;
        }

        let ideal_end = start_s + estimate_subtitle_duration_seconds(&cue_text);
        let latest_end = if index + 1 == weights.len() {
            available_end
        } else {
            (window_end - SUBTITLE_GAP_SECONDS).max(start_s + SUBTITLE_MIN_DURATION_SECONDS)
        };
        let mut end_s = ideal_end.min(latest_end);
        if end_s <= start_s {
            end_s = (start_s + SUBTITLE_MIN_DURATION_SECONDS).min(available_end);
        }

        normalized.push(TranscriptSegment {
            start_s,
            end_s,
            text: cue_text,
            speaker: segment.speaker.clone(),
        });
        current_start = (end_s + SUBTITLE_GAP_SECONDS).min(available_end);
    }

    normalized
}

fn normalized_subtitle_segments(segments: &[TranscriptSegment]) -> Vec<TranscriptSegment> {
    let mut normalized = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        let next_start = segments
            .iter()
            .skip(index + 1)
            .find(|candidate| {
                candidate.end_s > candidate.start_s && !candidate.text.trim().is_empty()
            })
            .map(|candidate| candidate.start_s);
        normalized.extend(subtitle_segments_for_segment(segment, next_start));
    }
    normalized
}

fn render_srt(segments: &[TranscriptSegment]) -> String {
    normalized_subtitle_segments(segments)
        .into_iter()
        .enumerate()
        .map(|(index, segment)| {
            format!(
                "{}\n{} --> {}\n{}",
                index + 1,
                format_subtitle_timestamp(segment.start_s, ','),
                format_subtitle_timestamp(segment.end_s, ','),
                segment_display_text(&segment, true),
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_vtt(segments: &[TranscriptSegment]) -> String {
    let body = normalized_subtitle_segments(segments)
        .into_iter()
        .map(|segment| {
            format!(
                "{} --> {}\n{}",
                format_subtitle_timestamp(segment.start_s, '.'),
                format_subtitle_timestamp(segment.end_s, '.'),
                segment_display_text(&segment, true),
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    if body.is_empty() {
        "WEBVTT".to_string()
    } else {
        format!("WEBVTT\n\n{body}")
    }
}

fn render_output(result: &BenchmarkResult, output_format: OutputFormat) -> Result<String> {
    match output_format {
        OutputFormat::Json => serde_json::to_string_pretty(result).map_err(Into::into),
        OutputFormat::Script(script_format) => Ok(render_speaker_script_lines(
            result.segments.as_deref().unwrap_or(&[]),
            script_format,
            true,
            "UNKNOWN",
        )
        .join("\n")),
        OutputFormat::Text(TextFormat::Plain) => Ok(result.text.trim().to_string()),
        OutputFormat::Text(TextFormat::Timestamped) => {
            Ok(render_timestamped_text_lines(result.segments.as_deref().unwrap_or(&[])).join("\n"))
        }
        OutputFormat::Subtitle(SubtitleFormat::Srt) => {
            Ok(render_srt(result.segments.as_deref().unwrap_or(&[])))
        }
        OutputFormat::Subtitle(SubtitleFormat::Vtt) => {
            Ok(render_vtt(result.segments.as_deref().unwrap_or(&[])))
        }
    }
}

fn emit_file_output_completion(
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    output_path: &Path,
    status_message: &str,
) -> Result<()> {
    writeln!(stdout, "{}", output_path.display())
        .context("failed to write final output path to stdout")?;
    writeln!(stderr, "{status_message}").context("failed to write status message to stderr")?;
    Ok(())
}

fn pick_manual_subtitle_language(info: &YtDlpVideoInfo) -> Option<String> {
    let subtitles = info.subtitles.as_ref()?;
    let preferred = ["pt-BR", "pt", "en", "en-US"];
    for language in preferred {
        if subtitles
            .get(language)
            .is_some_and(|tracks| !tracks.is_empty())
        {
            return Some(language.to_string());
        }
    }
    subtitles
        .iter()
        .find(|(_, tracks)| !tracks.is_empty())
        .map(|(language, _)| language.to_string())
}

fn choose_subtitle_extension(info: &YtDlpVideoInfo, language: &str) -> String {
    let Some(subtitles) = info.subtitles.as_ref() else {
        return "vtt".to_string();
    };
    let Some(tracks) = subtitles.get(language) else {
        return "vtt".to_string();
    };
    if tracks
        .iter()
        .any(|track| track.ext.as_deref() == Some("vtt"))
    {
        "vtt".to_string()
    } else {
        "json3".to_string()
    }
}

fn run_command(command: &mut Command, context: &str) -> Result<std::process::Output> {
    command
        .output()
        .with_context(|| format!("failed to run {context}"))
}

fn yt_dlp_command() -> Command {
    let mut direct = Command::new("yt-dlp");
    direct.arg("--version");
    if direct.output().is_ok_and(|output| output.status.success()) {
        return Command::new("yt-dlp");
    }

    let mut fallback = Command::new("uvx");
    fallback.args(["yt-dlp", "--version"]);
    if fallback
        .output()
        .is_ok_and(|output| output.status.success())
    {
        let mut command = Command::new("uvx");
        command.arg("yt-dlp");
        return command;
    }

    Command::new("yt-dlp")
}

fn parse_subtitle_timestamp(value: &str) -> Option<f64> {
    let normalized = value.trim().replace(',', ".");
    let parts = normalized.split(':').collect::<Vec<_>>();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [minutes, seconds] => (
            0u64,
            minutes.parse::<u64>().ok()?,
            seconds.parse::<f64>().ok()?,
        ),
        [hours, minutes, seconds] => (
            hours.parse::<u64>().ok()?,
            minutes.parse::<u64>().ok()?,
            seconds.parse::<f64>().ok()?,
        ),
        _ => return None,
    };
    Some(hours as f64 * 3600.0 + minutes as f64 * 60.0 + seconds)
}

fn parse_vtt_segments(contents: &str) -> Vec<TranscriptSegment> {
    contents
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split("\n\n")
        .filter_map(|block| {
            let lines = block
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>();
            if lines.is_empty() {
                return None;
            }

            let timing_index = lines.iter().position(|line| line.contains("-->"))?;
            let (start, end) = {
                let timing_line = lines[timing_index];
                let (start, end) = timing_line.split_once("-->")?;
                let end = end.split_whitespace().next()?;
                (
                    parse_subtitle_timestamp(start)?,
                    parse_subtitle_timestamp(end)?,
                )
            };

            let text = collapse_whitespace(&lines[timing_index + 1..].join(" "));
            if text.is_empty() || end <= start {
                return None;
            }

            Some(TranscriptSegment {
                start_s: start,
                end_s: end,
                text,
                speaker: None,
            })
        })
        .collect()
}

#[cfg(test)]
fn parse_vtt_text(contents: &str) -> String {
    parse_vtt_segments(contents)
        .into_iter()
        .map(|segment| segment.text)
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Deserialize)]
struct Json3Transcript {
    events: Vec<Json3Event>,
}

#[derive(Debug, Deserialize)]
struct Json3Event {
    #[serde(rename = "tStartMs")]
    t_start_ms: Option<f64>,
    #[serde(rename = "dDurationMs")]
    d_duration_ms: Option<f64>,
    segs: Option<Vec<Json3CaptionSegment>>,
}

#[derive(Debug, Deserialize)]
struct Json3CaptionSegment {
    utf8: Option<String>,
}

fn parse_json3_segments(contents: &str) -> Result<Vec<TranscriptSegment>> {
    let parsed: Json3Transcript =
        serde_json::from_str(contents).context("failed to parse json3 subtitle payload")?;
    Ok(parsed
        .events
        .into_iter()
        .filter_map(|event| {
            let start = event.t_start_ms? / 1000.0;
            let duration = event.d_duration_ms? / 1000.0;
            let end = start + duration;
            let text = collapse_whitespace(
                &event
                    .segs
                    .into_iter()
                    .flatten()
                    .filter_map(|segment| segment.utf8)
                    .filter(|segment| !segment.trim().is_empty() && segment.trim() != "\n")
                    .collect::<String>(),
            );
            if text.is_empty() || end <= start {
                return None;
            }
            Some(TranscriptSegment {
                start_s: start,
                end_s: end,
                text,
                speaker: None,
            })
        })
        .collect())
}

fn load_remote_video_info(url: &str) -> Result<YtDlpVideoInfo> {
    let output = run_command(
        yt_dlp_command().args(["--dump-single-json", "--no-warnings", "--no-playlist", url]),
        "yt-dlp to inspect remote media metadata",
    )
    .with_context(|| "install yt-dlp, or make `uvx yt-dlp` available, to transcribe remote URLs")?;
    if !output.status.success() {
        bail!(
            "yt-dlp failed while inspecting {url}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    serde_json::from_slice(&output.stdout)
        .with_context(|| format!("failed to parse yt-dlp metadata for {url}"))
}

fn download_manual_remote_transcript(
    url: &str,
    info: &YtDlpVideoInfo,
) -> Result<Option<DirectTranscript>> {
    let Some(language) = pick_manual_subtitle_language(info) else {
        return Ok(None);
    };
    let extension = choose_subtitle_extension(info, &language);
    let tempdir = tempfile::tempdir().context("failed to create temp dir for remote subtitles")?;
    let output_template = tempdir.path().join("%(id)s.%(ext)s");
    let output = run_command(
        yt_dlp_command().args([
            "--no-progress",
            "--no-warnings",
            "--skip-download",
            "--write-subs",
            "--sub-langs",
            &language,
            "--sub-format",
            &extension,
            "--no-playlist",
            "--output",
            output_template.to_string_lossy().as_ref(),
            url,
        ]),
        "yt-dlp for manual subtitles",
    )
    .with_context(|| "install yt-dlp, or make `uvx yt-dlp` available, to transcribe remote URLs")?;
    if !output.status.success() {
        bail!(
            "yt-dlp failed while downloading subtitles for {url}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let subtitle_path = fs::read_dir(tempdir.path())
        .with_context(|| format!("failed to inspect {}", tempdir.path().display()))?
        .filter_map(|entry| entry.ok().map(|value| value.path()))
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some(extension.as_str()))
        .with_context(|| {
            format!("yt-dlp reported subtitles for {url} but did not write a .{extension} file")
        })?;

    let (text, segments) = if extension == "vtt" {
        let contents = fs::read_to_string(&subtitle_path)
            .with_context(|| format!("failed to read {}", subtitle_path.display()))?;
        let segments = parse_vtt_segments(&contents);
        (
            segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            segments,
        )
    } else {
        let contents = fs::read_to_string(&subtitle_path)
            .with_context(|| format!("failed to read {}", subtitle_path.display()))?;
        let segments = parse_json3_segments(&contents)
            .with_context(|| format!("failed to parse {}", subtitle_path.display()))?;
        (
            segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            segments,
        )
    };

    if text.trim().is_empty() {
        return Ok(None);
    }

    Ok(Some(DirectTranscript { text, segments }))
}

fn download_remote_audio(url: &str, _info: &YtDlpVideoInfo) -> Result<DownloadedAudio> {
    let tempdir = tempfile::tempdir().context("failed to create temp dir for remote audio")?;
    let output_template = tempdir.path().join("%(id)s.%(ext)s");
    let output = run_command(
        yt_dlp_command().args([
            "--no-progress",
            "--no-warnings",
            "--no-playlist",
            "-f",
            "bestaudio/best",
            "--output",
            output_template.to_string_lossy().as_ref(),
            "--print",
            "after_move:filepath",
            url,
        ]),
        "yt-dlp to download remote audio",
    )
    .with_context(|| "install yt-dlp, or make `uvx yt-dlp` available, to transcribe remote URLs")?;
    if !output.status.success() {
        bail!(
            "yt-dlp failed while downloading audio for {url}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let audio_path = String::from_utf8_lossy(&output.stdout)
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(|line| PathBuf::from(line.trim()))
        .with_context(|| format!("yt-dlp did not report the downloaded audio path for {url}"))?;
    Ok(DownloadedAudio {
        audio_path,
        _tempdir: tempdir,
    })
}

fn parse_args(raw_args: &[String]) -> Result<CliArgs> {
    if raw_args.is_empty() {
        bail!("{}", usage());
    }

    let mut model_dir = default_model_dir();
    let mut model_package = default_model_package();
    let mut model_url = default_model_url();
    let mut input = None;
    let mut output_path = None;
    let mut output_to_stdout = false;
    let mut output_format = OutputFormat::Json;
    let mut prefer_local_for_remote = false;
    let mut chunk_seconds_override = None;
    let mut chunk_overlap_seconds_override = None;
    let mut diarization_requested = false;
    let mut diarization_backend = DiarizationBackend::Coreml;
    let mut diarization_num_speakers = None;
    let mut diarization_threshold = None;
    let mut index = 0usize;

    while index < raw_args.len() {
        match raw_args[index].as_str() {
            "--model-dir" => {
                let value = raw_args
                    .get(index + 1)
                    .with_context(|| format!("missing value for --model-dir\n{}", usage()))?;
                model_dir = PathBuf::from(value);
                index += 2;
            }
            "--model-package" => {
                let value = raw_args
                    .get(index + 1)
                    .with_context(|| format!("missing value for --model-package\n{}", usage()))?;
                model_package = PathBuf::from(value);
                index += 2;
            }
            "--model-url" => {
                let value = raw_args
                    .get(index + 1)
                    .with_context(|| format!("missing value for --model-url\n{}", usage()))?;
                model_url = value.to_string();
                index += 2;
            }
            "--stdout" => {
                output_to_stdout = true;
                index += 1;
            }
            "--script" => {
                output_format = OutputFormat::Script(ScriptFormat::Timestamped);
                if let Some(value) = raw_args.get(index + 1) {
                    if let Some(script_format) = ScriptFormat::from_cli_value(value) {
                        output_format = OutputFormat::Script(script_format);
                        index += 2;
                        continue;
                    }
                }
                index += 1;
            }
            "--text" => {
                output_format = OutputFormat::Text(TextFormat::Plain);
                if let Some(value) = raw_args.get(index + 1) {
                    if let Some(text_format) = TextFormat::from_cli_value(value) {
                        output_format = OutputFormat::Text(text_format);
                        index += 2;
                        continue;
                    }
                }
                index += 1;
            }
            "--srt" => {
                output_format = OutputFormat::Subtitle(SubtitleFormat::Srt);
                index += 1;
            }
            "--vtt" => {
                output_format = OutputFormat::Subtitle(SubtitleFormat::Vtt);
                index += 1;
            }
            "--prefer-local-for-remote" => {
                prefer_local_for_remote = true;
                index += 1;
            }
            "-d" | "--diarize" => {
                diarization_requested = true;
                if let Some(value) = raw_args.get(index + 1) {
                    if let Some(backend) = DiarizationBackend::from_cli_value(value) {
                        diarization_backend = backend;
                        index += 2;
                        continue;
                    }
                }
                index += 1;
            }
            "--num-speakers" => {
                let value = raw_args
                    .get(index + 1)
                    .with_context(|| format!("missing value for --num-speakers\n{}", usage()))?;
                let parsed = value.parse::<usize>().with_context(|| {
                    format!("invalid --num-speakers value {value:?}\n{}", usage())
                })?;
                if parsed == 0 {
                    bail!("--num-speakers must be >= 1");
                }
                diarization_num_speakers = Some(parsed);
                index += 2;
            }
            "-n" => {
                let value = raw_args
                    .get(index + 1)
                    .with_context(|| format!("missing value for --num-speakers\n{}", usage()))?;
                let parsed = value.parse::<usize>().with_context(|| {
                    format!("invalid --num-speakers value {value:?}\n{}", usage())
                })?;
                if parsed == 0 {
                    bail!("--num-speakers must be >= 1");
                }
                diarization_num_speakers = Some(parsed);
                index += 2;
            }
            "-t" | "--threshold" => {
                let value = raw_args
                    .get(index + 1)
                    .with_context(|| format!("missing value for --threshold\n{}", usage()))?;
                let parsed = value
                    .parse::<f64>()
                    .with_context(|| format!("invalid --threshold value {value:?}\n{}", usage()))?;
                diarization_threshold = Some(parsed);
                index += 2;
            }
            "--chunk-seconds" => {
                let value = raw_args
                    .get(index + 1)
                    .with_context(|| format!("missing value for --chunk-seconds\n{}", usage()))?;
                let parsed = value.parse::<f64>().with_context(|| {
                    format!("invalid --chunk-seconds value {value:?}\n{}", usage())
                })?;
                if parsed < 0.0 {
                    bail!("--chunk-seconds must be >= 0");
                }
                chunk_seconds_override = Some(parsed);
                index += 2;
            }
            "--chunk-overlap-seconds" => {
                let value = raw_args.get(index + 1).with_context(|| {
                    format!("missing value for --chunk-overlap-seconds\n{}", usage())
                })?;
                let parsed = value.parse::<f64>().with_context(|| {
                    format!(
                        "invalid --chunk-overlap-seconds value {value:?}\n{}",
                        usage()
                    )
                })?;
                if parsed < 0.0 {
                    bail!("--chunk-overlap-seconds must be >= 0");
                }
                chunk_overlap_seconds_override = Some(parsed);
                index += 2;
            }
            flag if flag.starts_with("--") => bail!("unknown argument {flag:?}\n{}", usage()),
            value => {
                if input.is_none() {
                    input = Some(value.to_string());
                } else if output_path.is_none() && !output_to_stdout {
                    if value == "-" {
                        output_to_stdout = true;
                    } else {
                        output_path = Some(PathBuf::from(value));
                    }
                } else {
                    bail!("unexpected positional argument {value:?}\n{}", usage());
                }
                index += 1;
            }
        }
    }

    let input = input.with_context(|| format!("missing audio path\n{}", usage()))?;
    let output_path = if output_to_stdout { None } else { output_path };
    if diarization_num_speakers.is_some() && !diarization_requested {
        bail!("--num-speakers requires -d or --diarize");
    }
    if diarization_threshold.is_some() && !diarization_requested {
        bail!("--threshold requires -d or --diarize");
    }
    if diarization_num_speakers.is_some()
        && diarization_backend == DiarizationBackend::LseendDihard3
    {
        bail!("--num-speakers is not supported with lseend-dihard3; use -t or --threshold instead");
    }

    let requested_chunk_seconds = chunk_seconds_override.unwrap_or(DEFAULT_CHUNK_SECONDS);
    let (chunk_seconds, chunk_overlap_seconds) = if requested_chunk_seconds == 0.0 {
        let overlap = chunk_overlap_seconds_override.unwrap_or(0.0);
        if overlap > 0.0 {
            bail!("--chunk-overlap-seconds requires chunking to stay enabled");
        }
        (None, 0.0)
    } else {
        let overlap = chunk_overlap_seconds_override.unwrap_or(DEFAULT_CHUNK_OVERLAP_SECONDS);
        if overlap >= requested_chunk_seconds {
            bail!("--chunk-overlap-seconds must be smaller than --chunk-seconds");
        }
        (Some(requested_chunk_seconds), overlap)
    };

    Ok(CliArgs {
        model_dir,
        model_package,
        model_url,
        input,
        output_path,
        output_to_stdout,
        output_format,
        prefer_local_for_remote,
        chunk_seconds,
        chunk_overlap_seconds,
        diarization: diarization_requested.then_some(DiarizationRequest {
            backend: diarization_backend,
            num_speakers: diarization_num_speakers,
            threshold: diarization_threshold,
        }),
    })
}

fn has_required_model_files(model_dir: &Path) -> bool {
    REQUIRED_MODEL_FILES
        .iter()
        .all(|file_name| model_dir.join(file_name).is_file())
}

fn download_model_package(model_url: &str, package_path: &Path) -> Result<()> {
    let parent = package_path
        .parent()
        .with_context(|| format!("package path {} has no parent", package_path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;

    eprintln!(
        "model missing; downloading {} to {}",
        model_url,
        package_path.display()
    );
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(60 * 60))
        .build()
        .context("failed to build HTTP client for model download")?;
    let mut response = client
        .get(model_url)
        .send()
        .with_context(|| format!("failed to request {model_url}"))?
        .error_for_status()
        .with_context(|| format!("download failed for {model_url}"))?;

    let tmp_path = package_path.with_extension("tar.gz.partial");
    let mut file = File::create(&tmp_path)
        .with_context(|| format!("failed to create {}", tmp_path.display()))?;
    io::copy(&mut response, &mut file)
        .with_context(|| format!("failed to write {}", tmp_path.display()))?;
    fs::rename(&tmp_path, package_path).with_context(|| {
        format!(
            "failed to move downloaded model package into {}",
            package_path.display()
        )
    })?;
    Ok(())
}

fn extract_model_package(model_dir: &Path, package_path: &Path) -> Result<()> {
    let destination_root = model_dir
        .parent()
        .with_context(|| format!("model dir {} has no parent", model_dir.display()))?;
    fs::create_dir_all(destination_root)
        .with_context(|| format!("failed to create {}", destination_root.display()))?;

    eprintln!(
        "extracting {} into {}",
        package_path.display(),
        destination_root.display()
    );
    let archive_file = File::open(package_path)
        .with_context(|| format!("failed to open {}", package_path.display()))?;
    let decoder = GzDecoder::new(archive_file);
    let mut archive = Archive::new(decoder);
    archive
        .unpack(destination_root)
        .with_context(|| format!("failed to unpack {}", package_path.display()))?;
    remove_appledouble_files(destination_root)?;

    let extracted_default_dir = destination_root.join(DEFAULT_MODEL_BASENAME);
    if extracted_default_dir != model_dir
        && has_required_model_files(&extracted_default_dir)
        && !model_dir.exists()
    {
        fs::rename(&extracted_default_dir, model_dir).with_context(|| {
            format!(
                "failed to move extracted model from {} to {}",
                extracted_default_dir.display(),
                model_dir.display()
            )
        })?;
    }
    Ok(())
}

fn remove_appledouble_files(root: &Path) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(root).with_context(|| format!("failed to read {}", root.display()))? {
        let entry = entry.with_context(|| format!("failed to inspect {}", root.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", path.display()))?;

        if file_type.is_dir() {
            remove_appledouble_files(&path)?;
            continue;
        }

        let is_appledouble = path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.starts_with("._"));
        if is_appledouble {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
    }

    Ok(())
}

fn ensure_model_dir(model_dir: &Path, package_path: &Path, model_url: &str) -> Result<()> {
    if has_required_model_files(model_dir) {
        return Ok(());
    }

    if !package_path.exists() {
        download_model_package(model_url, package_path)?;
    } else {
        eprintln!(
            "model missing; reusing cached package {}",
            package_path.display()
        );
    }

    extract_model_package(model_dir, package_path)?;
    if !has_required_model_files(model_dir) {
        bail!(
            "model directory {} is still incomplete after extraction",
            model_dir.display()
        );
    }
    Ok(())
}

fn probe_audio(path: &Path) -> Result<FfprobeStream> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_streams",
            path.to_string_lossy().as_ref(),
        ])
        .output()
        .with_context(|| "failed to run ffprobe; install ffmpeg/ffprobe to inspect audio")?;
    if !output.status.success() {
        bail!(
            "ffprobe failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let parsed: FfprobeOutput = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("failed to parse ffprobe output for {}", path.display()))?;
    parsed
        .streams
        .into_iter()
        .find(|stream| stream.codec_type.as_deref() == Some("audio"))
        .with_context(|| format!("no audio stream found in {}", path.display()))
}

fn is_supported_audio(stream: &FfprobeStream) -> bool {
    let sample_rate_ok = stream.sample_rate.as_deref() == Some("16000");
    let channels_ok = stream.channels == Some(1);
    let codec_ok = stream.codec_name.as_deref() == Some("pcm_s16le");
    let bits_ok = stream.bits_per_sample == Some(16) || stream.sample_fmt.as_deref() == Some("s16");
    sample_rate_ok && channels_ok && codec_ok && bits_ok
}

fn normalize_audio(input_path: &Path) -> Result<PreparedAudio> {
    let stream = probe_audio(input_path)?;
    if is_supported_audio(&stream) {
        return Ok(PreparedAudio {
            wav_path: input_path.to_path_buf(),
            normalized: false,
            _tempdir: None,
        });
    }

    let tempdir = tempfile::tempdir().context("failed to create temp dir for normalized audio")?;
    let stem = input_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("audio");
    let normalized_path = tempdir.path().join(format!("{stem}.16k_mono.wav"));

    eprintln!("normalizing audio...");
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostats",
            "-y",
            "-i",
            input_path.to_string_lossy().as_ref(),
            "-vn",
            "-sn",
            "-dn",
            "-ar",
            "16000",
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
            normalized_path.to_string_lossy().as_ref(),
        ])
        .status()
        .with_context(|| "failed to run ffmpeg; install ffmpeg to normalize audio")?;
    if !status.success() {
        bail!("ffmpeg failed while converting {}", input_path.display());
    }

    Ok(PreparedAudio {
        wav_path: normalized_path,
        normalized: true,
        _tempdir: Some(tempdir),
    })
}

fn render_chunk_progress(prefix: &str, current: usize, total: usize) -> String {
    let completed_chunks = current.saturating_sub(1).min(total);
    let filled = (completed_chunks * PROGRESS_BAR_WIDTH)
        .checked_div(total)
        .unwrap_or(PROGRESS_BAR_WIDTH);
    let empty = PROGRESS_BAR_WIDTH.saturating_sub(filled);
    let bar = format!("{}{}", "█".repeat(filled), "▒".repeat(empty));
    format!("{prefix} {bar} transcribing chunk {current}/{total}")
}

fn render_chunk_progress_done(total: usize) -> String {
    let bar = "█".repeat(PROGRESS_BAR_WIDTH);
    format!("✓ {bar} transcribing chunk {total}/{total}")
}

impl ChunkProgressReporter {
    fn start(total_chunks: usize) -> Self {
        let current_chunk = Arc::new(AtomicUsize::new(1));
        let stop = Arc::new(AtomicBool::new(false));

        if io::stderr().is_terminal() {
            let current_chunk_for_thread = Arc::clone(&current_chunk);
            let stop_for_thread = Arc::clone(&stop);
            let handle = thread::spawn(move || {
                let mut frame_index = 0usize;
                loop {
                    let current = current_chunk_for_thread
                        .load(Ordering::Relaxed)
                        .clamp(1, total_chunks.max(1));
                    let line =
                        render_chunk_progress(SPINNER_FRAMES[frame_index], current, total_chunks);
                    eprint!("\r{line}");
                    let _ = io::stderr().flush();

                    if stop_for_thread.load(Ordering::Relaxed) {
                        break;
                    }

                    frame_index = (frame_index + 1) % SPINNER_FRAMES.len();
                    thread::sleep(Duration::from_millis(80));
                }
            });

            Self {
                total_chunks,
                current_chunk,
                stop,
                handle: Some(handle),
            }
        } else {
            eprintln!("transcribing {total_chunks} chunks...");
            Self {
                total_chunks,
                current_chunk,
                stop,
                handle: None,
            }
        }
    }

    fn set_current_chunk(&self, current: usize) {
        let current = current.clamp(1, self.total_chunks.max(1));
        self.current_chunk.store(current, Ordering::Relaxed);
        if self.handle.is_none() {
            eprintln!("transcribing chunk {current}/{}", self.total_chunks);
        }
    }

    fn finish(self) {
        self.current_chunk
            .store(self.total_chunks.max(1), Ordering::Relaxed);
        self.stop.store(true, Ordering::Relaxed);

        if let Some(handle) = self.handle {
            let _ = handle.join();
            eprintln!("\r{}", render_chunk_progress_done(self.total_chunks));
        } else {
            eprintln!("done transcribing {} chunks", self.total_chunks);
        }
    }
}

fn build_chunk_ranges(
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

fn merge_chunk_texts(left: &str, right: &str) -> String {
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

fn transcript_segments_from_text(text: &str, start_s: f64, end_s: f64) -> Vec<TranscriptSegment> {
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

fn merge_transcript_segments(
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
        SAMPLE_RATE,
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
        transcription.offset_timestamps(start as f32 / SAMPLE_RATE as f32);

        let text = transcription.text.trim().to_string();
        merged_text = merge_chunk_texts(&merged_text, &text);
        merge_transcript_segments(
            &mut merged_segments,
            transcript_segments_from_transcription(
                &transcription,
                start as f64 / SAMPLE_RATE as f64,
                end as f64 / SAMPLE_RATE as f64,
            ),
        );

        chunks.push(BenchmarkChunk {
            index,
            start_s: start as f64 / SAMPLE_RATE as f64,
            end_s: end as f64 / SAMPLE_RATE as f64,
            audio_seconds: (end - start) as f64 / SAMPLE_RATE as f64,
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

fn main() -> Result<()> {
    let raw_args: Vec<String> = env::args().skip(1).collect();
    if raw_args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", usage());
        return Ok(());
    }
    if raw_args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{}", version_string());
        return Ok(());
    }

    let args = parse_args(&raw_args)?;
    let input_source = infer_input_source(&args.input);
    let output_path = if args.output_to_stdout {
        None
    } else {
        Some(args.output_path.clone().unwrap_or_else(|| {
            default_output_path_for_input(&input_source, None, args.output_format)
        }))
    };

    if let InputSource::RemoteUrl(url) = &input_source {
        let info = load_remote_video_info(url)?;
        let resolved_output_path = if args.output_to_stdout {
            None
        } else {
            Some(args.output_path.clone().unwrap_or_else(|| {
                default_output_path_for_input(
                    &input_source,
                    info.title.as_deref().or(info.id.as_deref()),
                    args.output_format,
                )
            }))
        };

        if !args.prefer_local_for_remote && args.diarization.is_none() {
            if let Some(transcript) = download_manual_remote_transcript(url, &info)? {
                let result = BenchmarkResult {
                    segments: (!transcript.segments.is_empty()).then_some(transcript.segments),
                    input_source: url.clone(),
                    model_dir: String::new(),
                    audio_path: url.clone(),
                    prepared_audio_path: String::new(),
                    used_ffmpeg_normalization: false,
                    used_local_model: false,
                    transcript_source: "remote-manual-subtitle".to_string(),
                    audio_seconds: 0.0,
                    load_seconds: 0.0,
                    transcribe_seconds: 0.0,
                    total_inside_seconds: 0.0,
                    seconds_per_audio_second: 0.0,
                    realtime_speedup: 0.0,
                    text: transcript.text.clone(),
                    chunk_seconds: None,
                    chunk_overlap_seconds: 0.0,
                    chunk_count: 1,
                    chunks: vec![BenchmarkChunk {
                        index: 0,
                        start_s: 0.0,
                        end_s: 0.0,
                        audio_seconds: 0.0,
                        transcribe_seconds: 0.0,
                        text: transcript.text.clone(),
                    }],
                    speaker_diarization: None,
                };
                let output = render_output(&result, args.output_format)?;
                if args.output_to_stdout {
                    println!("{output}");
                    return Ok(());
                }
                let output_path = resolved_output_path
                    .as_ref()
                    .context("missing output path for file output mode")?;
                let current_dir =
                    env::current_dir().context("failed to resolve current working directory")?;
                let absolute_output_path = write_output_file(&output, output_path, &current_dir)?;
                emit_file_output_completion(
                    &mut io::stdout(),
                    &mut io::stderr(),
                    &absolute_output_path,
                    "done: used manual subtitles via yt-dlp",
                )?;
                return Ok(());
            }
        }

        let downloaded_audio = download_remote_audio(url, &info)?;
        return transcribe_audio_input(
            &args,
            url,
            &downloaded_audio.audio_path,
            resolved_output_path,
            "downloaded-audio-local-model",
        );
    }

    let local_path = match &input_source {
        InputSource::LocalPath(path) => path.clone(),
        InputSource::RemoteUrl(_) => unreachable!(),
    };
    transcribe_audio_input(
        &args,
        &args.input,
        &local_path,
        output_path,
        "local-audio-local-model",
    )
}

fn transcribe_audio_input(
    args: &CliArgs,
    input_source: &str,
    audio_path: &Path,
    output_path: Option<PathBuf>,
    transcript_source: &str,
) -> Result<()> {
    ensure_model_dir(&args.model_dir, &args.model_package, &args.model_url)?;
    let prepared_audio = normalize_audio(audio_path)?;

    let samples = read_wav_samples(&prepared_audio.wav_path).with_context(|| {
        format!(
            "failed to read WAV samples from {}",
            prepared_audio.wav_path.display()
        )
    })?;
    let audio_seconds = samples.len() as f64 / SAMPLE_RATE as f64;

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
    let result = BenchmarkResult {
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
        seconds_per_audio_second: total_inside_seconds / audio_seconds,
        realtime_speedup: audio_seconds / total_inside_seconds,
        text,
        chunk_seconds: args.chunk_seconds,
        chunk_overlap_seconds: args.chunk_overlap_seconds,
        chunk_count: chunks.len(),
        chunks,
        segments: (!segments.is_empty()).then_some(segments),
        speaker_diarization,
    };

    let output = render_output(&result, args.output_format)?;
    if args.output_to_stdout {
        println!("{output}");
        return Ok(());
    }

    let output_path = output_path
        .as_ref()
        .context("missing output path for file output mode")?;
    let current_dir = env::current_dir().context("failed to resolve current working directory")?;
    let absolute_output_path = write_output_file(&output, output_path, &current_dir)?;
    emit_file_output_completion(
        &mut io::stdout(),
        &mut io::stderr(),
        &absolute_output_path,
        &format!("done in {:.2}x real-time", result.realtime_speedup),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        build_chunk_ranges, default_model_dir, default_model_package, default_output_path,
        default_output_path_for_input, emit_file_output_completion, format_hhmmss,
        infer_input_source, is_supported_audio, merge_chunk_texts, merge_transcript_segments,
        normalize_speaker_label, normalized_subtitle_segments, parse_args, parse_json3_segments,
        parse_vtt_segments, parse_vtt_text, pick_manual_subtitle_language,
        remove_appledouble_files, render_chunk_progress, render_chunk_progress_done, render_output,
        render_speaker_script_lines, resolve_absolute_output_path, transcript_segments_from_text,
        version_string, write_output_file, BenchmarkResult, DiarizationBackend, DiarizationRequest,
        FfprobeStream, InputSource, OutputFormat, ScriptFormat, SubtitleFormat, TextFormat,
        TranscriptSegment, YtDlpSubtitleTrack, YtDlpVideoInfo, DEFAULT_MODEL_BASENAME,
        DEFAULT_MODEL_PACKAGE_NAME, SUBTITLE_MAX_CHARS,
    };
    use std::collections::BTreeMap;
    use std::io::Cursor;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    fn path_ends_with(path: &Path, suffix: &[&str]) -> bool {
        let mut current = path;
        for expected in suffix.iter().rev() {
            let Some(name) = current.file_name().and_then(|value| value.to_str()) else {
                return false;
            };
            if name != *expected {
                return false;
            }
            let Some(parent) = current.parent() else {
                return false;
            };
            current = parent;
        }
        true
    }

    fn sample_result(segments: Vec<TranscriptSegment>) -> BenchmarkResult {
        let text = segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        BenchmarkResult {
            input_source: "input".to_string(),
            model_dir: "model".to_string(),
            audio_path: "audio".to_string(),
            prepared_audio_path: "prepared".to_string(),
            used_ffmpeg_normalization: false,
            used_local_model: true,
            transcript_source: "local-audio-local-model".to_string(),
            audio_seconds: 10.0,
            load_seconds: 1.0,
            transcribe_seconds: 1.0,
            total_inside_seconds: 2.0,
            seconds_per_audio_second: 0.2,
            realtime_speedup: 5.0,
            text,
            chunk_seconds: Some(120.0),
            chunk_overlap_seconds: 2.0,
            chunk_count: 1,
            chunks: vec![],
            segments: Some(segments),
            speaker_diarization: None,
        }
    }

    #[test]
    fn parse_args_defaults_to_easy_mode() {
        let args = vec!["audio.mp3".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.input, "audio.mp3");
        assert_eq!(parsed.output_path, None);
        assert!(!parsed.output_to_stdout);
        assert_eq!(parsed.output_format, OutputFormat::Json);
        assert_eq!(parsed.chunk_seconds, Some(120.0));
        assert_eq!(parsed.chunk_overlap_seconds, 2.0);
        assert_eq!(parsed.diarization, None);
        assert!(path_ends_with(
            &parsed.model_dir,
            &["models", DEFAULT_MODEL_BASENAME]
        ));
        assert!(path_ends_with(
            &parsed.model_package,
            &[DEFAULT_MODEL_PACKAGE_NAME]
        ));
    }

    #[test]
    fn default_model_paths_use_persistent_user_dirs() {
        assert!(path_ends_with(
            &default_model_dir(),
            &["models", DEFAULT_MODEL_BASENAME]
        ));
        assert!(path_ends_with(
            &default_model_package(),
            &[DEFAULT_MODEL_PACKAGE_NAME]
        ));
    }

    #[test]
    fn remove_appledouble_files_cleans_resource_forks_only() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let keep = nested.join("encoder-model.int8.onnx");
        let remove = nested.join("._encoder-model.int8.onnx");
        std::fs::write(&keep, "ok").unwrap();
        std::fs::write(&remove, "junk").unwrap();

        remove_appledouble_files(dir.path()).unwrap();

        assert!(keep.exists());
        assert!(!remove.exists());
    }

    #[test]
    fn parse_args_accepts_optional_output_and_chunk_seconds() {
        let args = vec![
            "audio.wav".to_string(),
            "out.json".to_string(),
            "--model-dir".to_string(),
            "custom-model".to_string(),
            "--chunk-seconds".to_string(),
            "60".to_string(),
            "--chunk-overlap-seconds".to_string(),
            "1.5".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.output_path, Some(PathBuf::from("out.json")));
        assert_eq!(parsed.model_dir, PathBuf::from("custom-model"));
        assert_eq!(parsed.chunk_seconds, Some(60.0));
        assert_eq!(parsed.chunk_overlap_seconds, 1.5);
    }

    #[test]
    fn parse_args_supports_stdout_shortcut() {
        let args = vec!["audio.wav".to_string(), "-".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(parsed.output_to_stdout);
        assert_eq!(parsed.output_path, None);
    }

    #[test]
    fn parse_args_supports_stdout_flag() {
        let args = vec!["audio.wav".to_string(), "--stdout".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(parsed.output_to_stdout);
        assert_eq!(parsed.output_path, None);
    }

    #[test]
    fn parse_args_supports_script_output_defaulting_to_timestamps() {
        let args = vec!["audio.wav".to_string(), "--script".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.output_format,
            OutputFormat::Script(ScriptFormat::Timestamped)
        );
    }

    #[test]
    fn parse_args_supports_plain_script_output() {
        let args = vec![
            "audio.wav".to_string(),
            "--script".to_string(),
            "plain".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.output_format,
            OutputFormat::Script(ScriptFormat::Plain)
        );
    }

    #[test]
    fn parse_args_supports_plain_text_output() {
        let args = vec!["audio.wav".to_string(), "--text".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.output_format, OutputFormat::Text(TextFormat::Plain));
    }

    #[test]
    fn parse_args_supports_timestamped_text_output() {
        let args = vec![
            "audio.wav".to_string(),
            "--text".to_string(),
            "timestamps".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.output_format,
            OutputFormat::Text(TextFormat::Timestamped)
        );
    }

    #[test]
    fn parse_args_supports_srt_output() {
        let args = vec!["audio.wav".to_string(), "--srt".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.output_format,
            OutputFormat::Subtitle(SubtitleFormat::Srt)
        );
    }

    #[test]
    fn parse_args_supports_vtt_output() {
        let args = vec!["audio.wav".to_string(), "--vtt".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.output_format,
            OutputFormat::Subtitle(SubtitleFormat::Vtt)
        );
    }

    #[test]
    fn parse_args_supports_prefer_local_for_remote() {
        let args = vec![
            "https://www.youtube.com/watch?v=QSdh8Gj0mEg".to_string(),
            "--prefer-local-for-remote".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert!(parsed.prefer_local_for_remote);
    }

    #[test]
    fn parse_args_supports_optional_diarization() {
        let args = vec!["audio.wav".to_string(), "-d".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.diarization,
            Some(DiarizationRequest {
                backend: DiarizationBackend::Coreml,
                num_speakers: None,
                threshold: None,
            })
        );
    }

    #[test]
    fn parse_args_passes_num_speakers_when_diarization_is_enabled() {
        let args = vec![
            "audio.wav".to_string(),
            "--diarize".to_string(),
            "-n".to_string(),
            "2".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.diarization,
            Some(DiarizationRequest {
                backend: DiarizationBackend::Coreml,
                num_speakers: Some(2),
                threshold: None,
            })
        );
    }

    #[test]
    fn parse_args_rejects_num_speakers_without_diarization() {
        let args = vec!["audio.wav".to_string(), "-n".to_string(), "2".to_string()];
        let err = parse_args(&args).unwrap_err();
        assert!(err
            .to_string()
            .contains("--num-speakers requires -d or --diarize"));
    }

    #[test]
    fn parse_args_supports_num_speakers_long_flag() {
        let args = vec![
            "audio.wav".to_string(),
            "-d".to_string(),
            "--num-speakers".to_string(),
            "2".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.diarization,
            Some(DiarizationRequest {
                backend: DiarizationBackend::Coreml,
                num_speakers: Some(2),
                threshold: None,
            })
        );
    }

    #[test]
    fn parse_args_supports_diarization_backend_argument() {
        let args = vec![
            "audio.wav".to_string(),
            "-d".to_string(),
            "lseend-dihard3".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.diarization,
            Some(DiarizationRequest {
                backend: DiarizationBackend::LseendDihard3,
                num_speakers: None,
                threshold: None,
            })
        );
    }

    #[test]
    fn parse_args_supports_threshold_short_flag() {
        let args = vec![
            "audio.wav".to_string(),
            "-d".to_string(),
            "lseend-dihard3".to_string(),
            "-t".to_string(),
            "0.3".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.diarization,
            Some(DiarizationRequest {
                backend: DiarizationBackend::LseendDihard3,
                num_speakers: None,
                threshold: Some(0.3),
            })
        );
    }

    #[test]
    fn parse_args_rejects_threshold_without_diarization() {
        let args = vec![
            "audio.wav".to_string(),
            "--threshold".to_string(),
            "0.3".to_string(),
        ];
        let err = parse_args(&args).unwrap_err();
        assert!(err
            .to_string()
            .contains("--threshold requires -d or --diarize"));
    }

    #[test]
    fn parse_args_rejects_num_speakers_with_lseend_backend() {
        let args = vec![
            "audio.wav".to_string(),
            "-d".to_string(),
            "lseend-dihard3".to_string(),
            "--num-speakers".to_string(),
            "2".to_string(),
        ];
        let err = parse_args(&args).unwrap_err();
        assert!(err.to_string().contains(
            "--num-speakers is not supported with lseend-dihard3; use -t or --threshold instead"
        ));
    }

    #[test]
    fn version_string_matches_package_version() {
        assert_eq!(
            version_string(),
            format!("fscript {}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn parse_args_can_disable_chunking() {
        let args = vec![
            "audio.wav".to_string(),
            "--chunk-seconds".to_string(),
            "0".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.chunk_seconds, None);
        assert_eq!(parsed.chunk_overlap_seconds, 0.0);
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
    fn render_chunk_progress_formats_spinner_and_bar() {
        let rendered = render_chunk_progress("⠟", 12, 23);
        assert_eq!(rendered, "⠟ █████████▒▒▒▒▒▒▒▒▒▒▒ transcribing chunk 12/23");
    }

    #[test]
    fn render_chunk_progress_done_shows_complete_bar() {
        let rendered = render_chunk_progress_done(23);
        assert_eq!(rendered, "✓ ████████████████████ transcribing chunk 23/23");
    }

    #[test]
    fn supported_audio_requires_16k_mono_pcm_s16le() {
        let ok = FfprobeStream {
            codec_type: Some("audio".to_string()),
            codec_name: Some("pcm_s16le".to_string()),
            sample_rate: Some("16000".to_string()),
            channels: Some(1),
            bits_per_sample: Some(16),
            sample_fmt: Some("s16".to_string()),
        };
        let bad = FfprobeStream {
            codec_type: Some("audio".to_string()),
            codec_name: Some("mp3".to_string()),
            sample_rate: Some("44100".to_string()),
            channels: Some(2),
            bits_per_sample: Some(0),
            sample_fmt: Some("fltp".to_string()),
        };
        assert!(is_supported_audio(&ok));
        assert!(!is_supported_audio(&bad));
    }

    #[test]
    fn default_output_path_stays_next_to_source_audio() {
        let path = PathBuf::from("/tmp/folder/audio.file.mp3");
        let output = default_output_path(&path, OutputFormat::Json);
        assert_eq!(
            output,
            PathBuf::from("/tmp/folder/audio.file.transcript.json")
        );
    }

    #[test]
    fn default_script_output_path_uses_script_extension() {
        let path = PathBuf::from("/tmp/folder/audio.file.mp3");
        let output = default_output_path(&path, OutputFormat::Script(ScriptFormat::Timestamped));
        assert_eq!(output, PathBuf::from("/tmp/folder/audio.file.script.txt"));
    }

    #[test]
    fn default_text_output_path_uses_transcript_text_extension() {
        let path = PathBuf::from("/tmp/folder/audio.file.mp3");
        let output = default_output_path(&path, OutputFormat::Text(TextFormat::Plain));
        assert_eq!(
            output,
            PathBuf::from("/tmp/folder/audio.file.transcript.txt")
        );
    }

    #[test]
    fn default_srt_output_path_uses_srt_extension() {
        let path = PathBuf::from("/tmp/folder/audio.file.mp3");
        let output = default_output_path(&path, OutputFormat::Subtitle(SubtitleFormat::Srt));
        assert_eq!(output, PathBuf::from("/tmp/folder/audio.file.srt"));
    }

    #[test]
    fn default_vtt_output_path_uses_vtt_extension() {
        let path = PathBuf::from("/tmp/folder/audio.file.mp3");
        let output = default_output_path(&path, OutputFormat::Subtitle(SubtitleFormat::Vtt));
        assert_eq!(output, PathBuf::from("/tmp/folder/audio.file.vtt"));
    }

    #[test]
    fn infer_input_source_detects_remote_urls() {
        assert_eq!(
            infer_input_source("https://www.youtube.com/watch?v=QSdh8Gj0mEg"),
            InputSource::RemoteUrl("https://www.youtube.com/watch?v=QSdh8Gj0mEg".to_string())
        );
        assert_eq!(
            infer_input_source("lecture.mp3"),
            InputSource::LocalPath(PathBuf::from("lecture.mp3"))
        );
    }

    #[test]
    fn default_output_path_for_remote_url_uses_video_title() {
        let output = default_output_path_for_input(
            &InputSource::RemoteUrl("https://youtu.be/demo".to_string()),
            Some("TED Talk: Future / Now"),
            OutputFormat::Json,
        );
        assert_eq!(output, PathBuf::from("TED_Talk_Future_Now.transcript.json"));
    }

    #[test]
    fn default_script_output_path_for_remote_url_uses_video_title() {
        let output = default_output_path_for_input(
            &InputSource::RemoteUrl("https://youtu.be/demo".to_string()),
            Some("TED Talk: Future / Now"),
            OutputFormat::Script(ScriptFormat::Timestamped),
        );
        assert_eq!(output, PathBuf::from("TED_Talk_Future_Now.script.txt"));
    }

    #[test]
    fn default_text_output_path_for_remote_url_uses_video_title() {
        let output = default_output_path_for_input(
            &InputSource::RemoteUrl("https://youtu.be/demo".to_string()),
            Some("TED Talk: Future / Now"),
            OutputFormat::Text(TextFormat::Plain),
        );
        assert_eq!(output, PathBuf::from("TED_Talk_Future_Now.transcript.txt"));
    }

    #[test]
    fn default_srt_output_path_for_remote_url_uses_video_title() {
        let output = default_output_path_for_input(
            &InputSource::RemoteUrl("https://youtu.be/demo".to_string()),
            Some("TED Talk: Future / Now"),
            OutputFormat::Subtitle(SubtitleFormat::Srt),
        );
        assert_eq!(output, PathBuf::from("TED_Talk_Future_Now.srt"));
    }

    #[test]
    fn default_vtt_output_path_for_remote_url_uses_video_title() {
        let output = default_output_path_for_input(
            &InputSource::RemoteUrl("https://youtu.be/demo".to_string()),
            Some("TED Talk: Future / Now"),
            OutputFormat::Subtitle(SubtitleFormat::Vtt),
        );
        assert_eq!(output, PathBuf::from("TED_Talk_Future_Now.vtt"));
    }

    #[test]
    fn pick_manual_subtitle_language_prefers_manual_tracks() {
        let mut subtitles = BTreeMap::new();
        subtitles.insert(
            "en".to_string(),
            vec![YtDlpSubtitleTrack {
                ext: Some("vtt".to_string()),
            }],
        );
        subtitles.insert(
            "pt-BR".to_string(),
            vec![YtDlpSubtitleTrack {
                ext: Some("json3".to_string()),
            }],
        );

        let info = YtDlpVideoInfo {
            id: Some("abc123".to_string()),
            title: Some("Demo".to_string()),
            subtitles: Some(subtitles),
        };

        assert_eq!(
            pick_manual_subtitle_language(&info),
            Some("pt-BR".to_string())
        );
    }

    #[test]
    fn parse_vtt_text_strips_headers_and_timestamps() {
        let text = parse_vtt_text(
            "WEBVTT\nKind: captions\nLanguage: en\n\n1\n00:00:00.000 --> 00:00:01.000\nHello world\n\n2\n00:00:01.000 --> 00:00:02.000\nSecond line",
        );
        assert_eq!(text, "Hello world Second line");
    }

    #[test]
    fn parse_vtt_segments_extracts_timed_cues() {
        let segments = parse_vtt_segments(
            "WEBVTT\n\nintro\n00:00:00.000 --> 00:00:02.500 align:start position:0%\nHello\nworld\n\n00:00:03.000 --> 00:00:04.250\nSecond line",
        );
        assert_eq!(
            segments,
            vec![
                TranscriptSegment {
                    start_s: 0.0,
                    end_s: 2.5,
                    text: "Hello world".to_string(),
                    speaker: None,
                },
                TranscriptSegment {
                    start_s: 3.0,
                    end_s: 4.25,
                    text: "Second line".to_string(),
                    speaker: None,
                },
            ]
        );
    }

    #[test]
    fn parse_json3_segments_extracts_timed_cues() {
        let segments = parse_json3_segments(
            r#"{
              "events": [
                {
                  "tStartMs": 1250,
                  "dDurationMs": 1750,
                  "segs": [{"utf8": "Hello "}, {"utf8": "world"}]
                },
                {
                  "tStartMs": 4000,
                  "dDurationMs": 500,
                  "segs": [{"utf8": "Second line"}]
                }
              ]
            }"#,
        )
        .unwrap();
        assert_eq!(
            segments,
            vec![
                TranscriptSegment {
                    start_s: 1.25,
                    end_s: 3.0,
                    text: "Hello world".to_string(),
                    speaker: None,
                },
                TranscriptSegment {
                    start_s: 4.0,
                    end_s: 4.5,
                    text: "Second line".to_string(),
                    speaker: None,
                },
            ]
        );
    }

    #[test]
    fn write_output_file_returns_absolute_path_for_relative_output() {
        let temp = tempdir().unwrap();
        let cwd = temp.path();
        let expected = cwd.join("nested/out.json");

        let output = write_output_file("{\"ok\":true}", Path::new("nested/out.json"), cwd).unwrap();

        assert_eq!(output, std::fs::canonicalize(&expected).unwrap());
        assert_eq!(
            std::fs::read_to_string(expected).unwrap(),
            "{\"ok\":true}\n"
        );
    }

    #[test]
    fn resolve_absolute_output_path_keeps_absolute_paths() {
        let temp = tempdir().unwrap();
        let output = temp.path().join("result.json");
        std::fs::write(&output, "{}\n").unwrap();

        assert_eq!(
            resolve_absolute_output_path(&output, Path::new("/tmp/ignored")).unwrap(),
            std::fs::canonicalize(&output).unwrap()
        );
    }

    #[test]
    fn emit_file_output_completion_sends_path_to_stdout_and_status_to_stderr() {
        let mut stdout = Cursor::new(Vec::new());
        let mut stderr = Cursor::new(Vec::new());
        let path = Path::new("/tmp/example.transcript.json");

        emit_file_output_completion(&mut stdout, &mut stderr, path, "done in 11.11x real-time")
            .unwrap();

        assert_eq!(
            String::from_utf8(stdout.into_inner()).unwrap(),
            "/tmp/example.transcript.json\n"
        );
        assert_eq!(
            String::from_utf8(stderr.into_inner()).unwrap(),
            "done in 11.11x real-time\n"
        );
    }

    #[test]
    fn normalize_speaker_label_formats_simple_labels() {
        assert_eq!(normalize_speaker_label("S1", "UNKNOWN"), "SPEAKER_01");
        assert_eq!(
            normalize_speaker_label("SPEAKER_2", "UNKNOWN"),
            "SPEAKER_02"
        );
        assert_eq!(normalize_speaker_label("", "UNKNOWN"), "UNKNOWN");
        assert_eq!(normalize_speaker_label("Speaker 0", "UNKNOWN"), "Speaker 0");
    }

    #[test]
    fn format_hhmmss_truncates_to_whole_seconds() {
        assert_eq!(format_hhmmss(0.0), "00:00:00");
        assert_eq!(format_hhmmss(65.9), "00:01:05");
        assert_eq!(format_hhmmss(3_661.2), "01:01:01");
    }

    #[test]
    fn render_speaker_script_lines_merges_consecutive_turns() {
        let segments = vec![
            TranscriptSegment {
                start_s: 5.0,
                end_s: 6.0,
                text: "Oi.".to_string(),
                speaker: Some("S1".to_string()),
            },
            TranscriptSegment {
                start_s: 6.0,
                end_s: 7.0,
                text: "Tudo bem?".to_string(),
                speaker: Some("S1".to_string()),
            },
            TranscriptSegment {
                start_s: 8.0,
                end_s: 9.0,
                text: "Tudo.".to_string(),
                speaker: Some("S2".to_string()),
            },
        ];

        assert_eq!(
            render_speaker_script_lines(&segments, ScriptFormat::Plain, true, "UNKNOWN"),
            vec![
                "SPEAKER_01: Oi. Tudo bem?".to_string(),
                "SPEAKER_02: Tudo.".to_string(),
            ]
        );
    }

    #[test]
    fn render_speaker_script_lines_can_include_timestamps() {
        let segments = vec![
            TranscriptSegment {
                start_s: 65.9,
                end_s: 67.0,
                text: "Primeira.".to_string(),
                speaker: Some("S1".to_string()),
            },
            TranscriptSegment {
                start_s: 68.0,
                end_s: 70.0,
                text: "Segunda.".to_string(),
                speaker: Some("S1".to_string()),
            },
        ];

        assert_eq!(
            render_speaker_script_lines(&segments, ScriptFormat::Timestamped, false, "UNKNOWN"),
            vec![
                "00:01:05 - SPEAKER_01: Primeira.".to_string(),
                "00:01:08 - SPEAKER_01: Segunda.".to_string(),
            ]
        );
    }

    #[test]
    fn render_output_supports_plain_text() {
        let result = sample_result(vec![
            TranscriptSegment {
                start_s: 0.0,
                end_s: 1.0,
                text: "Primeira frase.".to_string(),
                speaker: None,
            },
            TranscriptSegment {
                start_s: 1.0,
                end_s: 2.0,
                text: "Segunda frase.".to_string(),
                speaker: None,
            },
        ]);

        assert_eq!(
            render_output(&result, OutputFormat::Text(TextFormat::Plain)).unwrap(),
            "Primeira frase. Segunda frase."
        );
    }

    #[test]
    fn render_output_supports_timestamped_text() {
        let result = sample_result(vec![
            TranscriptSegment {
                start_s: 5.0,
                end_s: 6.0,
                text: "Primeira frase.".to_string(),
                speaker: None,
            },
            TranscriptSegment {
                start_s: 65.9,
                end_s: 67.0,
                text: "Segunda frase.".to_string(),
                speaker: None,
            },
        ]);

        assert_eq!(
            render_output(&result, OutputFormat::Text(TextFormat::Timestamped)).unwrap(),
            "00:00:05 - Primeira frase.\n00:01:05 - Segunda frase."
        );
    }

    #[test]
    fn render_output_supports_srt_subtitles() {
        let result = sample_result(vec![
            TranscriptSegment {
                start_s: 1.25,
                end_s: 3.0,
                text: "Hello world".to_string(),
                speaker: Some("S1".to_string()),
            },
            TranscriptSegment {
                start_s: 4.0,
                end_s: 4.5,
                text: "Second line".to_string(),
                speaker: None,
            },
        ]);

        assert_eq!(
            render_output(&result, OutputFormat::Subtitle(SubtitleFormat::Srt)).unwrap(),
            "1\n00:00:01,250 --> 00:00:03,000\nSPEAKER_01: Hello world\n\n2\n00:00:04,000 --> 00:00:04,500\nSecond line"
        );
    }

    #[test]
    fn render_output_clips_overlong_subtitle_duration() {
        let result = sample_result(vec![
            TranscriptSegment {
                start_s: 150.0,
                end_s: 228.0,
                text: "Chissa come saranno ridotto.".to_string(),
                speaker: None,
            },
            TranscriptSegment {
                start_s: 228.0,
                end_s: 233.0,
                text: "Lo sai che non devi allontanarti.".to_string(),
                speaker: None,
            },
        ]);

        let rendered = render_output(&result, OutputFormat::Subtitle(SubtitleFormat::Srt)).unwrap();
        assert!(rendered.contains("1\n00:02:30,000 --> 00:03:47,950\nChissa come saranno ridotto."));
        assert!(rendered
            .contains("2\n00:03:48,000 --> 00:03:53,000\nLo sai che non devi allontanarti."));
    }

    #[test]
    fn normalized_subtitle_segments_preserve_end_for_single_long_segment() {
        let normalized = normalized_subtitle_segments(&[TranscriptSegment {
            start_s: 150.0,
            end_s: 228.0,
            text: "Chissa come saranno ridotto.".to_string(),
            speaker: None,
        }]);

        assert_eq!(
            normalized,
            vec![TranscriptSegment {
                start_s: 150.0,
                end_s: 228.0,
                text: "Chissa come saranno ridotto.".to_string(),
                speaker: None,
            }]
        );
    }

    #[test]
    fn normalized_subtitle_segments_split_run_on_fallback_text() {
        let normalized = normalized_subtitle_segments(&[TranscriptSegment {
            start_s: 10.0,
            end_s: 70.0,
            text: "um texto bastante longo sem pontuacao que continua crescendo com varias palavras para simular uma saida grosseira do asr dentro de um chunk muito grande e precisa ser quebrado em legendas menores sem ficar preso em um unico cue enorme na tela".to_string(),
            speaker: None,
        }]);

        assert!(normalized.len() >= 3);
        assert!(normalized
            .iter()
            .all(|segment| segment.text.len() <= SUBTITLE_MAX_CHARS));
        assert_eq!(normalized.first().unwrap().start_s, 10.0);
        assert!(normalized.last().unwrap().start_s > 40.0);
        assert!(normalized
            .windows(2)
            .all(|pair| pair[0].end_s <= pair[1].start_s));
    }

    #[test]
    fn normalized_subtitle_segments_spread_split_cues_across_original_window() {
        let normalized = normalized_subtitle_segments(&[
            TranscriptSegment {
                start_s: 100.0,
                end_s: 160.0,
                text: "Primeira frase bem maior do que um cue de legenda normal para testar a divisao. Segunda frase tambem maior do que um cue curto para forcar outra divisao. Terceira frase fechando o bloco para verificar distribuicao no tempo.".to_string(),
                speaker: None,
            },
            TranscriptSegment {
                start_s: 162.0,
                end_s: 165.0,
                text: "proximo bloco".to_string(),
                speaker: None,
            },
        ]);

        assert!(normalized.len() >= 4);
        assert_eq!(normalized[0].start_s, 100.0);
        assert!(normalized[1].start_s > 110.0);
        assert!(normalized[2].start_s > 125.0);
        assert!(normalized[2].end_s < 162.0);
        assert_eq!(normalized.last().unwrap().text, "proximo bloco");
    }

    #[test]
    fn render_output_supports_vtt_subtitles() {
        let result = sample_result(vec![TranscriptSegment {
            start_s: 1.25,
            end_s: 3.0,
            text: "Hello world".to_string(),
            speaker: Some("S1".to_string()),
        }]);

        assert_eq!(
            render_output(&result, OutputFormat::Subtitle(SubtitleFormat::Vtt)).unwrap(),
            "WEBVTT\n\n00:00:01.250 --> 00:00:03.000\nSPEAKER_01: Hello world"
        );
    }
}
