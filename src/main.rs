use anyhow::{bail, Context, Result};
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
    prefer_local_for_remote: bool,
    chunk_seconds: Option<f64>,
    chunk_overlap_seconds: f64,
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
}

struct ChunkProgressReporter {
    total_chunks: usize,
    current_chunk: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

fn usage() -> String {
    format!(
        "usage: fscript <audio-or-url> [output.json | - | --stdout] [--prefer-local-for-remote] [--model-dir PATH] [--model-package PATH] [--model-url URL] [--chunk-seconds N] [--chunk-overlap-seconds N]\n\
defaults:\n\
  --model-dir {}\n\
  --model-package {}\n\
  --chunk-seconds 120\n\
  --chunk-overlap-seconds 2",
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

fn default_output_path(audio_path: &Path) -> PathBuf {
    let stem = audio_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("transcript");
    let file_name = format!("{stem}.transcript.json");
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

fn default_output_path_for_input(source: &InputSource, video_title: Option<&str>) -> PathBuf {
    match source {
        InputSource::LocalPath(path) => default_output_path(path),
        InputSource::RemoteUrl(url) => {
            let stem = video_title
                .filter(|value| !value.trim().is_empty())
                .map(sanitize_file_stem)
                .unwrap_or_else(|| sanitize_file_stem(url));
            PathBuf::from(format!("{stem}.transcript.json"))
        }
    }
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

fn parse_vtt_text(contents: &str) -> String {
    contents
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && *line != "WEBVTT"
                && !line.starts_with("NOTE")
                && !line.starts_with("Kind:")
                && !line.starts_with("Language:")
                && !line.starts_with("Style:")
                && !line.contains("-->")
                && !line.chars().all(|ch| ch.is_ascii_digit())
        })
        .collect::<Vec<_>>()
        .join(" ")
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

    let text = if extension == "vtt" {
        parse_vtt_text(
            &fs::read_to_string(&subtitle_path)
                .with_context(|| format!("failed to read {}", subtitle_path.display()))?,
        )
    } else {
        let value: serde_json::Value = serde_json::from_slice(
            &fs::read(&subtitle_path)
                .with_context(|| format!("failed to read {}", subtitle_path.display()))?,
        )
        .with_context(|| format!("failed to parse {}", subtitle_path.display()))?;
        value["events"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|event| event["segs"].as_array())
            .flatten()
            .filter_map(|segment| segment["utf8"].as_str())
            .map(str::trim)
            .filter(|segment| !segment.is_empty() && *segment != "\n")
            .collect::<Vec<_>>()
            .join(" ")
    };

    if text.trim().is_empty() {
        return Ok(None);
    }

    Ok(Some(DirectTranscript { text }))
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
    let mut prefer_local_for_remote = false;
    let mut chunk_seconds_override = None;
    let mut chunk_overlap_seconds_override = None;
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
            "--prefer-local-for-remote" => {
                prefer_local_for_remote = true;
                index += 1;
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
        prefer_local_for_remote,
        chunk_seconds,
        chunk_overlap_seconds,
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

fn transcribe_chunked(
    model: &mut ParakeetModel,
    samples: &[f32],
    chunk_seconds: f64,
    chunk_overlap_seconds: f64,
    params: &ParakeetParams,
) -> Result<(String, Vec<BenchmarkChunk>, f64)> {
    let ranges = build_chunk_ranges(
        samples.len(),
        SAMPLE_RATE,
        chunk_seconds,
        chunk_overlap_seconds,
    )?;
    let mut chunks = Vec::with_capacity(ranges.len());
    let mut merged_text = String::new();
    let mut total_transcribe_seconds = 0.0;
    let total_chunks = ranges.len();
    let progress = ChunkProgressReporter::start(total_chunks);

    for (index, (start, end)) in ranges.into_iter().enumerate() {
        progress.set_current_chunk(index + 1);
        let transcribe_started = Instant::now();
        let transcription = model
            .transcribe_with(&samples[start..end], params)
            .with_context(|| format!("failed chunk {index} ({start}..{end})"))?;
        let transcribe_seconds = transcribe_started.elapsed().as_secs_f64();
        total_transcribe_seconds += transcribe_seconds;

        let text = transcription.text.trim().to_string();
        merged_text = merge_chunk_texts(&merged_text, &text);

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
    Ok((merged_text, chunks, total_transcribe_seconds))
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
        Some(
            args.output_path
                .clone()
                .unwrap_or_else(|| default_output_path_for_input(&input_source, None)),
        )
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
                )
            }))
        };

        if !args.prefer_local_for_remote {
            if let Some(transcript) = download_manual_remote_transcript(url, &info)? {
                let result = BenchmarkResult {
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
                        text: transcript.text,
                    }],
                };
                let json = serde_json::to_string_pretty(&result)?;
                if args.output_to_stdout {
                    println!("{json}");
                    return Ok(());
                }
                let output_path = resolved_output_path
                    .as_ref()
                    .context("missing output path for file output mode")?;
                if let Some(parent) = output_path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)
                            .with_context(|| format!("failed to create {}", parent.display()))?;
                    }
                }
                fs::write(output_path, format!("{json}\n"))
                    .with_context(|| format!("failed to write {}", output_path.display()))?;
                eprintln!(
                    "done: wrote {} (used manual subtitles via yt-dlp)",
                    output_path.display()
                );
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

    let load_start = Instant::now();
    eprintln!("loading model...");
    let mut model = ParakeetModel::load(&args.model_dir, &Quantization::Int8)
        .context("failed to load Parakeet model")?;
    let load_seconds = load_start.elapsed().as_secs_f64();

    let params = ParakeetParams {
        timestamp_granularity: Some(TimestampGranularity::Segment),
        ..Default::default()
    };
    let (text, chunks, transcribe_seconds) = if let Some(chunk_seconds) = args.chunk_seconds {
        transcribe_chunked(
            &mut model,
            &samples,
            chunk_seconds,
            args.chunk_overlap_seconds,
            &params,
        )?
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
        (text, chunks, transcribe_seconds)
    };

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
    };

    let json = serde_json::to_string_pretty(&result)?;
    if args.output_to_stdout {
        println!("{json}");
        return Ok(());
    }

    let output_path = output_path
        .as_ref()
        .context("missing output path for file output mode")?;
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
    }
    fs::write(output_path, format!("{json}\n"))
        .with_context(|| format!("failed to write {}", output_path.display()))?;

    eprintln!(
        "done: wrote {} ({:.2}x real-time)",
        output_path.display(),
        result.realtime_speedup
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        build_chunk_ranges, default_model_dir, default_model_package, default_output_path,
        default_output_path_for_input, infer_input_source, is_supported_audio, merge_chunk_texts,
        parse_args, parse_vtt_text, pick_manual_subtitle_language, remove_appledouble_files,
        render_chunk_progress, render_chunk_progress_done, version_string, FfprobeStream,
        InputSource, YtDlpSubtitleTrack, YtDlpVideoInfo, DEFAULT_MODEL_BASENAME,
        DEFAULT_MODEL_PACKAGE_NAME,
    };
    use std::collections::BTreeMap;
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

    #[test]
    fn parse_args_defaults_to_easy_mode() {
        let args = vec!["audio.mp3".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.input, "audio.mp3");
        assert_eq!(parsed.output_path, None);
        assert!(!parsed.output_to_stdout);
        assert_eq!(parsed.chunk_seconds, Some(120.0));
        assert_eq!(parsed.chunk_overlap_seconds, 2.0);
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
    fn parse_args_supports_prefer_local_for_remote() {
        let args = vec![
            "https://www.youtube.com/watch?v=QSdh8Gj0mEg".to_string(),
            "--prefer-local-for-remote".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert!(parsed.prefer_local_for_remote);
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
        let output = default_output_path(&path);
        assert_eq!(
            output,
            PathBuf::from("/tmp/folder/audio.file.transcript.json")
        );
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
        );
        assert_eq!(output, PathBuf::from("TED_Talk_Future_Now.transcript.json"));
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
}
