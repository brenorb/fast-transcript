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
                .map_err(crate::errors::technical)
                .with_context(|| {
                    let folder = config.directory.display();
                    if crate::model::is_managed_model_dir(&config.directory, &config.url) {
                        format!("could not load the speech model from {folder}; delete that folder and run fscript again to reinstall it")
                    } else {
                        format!("could not load the speech model from {folder}; check that --model-dir points to a compatible model folder")
                    }
                })?;
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
            .map_err(crate::errors::technical)
            .context("the speech model could not transcribe this audio")
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
    name: &'static str,
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    diagnostics: Option<tempfile::NamedTempFile>,
}

impl PythonEngine {
    fn load(config: &ModelConfig) -> Result<(Self, WorkerInfo)> {
        let preset = config
            .preset
            .context("this model can only be selected by name; run fscript --list-models")?;
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
        let diagnostics = crate::diagnostics::capture_stderr(&mut command)?;
        let name = preset.name;
        let mut child = command.spawn().map_err(|error| {
            if let Some(python) = env::var_os("FSCRIPT_PYTHON_BINARY") {
                anyhow::Error::new(error).context(format!(
                    "could not start the Python interpreter in FSCRIPT_PYTHON_BINARY ({}); point it to a working interpreter, or unset it so fscript can set up the model itself",
                    std::path::Path::new(&python).display()
                ))
            } else if error.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!("the {name} model needs uv to set itself up, but uv is not installed. Install uv (https://docs.astral.sh/uv/), or use the default model with --model default")
            } else {
                anyhow::Error::new(error).context(format!("could not start the {name} model"))
            }
        })?;
        let unexpected = || unexpected_reply(name);
        let input = child.stdin.take().with_context(unexpected)?;
        let output = child.stdout.take().with_context(unexpected)?;
        let mut engine = Self {
            name,
            child,
            input: Some(input),
            output: BufReader::new(output),
            diagnostics,
        };
        let info: WorkerInfo = serde_json::from_value(engine.read_response()?)
            .map_err(crate::errors::technical)
            .with_context(|| engine.failure_context(&unexpected_reply(name)))?;
        if !info.ready || !info.load_seconds.is_finite() || info.load_seconds < 0.0 {
            bail!("{}", engine.failure_context(&unexpected_reply(name)));
        }
        Ok((engine, info))
    }

    fn failure_context(&self, message: &str) -> String {
        format!(
            "{message}{}",
            crate::diagnostics::diagnostic_tail(self.diagnostics.as_ref())
        )
    }

    fn stopped_message(&self) -> String {
        let see_above = if crate::diagnostics::quiet() {
            ""
        } else {
            " (see the messages above)"
        };
        self.failure_context(&format!(
            "the {} model stopped unexpectedly{see_above}; try again, or use the default model with --model default",
            self.name
        ))
    }

    fn read_response(&mut self) -> Result<serde_json::Value> {
        let mut line = String::new();
        let read = self
            .output
            .read_line(&mut line)
            .map_err(crate::errors::technical)
            .with_context(|| self.stopped_message())?;
        if read == 0 {
            let _ = self.child.try_wait();
            bail!("{}", self.stopped_message());
        }
        let response: serde_json::Value = serde_json::from_str(&line)
            .map_err(crate::errors::technical)
            .with_context(|| self.failure_context(&unexpected_reply(self.name)))?;
        if let Some(error) = response.get("error").and_then(|value| value.as_str()) {
            bail!(
                "the {} model reported an error: {error}{}",
                self.name,
                crate::diagnostics::diagnostic_tail(self.diagnostics.as_ref())
            );
        }
        Ok(response)
    }
}

impl SpeechEngine for PythonEngine {
    fn transcribe(&mut self, samples: &[f32]) -> Result<TranscriptionResult> {
        let audio = worker_audio(samples)?;
        let stopped = self.stopped_message();
        let input = self.input.as_mut().context(stopped.clone())?;
        let request = serde_json::to_vec(&serde_json::json!({ "audio_path": audio.path() }))?;
        input
            .write_all(&request)
            .and_then(|()| input.write_all(b"\n"))
            .and_then(|()| input.flush())
            .map_err(crate::errors::technical)
            .context(stopped)?;
        let result: WorkerTranscript = serde_json::from_value(self.read_response()?)
            .map_err(crate::errors::technical)
            .with_context(|| self.failure_context(&unexpected_reply(self.name)))?;
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

fn unexpected_reply(name: &str) -> String {
    format!("the {name} model sent an unexpected reply; update fscript, or use the default model with --model default")
}

/// Keep disk errors (e.g. a full disk) readable; other WAV errors are internal.
fn wav_error(error: hound::Error) -> Box<dyn std::error::Error + Send + Sync> {
    match error {
        hound::Error::IoError(error) => Box::new(error),
        other => Box::new(crate::errors::technical(other)),
    }
}

fn worker_audio(samples: &[f32]) -> Result<tempfile::NamedTempFile> {
    // Recreate normalized PCM16 exactly so every engine receives the same audio.
    let audio = crate::errors::temp(tempfile::Builder::new().suffix(".wav").tempfile())?;
    let write = || -> std::result::Result<(), hound::Error> {
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
        writer.finalize()
    };
    crate::errors::temp(write().map_err(wav_error))?;
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
