mod audio;
mod cli;
mod diarization;
mod model;
mod output;
mod progress;
mod remote;
mod transcribe;
mod types;

use anyhow::{Context, Result};
use cli::{parse_args, usage, version_string};
use output::{
    default_output_path_for_input, emit_file_output_completion, render_output,
    resolve_output_target_path, write_output_file,
};
use remote::{
    download_manual_remote_transcript, download_remote_audio, infer_input_source,
    load_remote_video_info, DirectTranscript,
};
use std::env;
use std::io;
use std::path::PathBuf;
use transcribe::transcribe_audio_input;
use types::{BenchmarkChunk, BenchmarkResult, CliArgs, InputSource};

pub(crate) const SAMPLE_RATE: usize = 16_000;
pub(crate) const DEFAULT_DATA_DIR_FALLBACK: &str = ".fast-transcript";
pub(crate) const DEFAULT_CACHE_DIR_FALLBACK: &str = ".fast-transcript-cache";
pub(crate) const DEFAULT_MODEL_SUBDIR: &str = "models";
pub(crate) const DEFAULT_MODEL_PACKAGE_NAME: &str = "parakeet-v3-int8.tar.gz";
pub(crate) const DEFAULT_MODEL_URL: &str = "https://huggingface.co/brenorb/parakeet-tdt-0.6b-v3-int8-onnx-bundle/resolve/main/parakeet-v3-int8.tar.gz?download=1";
pub(crate) const DEFAULT_MODEL_BASENAME: &str = "parakeet-tdt-0.6b-v3-int8";
pub(crate) const DEFAULT_CHUNK_SECONDS: f64 = 120.0;
pub(crate) const DEFAULT_CHUNK_OVERLAP_SECONDS: f64 = 2.0;
pub(crate) const PROGRESS_BAR_WIDTH: usize = 20;
pub(crate) const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
pub(crate) const REQUIRED_MODEL_FILES: [&str; 4] = [
    "encoder-model.int8.onnx",
    "decoder_joint-model.int8.onnx",
    "nemo128.onnx",
    "vocab.txt",
];

pub fn usage_text() -> String {
    usage()
}

pub fn version_text() -> String {
    version_string()
}

pub fn run_from_args(raw_args: Vec<String>) -> Result<()> {
    if raw_args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", usage());
        return Ok(());
    }
    if raw_args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{}", version_string());
        return Ok(());
    }

    let args = parse_args(&raw_args)?;
    if let Some(notice) = &args.diarization_notice {
        eprintln!("{notice}");
    }
    let input_source = infer_input_source(&args.input);
    let default_output_path = default_output_path_for_args(&args, &input_source, None);

    match &input_source {
        InputSource::RemoteUrl(url) => handle_remote_input(&args, &input_source, url),
        InputSource::LocalPath(path) => {
            let result =
                transcribe_audio_input(&args, &args.input, path, "local-audio-local-model")?;
            write_or_print_result(&args, &result, default_output_path)
        }
    }
}

pub fn fuzz_parse_args(raw_args: &[String]) {
    let _ = parse_args(raw_args);
}

fn handle_remote_input(args: &CliArgs, input_source: &InputSource, url: &str) -> Result<()> {
    let info = load_remote_video_info(url)?;
    let resolved_output_path = default_output_path_for_args(
        args,
        input_source,
        info.title.as_deref().or(info.id.as_deref()),
    );

    if !args.force_local_for_remote && args.diarization.is_none() {
        match download_manual_remote_transcript(url, &info) {
            Ok(Some(transcript)) => {
                let result = manual_subtitle_result(url, transcript);
                return write_or_print_result_with_status(
                    args,
                    &result,
                    resolved_output_path,
                    "done: used manual subtitles via yt-dlp",
                );
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!(
                    "warning: manual subtitle download failed; falling back to remote audio download: {error}"
                );
            }
        }
    }

    let downloaded_audio = download_remote_audio(url)?;
    let result = transcribe_audio_input(
        args,
        url,
        &downloaded_audio.audio_path,
        "downloaded-audio-local-model",
    )?;
    write_or_print_result(args, &result, resolved_output_path)
}

fn default_output_path_for_args(
    args: &CliArgs,
    input_source: &InputSource,
    title_hint: Option<&str>,
) -> Option<PathBuf> {
    if args.output_to_stdout {
        None
    } else {
        let default_path =
            default_output_path_for_input(input_source, title_hint, args.output_format);
        Some(match args.output_path.clone() {
            Some(path) => resolve_output_target_path(
                &path,
                &env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                &default_path,
            ),
            None => default_path,
        })
    }
}

fn manual_subtitle_result(input_source: &str, transcript: DirectTranscript) -> BenchmarkResult {
    BenchmarkResult {
        input_source: input_source.to_string(),
        model_dir: String::new(),
        audio_path: input_source.to_string(),
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
        segments: (!transcript.segments.is_empty()).then_some(transcript.segments),
        speaker_diarization: None,
    }
}

fn write_or_print_result(
    args: &CliArgs,
    result: &BenchmarkResult,
    output_path: Option<PathBuf>,
) -> Result<()> {
    write_or_print_result_with_status(
        args,
        result,
        output_path,
        &format!("done in {:.2}x real-time", result.realtime_speedup),
    )
}

fn write_or_print_result_with_status(
    args: &CliArgs,
    result: &BenchmarkResult,
    output_path: Option<PathBuf>,
    status_message: &str,
) -> Result<()> {
    let output = render_output(result, args.output_format, args.clean_output)?;
    if args.output_to_stdout {
        println!("{output}");
        return Ok(());
    }

    let output_path = output_path
        .as_ref()
        .context("missing output path for file output mode")?;
    let current_dir = env::current_dir().context("failed to resolve current working directory")?;
    let resolved_output_target = resolve_output_target_path(output_path, &current_dir, output_path);
    let absolute_output_path = write_output_file(&output, &resolved_output_target, &current_dir)?;
    emit_file_output_completion(
        &mut io::stdout(),
        &mut io::stderr(),
        &absolute_output_path,
        status_message,
    )?;
    Ok(())
}
