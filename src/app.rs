//! Application routing for local media, remote subtitles, and rendered output.

use crate::cli::{parse_args, usage, version_string};
use crate::delivery::{default_output_path_for_input, OutputDestination};
use crate::output::render_output;
use crate::remote::{
    download_manual_remote_transcript, download_remote_audio, infer_input_source,
    load_remote_video_info, DirectTranscript,
};
use crate::transcribe::transcribe_audio_input;
use crate::types::{BenchmarkChunk, BenchmarkResult, CliArgs, InputSource};
use anyhow::{Context, Result};
use std::env;
use std::io;

pub(crate) fn run(raw_args: Vec<String>) -> Result<()> {
    if raw_args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", usage());
        return Ok(());
    }
    if raw_args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{}", version_string());
        return Ok(());
    }
    if raw_args.iter().any(|arg| arg == "--list-models") {
        println!("{}", crate::model_catalog::list_models());
        return Ok(());
    }

    let args = parse_args(&raw_args)?;
    if let Some(notice) = &args.diarization_notice {
        eprintln!("{notice}");
    }
    let input_source = infer_input_source(&args.input);

    match &input_source {
        InputSource::RemoteUrl(url) => handle_remote_input(&args, &input_source, url),
        InputSource::LocalPath(path) => {
            let destination = output_destination_for_args(&args, &input_source, None)?;
            let result =
                transcribe_audio_input(&args, &args.input, path, "local-audio-local-model")?;
            emit_result(&args, &result, destination)
        }
    }
}

fn handle_remote_input(args: &CliArgs, input_source: &InputSource, url: &str) -> Result<()> {
    let info = load_remote_video_info(url)?;
    let destination = output_destination_for_args(
        args,
        input_source,
        info.title.as_deref().or(info.id.as_deref()),
    )?;

    if !args.force_local_for_remote && args.diarization.is_none() {
        match download_manual_remote_transcript(url, &info) {
            Ok(Some(transcript)) => {
                let result = manual_subtitle_result(url, transcript);
                return emit_result_with_status(
                    args,
                    &result,
                    destination,
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
    emit_result(args, &result, destination)
}

fn output_destination_for_args(
    args: &CliArgs,
    input_source: &InputSource,
    title_hint: Option<&str>,
) -> Result<OutputDestination> {
    if args.output_to_stdout {
        return Ok(OutputDestination::Stdout);
    }
    let current_dir = env::current_dir().context("failed to resolve current working directory")?;
    let default_path = default_output_path_for_input(input_source, title_hint, args.output_format);
    Ok(OutputDestination::file(
        args.output_path.as_deref(),
        &default_path,
        &current_dir,
    ))
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
        model: None,
    }
}

fn emit_result(
    args: &CliArgs,
    result: &BenchmarkResult,
    destination: OutputDestination,
) -> Result<()> {
    emit_result_with_status(
        args,
        result,
        destination,
        &format!("done in {:.2}x real-time", result.realtime_speedup),
    )
}

fn emit_result_with_status(
    args: &CliArgs,
    result: &BenchmarkResult,
    destination: OutputDestination,
    status_message: &str,
) -> Result<()> {
    let output = render_output(result, args.output_format, args.clean_output)?;
    destination.write(
        &output,
        status_message,
        &mut io::stdout(),
        &mut io::stderr(),
    )
}
