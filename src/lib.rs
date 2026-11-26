#[macro_use]
mod diagnostics;
mod agents;
mod app;
mod audio;
mod capture;
mod cli;
mod clipboard;
mod control;
mod delivery;
mod diarization;
mod engine;
mod executables;
mod export;
mod inference;
mod input;
mod job;
mod media_range;
mod model;
mod model_catalog;
mod notes;
mod notes_input;
mod output;
mod pcm_stream;
mod progress;
mod recording;
mod remote;
mod settings;
mod shortcut;
mod streaming;
mod transcribe;
mod transcript;
mod types;

use anyhow::Result;

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
    cli::usage()
}

pub fn version_text() -> String {
    cli::version_string()
}

pub fn run_from_args(raw_args: Vec<String>) -> Result<()> {
    if let Some(arguments) = control::arguments(&raw_args) {
        control::run(arguments)
    } else {
        app::run(raw_args)
    }
}

/// Binary entry point with process ownership and cancellation for streaming jobs.
pub fn run_cli_from_args(raw_args: Vec<String>) -> Result<i32> {
    if let Some(arguments) = control::arguments(&raw_args) {
        let supervised = control::needs_supervision(&arguments);
        let finish_on_interrupt = control::finish_on_interrupt(&arguments);
        job::run(arguments, control::run, supervised, finish_on_interrupt)
    } else {
        let stream = cli::parse_command(&raw_args).is_ok_and(|matches| matches.get_flag("stream"));
        job::run(raw_args, app::run, stream, false)
    }
}

pub fn fuzz_parse_args(raw_args: &[String]) {
    if let Some(arguments) = control::arguments(raw_args) {
        let _ = control::validate_arguments(&arguments);
    } else {
        let _ = cli::parse_args(raw_args);
    }
}

pub fn cli_exit(result: Result<i32>) -> std::process::ExitCode {
    use std::io::Write;
    match result {
        Ok(code) => std::process::ExitCode::from(code as u8),
        Err(error) => {
            let _ = writeln!(std::io::stderr().lock(), "error: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
