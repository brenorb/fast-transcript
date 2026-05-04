use anyhow::{bail, Context, Result};
use directories::ProjectDirs;
use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
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
const DEFAULT_MODEL_URL: &str = "https://blob.handy.computer/parakeet-v3-int8.tar.gz";
const DEFAULT_MODEL_BASENAME: &str = "parakeet-tdt-0.6b-v3-int8";
const DEFAULT_CHUNK_SECONDS: f64 = 120.0;
const DEFAULT_CHUNK_OVERLAP_SECONDS: f64 = 2.0;
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
    audio_path: PathBuf,
    output_path: PathBuf,
    chunk_seconds: Option<f64>,
    chunk_overlap_seconds: f64,
}

#[derive(Debug)]
struct PreparedAudio {
    wav_path: PathBuf,
    normalized: bool,
    _tempdir: Option<TempDir>,
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
    model_dir: String,
    audio_path: String,
    prepared_audio_path: String,
    used_ffmpeg_normalization: bool,
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

fn usage() -> String {
    format!(
        "usage: fscript <audio> [output.json] [--model-dir PATH] [--model-package PATH] [--model-url URL] [--chunk-seconds N] [--chunk-overlap-seconds N]\n\
defaults:\n\
  --model-dir {}\n\
  --model-package {}\n\
  --chunk-seconds 120\n\
  --chunk-overlap-seconds 2",
        default_model_dir().display(),
        default_model_package().display()
    )
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

fn parse_args(raw_args: &[String]) -> Result<CliArgs> {
    if raw_args.is_empty() {
        bail!("{}", usage());
    }

    let mut model_dir = default_model_dir();
    let mut model_package = default_model_package();
    let mut model_url = default_model_url();
    let mut audio_path = None;
    let mut output_path = None;
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
                if audio_path.is_none() {
                    audio_path = Some(PathBuf::from(value));
                } else if output_path.is_none() {
                    output_path = Some(PathBuf::from(value));
                } else {
                    bail!("unexpected positional argument {value:?}\n{}", usage());
                }
                index += 1;
            }
        }
    }

    let audio_path = audio_path.with_context(|| format!("missing audio path\n{}", usage()))?;
    let output_path = output_path.unwrap_or_else(|| default_output_path(&audio_path));

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
        audio_path,
        output_path,
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

    eprintln!(
        "normalizing {} to 16 kHz mono PCM WAV via ffmpeg",
        input_path.display()
    );
    let status = Command::new("ffmpeg")
        .args([
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

    for (index, (start, end)) in ranges.into_iter().enumerate() {
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

    Ok((merged_text, chunks, total_transcribe_seconds))
}

fn main() -> Result<()> {
    let raw_args: Vec<String> = env::args().skip(1).collect();
    if raw_args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", usage());
        return Ok(());
    }

    let args = parse_args(&raw_args)?;
    ensure_model_dir(&args.model_dir, &args.model_package, &args.model_url)?;
    let prepared_audio = normalize_audio(&args.audio_path)?;

    let samples = read_wav_samples(&prepared_audio.wav_path).with_context(|| {
        format!(
            "failed to read WAV samples from {}",
            prepared_audio.wav_path.display()
        )
    })?;
    let audio_seconds = samples.len() as f64 / SAMPLE_RATE as f64;

    let load_start = Instant::now();
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
        model_dir: args.model_dir.display().to_string(),
        audio_path: args.audio_path.display().to_string(),
        prepared_audio_path: prepared_audio.wav_path.display().to_string(),
        used_ffmpeg_normalization: prepared_audio.normalized,
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
    if let Some(parent) = args.output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
    }
    fs::write(&args.output_path, format!("{json}\n"))
        .with_context(|| format!("failed to write {}", args.output_path.display()))?;

    println!("{json}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        build_chunk_ranges, default_model_dir, default_model_package, default_output_path,
        is_supported_audio, merge_chunk_texts, parse_args, remove_appledouble_files, FfprobeStream,
        DEFAULT_MODEL_BASENAME, DEFAULT_MODEL_PACKAGE_NAME,
    };
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
        assert_eq!(parsed.audio_path, PathBuf::from("audio.mp3"));
        assert_eq!(parsed.output_path, PathBuf::from("audio.transcript.json"));
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
        assert_eq!(parsed.output_path, PathBuf::from("out.json"));
        assert_eq!(parsed.model_dir, PathBuf::from("custom-model"));
        assert_eq!(parsed.chunk_seconds, Some(60.0));
        assert_eq!(parsed.chunk_overlap_seconds, 1.5);
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
}
