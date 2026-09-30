//! ONNX worker for the canonical cold/warm ASR benchmark protocol.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{env, fs, io::Write, path::Path, time::Instant};
use transcribe_rs::audio::read_wav_samples;
use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams, TimestampGranularity};
use transcribe_rs::onnx::Quantization;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    ensure!(args.len() == 3, "usage: worker MANIFEST MODEL_DIR");
    let path = Path::new(&args[1]);
    let manifest: Value = serde_json::from_slice(&fs::read(path)?)?;
    let base = path.parent().context("manifest parent missing")?;
    let utterances = manifest["utterances"]
        .as_array()
        .context("utterances missing")?;
    let mut model = ParakeetModel::load(Path::new(&args[2]), &Quantization::Int8)?;
    let params = ParakeetParams {
        timestamp_granularity: Some(TimestampGranularity::Segment),
        ..Default::default()
    };
    for phase in ["cold", "warm"] {
        let started = Instant::now();
        let mut results = Vec::new();
        for utterance in utterances {
            let mut parts = Vec::new();
            for part in utterance["parts"].as_array().context("parts missing")? {
                let audio_path = base.join(part["path"].as_str().context("audio path missing")?);
                let started = Instant::now();
                let samples = read_wav_samples(&audio_path)?;
                let output = model.transcribe_with(&samples, &params)?;
                parts.push(json!({"text":output.text.trim(), "call_seconds":started.elapsed().as_secs_f64(), "audio_seconds":samples.len() as f64/16000.0}));
            }
            let text = parts
                .iter()
                .map(|p| p["text"].as_str().unwrap_or(""))
                .collect::<Vec<_>>()
                .join(" ");
            results.push(json!({"id":utterance["id"], "text":text, "parts":parts}));
        }
        let seconds = started.elapsed().as_secs_f64();
        println!(
            "{}",
            json!({"phase":phase, "corpus_seconds":seconds, "utterances":results, "runtime":"transcribe-rs 0.3.8 int8"})
        );
        std::io::stdout().flush()?;
    }
    Ok(())
}
