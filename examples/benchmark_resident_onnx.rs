//! Resident ONNX worker for scripts/benchmark_english_transcription.py.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{env, fs, path::Path, time::Instant};
use transcribe_rs::audio::read_wav_samples;
use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams, TimestampGranularity};
use transcribe_rs::onnx::Quantization;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    ensure!(
        args.len() == 4,
        "usage: worker MANIFEST MODEL_DIR REPETITIONS"
    );
    let manifest_path = Path::new(&args[1]);
    let manifest: Value = serde_json::from_slice(&fs::read(manifest_path)?)?;
    let rows = manifest["utterances"]
        .as_array()
        .context("missing utterances")?;
    let base = manifest_path.parent().context("missing manifest parent")?;
    let repetitions: usize = args[3].parse()?;
    ensure!(repetitions > 0, "repetitions must be positive");
    let start = Instant::now();
    let mut model = ParakeetModel::load(Path::new(&args[2]), &Quantization::Int8)?;
    let load_seconds = start.elapsed().as_secs_f64();
    let params = ParakeetParams {
        timestamp_granularity: Some(TimestampGranularity::Segment),
        ..Default::default()
    };
    let mut runs = Vec::new();
    let mut warmup_seconds = 0.0;
    for repetition in 0..=repetitions {
        let pass_started = Instant::now();
        for row in rows {
            let path = base.join(row["audio_path"].as_str().context("missing path")?);
            let started = Instant::now();
            let samples = read_wav_samples(&path)?;
            let output = model.transcribe_with(&samples, &params)?;
            let seconds = started.elapsed().as_secs_f64();
            if repetition > 0 {
                runs.push(json!({"repetition": repetition, "id": row["id"],
                    "text": output.text.trim(), "transcribe_seconds": seconds}));
            }
        }
        if repetition == 0 {
            warmup_seconds = pass_started.elapsed().as_secs_f64();
        }
        eprintln!("ONNX pass {repetition}/{repetitions} complete");
    }
    println!(
        "{}",
        json!({"engine": "onnx", "device": "cpu", "load_seconds": load_seconds,
        "warmup_seconds": warmup_seconds, "runtime": "transcribe-rs 0.3.8 / int8",
        "model_dir": args[2], "runs": runs})
    );
    Ok(())
}
