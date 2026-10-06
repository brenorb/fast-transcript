mod app;
mod audio;
mod cli;
mod delivery;
mod diarization;
mod engine;
mod inference;
mod model;
mod model_catalog;
mod output;
mod progress;
mod remote;
mod settings;
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
    app::run(raw_args)
}

pub fn fuzz_parse_args(raw_args: &[String]) {
    let _ = cli::parse_args(raw_args);
}
