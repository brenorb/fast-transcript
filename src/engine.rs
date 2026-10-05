//! Local inference adapters. Audio chunking and transcript merging stay backend-independent.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::env;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Instant;
use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams, TimestampGranularity};
use transcribe_rs::onnx::Quantization;
use transcribe_rs::{TranscriptionResult, TranscriptionSegment};

use crate::model::{ensure_model_dir, ModelConfig};
use crate::model_catalog::ModelRuntime;
use crate::types::ModelMetadata;

pub(crate) trait SpeechEngine {
    fn transcribe(&mut self, samples: &[f32]) -> Result<TranscriptionResult>;
}

pub(crate) struct LoadedEngine {
    pub(crate) engine: Box<dyn SpeechEngine>,
    pub(crate) load_seconds: f64,
    pub(crate) directory: String,
    pub(crate) metadata: ModelMetadata,
}

pub(crate) fn load_engine(config: &ModelConfig) -> Result<LoadedEngine> {
    let preset = config.preset;
    let runtime = preset.map_or(ModelRuntime::Onnx, |model| model.runtime);
    let (engine, load_seconds, directory, device): (Box<dyn SpeechEngine>, _, _, _) = match runtime
    {
        ModelRuntime::Onnx => {
            ensure_model_dir(&config.directory, &config.package, &config.url)?;
            let started = Instant::now();
            let model = ParakeetModel::load(&config.directory, &Quantization::Int8)
                .context("failed to load Parakeet ONNX model")?;
            (
                Box::new(NativeEngine { model }),
                started.elapsed().as_secs_f64(),
                config.directory.display().to_string(),
                "cpu".to_string(),
            )
        }
        ModelRuntime::Photon | ModelRuntime::Phonon => {
            let (engine, info) = PythonEngine::load(config)?;
            (
                Box::new(engine),
                info.load_seconds,
                info.model_dir,
                info.device,
            )
        }
    };
    let runtime_name = match runtime {
        ModelRuntime::Onnx => "onnx",
        ModelRuntime::Photon => "photon",
        ModelRuntime::Phonon => "phonon",
    };
    Ok(LoadedEngine {
        engine,
        load_seconds,
        directory,
        metadata: ModelMetadata {
            name: preset
                .map_or_else(
                    || {
                        if config.url == crate::DEFAULT_MODEL_URL {
                            "parakeet-v3-int8"
                        } else {
                            "custom-onnx"
                        }
                    },
                    |model| model.name,
                )
                .to_string(),
            source_url: config.url.clone(),
            runtime: runtime_name.to_string(),
            device,
            revision: preset
                .filter(|_| config.directory.as_os_str().is_empty())
                .filter(|model| model.runtime == ModelRuntime::Photon)
                .and_then(|model| model.revision)
                .map(str::to_string),
            artifact_sha256: preset
                .filter(|_| config.directory.as_os_str().is_empty())
                .filter(|model| model.runtime == ModelRuntime::Phonon)
                .and_then(|model| model.revision)
                .map(str::to_string),
        },
    })
}

struct NativeEngine {
    model: ParakeetModel,
}

impl SpeechEngine for NativeEngine {
    fn transcribe(&mut self, samples: &[f32]) -> Result<TranscriptionResult> {
        self.model
            .transcribe_with(
                samples,
                &ParakeetParams {
                    timestamp_granularity: Some(TimestampGranularity::Segment),
                    ..Default::default()
                },
            )
            .context("Parakeet ONNX transcription failed")
    }
}

#[derive(Deserialize)]
struct WorkerInfo {
    ready: bool,
    device: String,
    model_dir: String,
    load_seconds: f64,
}

#[derive(Deserialize)]
struct WorkerSegment {
    start: f32,
    end: f32,
    text: String,
}

#[derive(Deserialize)]
struct WorkerTranscript {
    text: String,
    #[serde(default)]
    segments: Vec<WorkerSegment>,
}

struct PythonEngine {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl PythonEngine {
    fn load(config: &ModelConfig) -> Result<(Self, WorkerInfo)> {
        let preset = config.preset.context("missing SDK model preset")?;
        let mut command = if let Some(python) = env::var_os("FSCRIPT_PYTHON_BINARY") {
            Command::new(python)
        } else {
            let mut command = Command::new("uv");
            command.args(["run", "--no-project", "--python", "3.12"]);
            let dependencies: &[&str] = match preset.runtime {
                ModelRuntime::Photon => &["moondream==2.4.1", "kestrel==0.8.1", "torch==2.14.0"],
                ModelRuntime::Phonon => &[
                    "fermion-research==0.2.3",
                    "torch==2.14.0",
                    "transformers==5.17.0",
                    "soundfile==0.14.0",
                    "scipy==1.18.1",
                    "zstandard==0.25.0",
                ],
                ModelRuntime::Onnx => unreachable!(),
            };
            for dependency in dependencies {
                command.args(["--with", dependency]);
            }
            if preset.runtime == ModelRuntime::Phonon
                && cfg!(all(target_os = "macos", target_arch = "aarch64"))
            {
                for dependency in ["mlx==0.32.3", "mlx-audio==0.5.7", "mlx-lm==0.31.3"] {
                    command.args(["--with", dependency]);
                }
            }
            command.args(["--", "python"]);
            command
        };
        command.args([
            "-u",
            "-c",
            include_str!("python_worker.py"),
            preset.repository,
            preset.revision.unwrap_or_default(),
            &config.device,
        ]);
        command.arg(&config.directory);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let mut child = command.spawn().context(
            "failed to start model runtime; install uv (https://docs.astral.sh/uv/) or set FSCRIPT_PYTHON_BINARY to an interpreter with the model SDK installed"
        )?;
        let input = child
            .stdin
            .take()
            .context("model worker stdin is unavailable")?;
        let output = child
            .stdout
            .take()
            .context("model worker stdout is unavailable")?;
        let mut engine = Self {
            child,
            input: Some(input),
            output: BufReader::new(output),
        };
        let info: WorkerInfo = serde_json::from_value(engine.read_response()?)
            .context("invalid model worker startup response")?;
        if !info.ready || !info.load_seconds.is_finite() || info.load_seconds < 0.0 {
            bail!("model worker did not report a valid loaded model");
        }
        Ok((engine, info))
    }

    fn read_response(&mut self) -> Result<serde_json::Value> {
        let mut line = String::new();
        if self
            .output
            .read_line(&mut line)
            .context("failed to read model worker response")?
            == 0
        {
            let status = self
                .child
                .try_wait()
                .context("failed to inspect model worker")?;
            bail!(
                "model worker stopped before responding ({status:?}); see the runtime error above"
            );
        }
        let response: serde_json::Value =
            serde_json::from_str(&line).context("model worker returned invalid JSON")?;
        if let Some(error) = response.get("error").and_then(|value| value.as_str()) {
            bail!("model runtime failed: {error}");
        }
        Ok(response)
    }
}

impl SpeechEngine for PythonEngine {
    fn transcribe(&mut self, samples: &[f32]) -> Result<TranscriptionResult> {
        let audio = worker_audio(samples)?;
        let input = self
            .input
            .as_mut()
            .context("model worker stdin was closed")?;
        serde_json::to_writer(
            &mut *input,
            &serde_json::json!({ "audio_path": audio.path() }),
        )?;
        input.write_all(b"\n")?;
        input.flush()?;
        let result: WorkerTranscript = serde_json::from_value(self.read_response()?)
            .context("invalid model worker transcript response")?;
        Ok(TranscriptionResult {
            text: result.text,
            segments: Some(
                result
                    .segments
                    .into_iter()
                    .map(|segment| TranscriptionSegment {
                        start: segment.start,
                        end: segment.end,
                        text: segment.text,
                    })
                    .collect(),
            ),
        })
    }
}

fn worker_audio(samples: &[f32]) -> Result<tempfile::NamedTempFile> {
    // Recreate normalized PCM16 exactly so every engine receives the same audio.
    let audio = tempfile::Builder::new()
        .suffix(".wav")
        .tempfile()
        .context("failed to create model worker audio file")?;
    let mut writer = hound::WavWriter::new(
        audio.reopen()?,
        hound::WavSpec {
            channels: 1,
            sample_rate: crate::SAMPLE_RATE as u32,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for sample in samples {
        writer.write_sample((sample * i16::MAX as f32).round() as i16)?;
    }
    writer.finalize()?;
    Ok(audio)
}

impl Drop for PythonEngine {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn worker_audio_preserves_pcm16_including_negative_full_scale() {
        let pcm = [i16::MIN, -32767, -1, 0, 1, 32766, i16::MAX];
        let samples = pcm
            .iter()
            .map(|sample| *sample as f32 / i16::MAX as f32)
            .collect::<Vec<_>>();
        let audio = super::worker_audio(&samples).unwrap();
        let mut reader = hound::WavReader::open(audio.path()).unwrap();
        assert_eq!(reader.spec().sample_rate, crate::SAMPLE_RATE as u32);
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(
            reader
                .samples::<i16>()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            pcm
        );
    }
}
