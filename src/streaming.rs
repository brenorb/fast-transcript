//! Incremental media decoding and CLI protocol. All inference and exports use the batch core.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::Instant;

use crate::engine::{load_engine, LoadedEngine};
use crate::inference::InferenceOutcome;
use crate::media_range::ResolvedRange;
use crate::pcm_stream::{Pcm16Decoder, PcmChunker};
use crate::types::{
    BenchmarkChunk, BenchmarkResult, CliArgs, InputSource, PreparedAudio, TranscriptSegment,
};

#[derive(Default, Serialize)]
struct Metrics {
    model_ready_seconds: f64,
    first_result_seconds: Option<f64>,
    wall_seconds: f64,
}

#[derive(Serialize)]
struct Envelope<'a, T: Serialize> {
    version: u32,
    job_id: &'a str,
    sequence: usize,
    #[serde(rename = "type")]
    kind: &'a str,
    data: T,
}

struct Sink<W: Write> {
    writer: W,
    json: bool,
    job_id: String,
    sequence: usize,
    printed: String,
}

impl<W: Write> Sink<W> {
    fn event(&mut self, kind: &str, data: impl Serialize) -> Result<()> {
        if self.json {
            let envelope = Envelope {
                version: 1,
                job_id: &self.job_id,
                sequence: self.sequence,
                kind,
                data,
            };
            serde_json::to_writer(&mut self.writer, &envelope).map_err(|error| {
                if error.io_error_kind() == Some(io::ErrorKind::BrokenPipe) {
                    anyhow::Error::from(io::Error::from(io::ErrorKind::BrokenPipe))
                } else {
                    anyhow::Error::from(error)
                }
            })?;
            self.writer.write_all(b"\n")?;
            self.writer.flush()?;
            self.sequence += 1;
        }
        Ok(())
    }

    fn update(
        &mut self,
        text: &str,
        segments: &[TranscriptSegment],
        chunk: &BenchmarkChunk,
        source_start: f64,
    ) -> Result<()> {
        if self.json {
            #[derive(Serialize)]
            struct Update<'a> {
                provisional: bool,
                text: &'a str,
                segments: Vec<TranscriptSegment>,
                chunk: BenchmarkChunk,
            }
            let mut segments = segments.to_vec();
            for segment in &mut segments {
                segment.start_s += source_start;
                segment.end_s += source_start;
            }
            let mut chunk = chunk.clone();
            chunk.start_s += source_start;
            chunk.end_s += source_start;
            self.event(
                "update",
                Update {
                    provisional: true,
                    text,
                    segments,
                    chunk,
                },
            )
        } else {
            let addition = text
                .strip_prefix(&self.printed)
                .context("stream transcript prefix changed unexpectedly")?;
            self.writer.write_all(addition.as_bytes())?;
            self.writer.flush()?;
            self.printed = text.to_owned();
            Ok(())
        }
    }

    fn complete(
        &mut self,
        args: &CliArgs,
        result: BenchmarkResult,
        source: &InputSource,
        title: Option<&str>,
        metrics: &Metrics,
    ) -> Result<()> {
        let result = if args.clean_output {
            crate::output::cleaned_benchmark_result(&result)
        } else {
            result
        };
        let mut saved = None;
        if args.output_path.is_some() {
            if let crate::delivery::OutputDestination::File(path) =
                crate::app::output_destination_for_args(args, source, title)?
            {
                let text = crate::output::render_output(&result, args.output_format, false)?;
                saved = Some(crate::delivery::write_output_file(&text, &path)?);
            }
        }
        if self.json {
            #[derive(Serialize)]
            struct Completed<'a> {
                result: &'a BenchmarkResult,
                metrics: &'a Metrics,
                output_path: Option<&'a Path>,
            }
            self.event(
                "completed",
                Completed {
                    result: &result,
                    metrics,
                    output_path: saved.as_deref(),
                },
            )?;
        } else {
            self.writer.write_all(b"\n")?;
            self.writer.flush()?;
        }
        if let Some(path) = saved {
            eprintln!("saved {}", path.display());
        }
        Ok(())
    }
}

pub(crate) fn run(args: &CliArgs, source: InputSource) -> Result<()> {
    let started = Instant::now();
    let job_id = std::env::var(crate::stream_supervisor::JOB_ENV)
        .unwrap_or_else(|_| std::process::id().to_string());
    let mut sink = Sink {
        writer: io::stdout().lock(),
        json: matches!(args.output_format, crate::types::OutputFormat::Json),
        job_id,
        sequence: 0,
        printed: String::new(),
    };
    let result = sink
        .event("started", serde_json::json!({"input_source": args.input}))
        .and_then(|()| run_job(args, source, &mut sink, started));
    if let Err(error) = &result {
        if error.chain().any(|cause| {
            cause
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::BrokenPipe)
        }) {
            return Ok(());
        }
        let _ = sink.event(
            "error",
            serde_json::json!({"message": format!("{error:#}")}),
        );
    }
    result
}

fn run_job<W: Write>(
    args: &CliArgs,
    source: InputSource,
    sink: &mut Sink<W>,
    started: Instant,
) -> Result<()> {
    let mut title = None;
    let mut headers = String::new();
    let mut spooled = None;
    let (address, range, network, stdin) = match &source {
        InputSource::LocalPath(path) => {
            let duration = if args.media_range.is_selected() {
                crate::audio::probe_duration(path)?
            } else {
                None
            };
            (
                std::path::absolute(path)?.to_string_lossy().into_owned(),
                args.media_range.resolve(duration)?,
                false,
                false,
            )
        }
        InputSource::Stdin => {
            if io::stdin().is_terminal() {
                bail!("stdin is a terminal; pipe media: cat lecture.wav | fscript - --stream");
            }
            if args.media_range.needs_duration() {
                let media = crate::input::spool_stdin(&mut io::stdin().lock(), false)?;
                let range = args
                    .media_range
                    .resolve(crate::audio::probe_duration(media.path())?)?;
                let address = media.path().to_string_lossy().into_owned();
                spooled = Some(media);
                (address, range, false, false)
            } else {
                (
                    "pipe:0".into(),
                    args.media_range.resolve(None)?,
                    false,
                    true,
                )
            }
        }
        InputSource::RemoteUrl(url) => {
            let (info, range, transcript) = crate::remote::resolve_remote_input(url, args)?;
            title = info.title.or(info.id);
            if let Some(transcript) = transcript {
                let result = crate::app::manual_subtitle_result(url, transcript);
                sink.event("ready", serde_json::json!({"source_start_s": range.start_s, "model": null, "transcript_source": "remote-manual-subtitle"}))?;
                sink.update(
                    &result.text,
                    result.segments.as_deref().unwrap_or(&[]),
                    &result.chunks[0],
                    0.0,
                )?;
                let elapsed = started.elapsed().as_secs_f64();
                let metrics = Metrics {
                    model_ready_seconds: 0.0,
                    first_result_seconds: (!result.text.is_empty()).then_some(elapsed),
                    wall_seconds: elapsed,
                };
                return sink.complete(args, result, &source, title.as_deref(), &metrics);
            }
            if let Some(values) = info.http_headers {
                for (name, value) in values {
                    if name.is_empty()
                        || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
                        || value.contains(['\r', '\n'])
                    {
                        bail!("yt-dlp returned an invalid media HTTP header");
                    }
                    headers.push_str(&format!("{name}: {value}\r\n"));
                }
            }
            let address = info.url.context("yt-dlp did not return a streamable audio URL; update yt-dlp or transcribe without --stream")?;
            if !(address.starts_with("http://") || address.starts_with("https://")) {
                bail!("streaming requires an HTTP(S) media stream from yt-dlp");
            }
            (address, range, true, false)
        }
    };

    eprintln!("starting incremental audio decoding...");
    let mut decoder = Decoder::start(&address, range, network, stdin, &headers)?;
    let mut audio = decoder
        .output
        .take()
        .context("FFmpeg stdout is unavailable")?;
    let LoadedEngine {
        mut engine,
        load_seconds,
        directory,
        metadata,
    } = load_engine(&args.model)?;
    let mut metrics = Metrics {
        model_ready_seconds: started.elapsed().as_secs_f64(),
        ..Default::default()
    };
    sink.event("ready", serde_json::json!({"model": metadata, "source_start_s": range.start_s, "model_ready_seconds": metrics.model_ready_seconds}))?;

    let scratch = tempfile::Builder::new()
        .prefix("fscript-stream-")
        .tempdir()?;
    let wav_path = scratch.path().join("selected.wav");
    let mut recording = if args.diarization.is_some() {
        Some(hound::WavWriter::create(
            &wav_path,
            hound::WavSpec {
                channels: 1,
                sample_rate: crate::SAMPLE_RATE as u32,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?)
    } else {
        None
    };
    let to_samples = |seconds: f64| (seconds * crate::SAMPLE_RATE as f64).round() as usize;
    let mut chunker = PcmChunker::new(
        to_samples(args.chunk_seconds.context("streaming requires chunking")?),
        to_samples(args.chunk_overlap_seconds),
        to_samples(args.stream_opening_seconds),
    )?;
    let mut outcome = InferenceOutcome::default();
    chunker.hold_opening();
    let text_seen = std::cell::Cell::new(false);
    let mut decode = |start: usize, samples: &[f32]| -> Result<()> {
        outcome.decode_chunk(engine.as_mut(), samples, start, true)?;
        text_seen.set(!outcome.text.is_empty());
        if metrics.first_result_seconds.is_none() && !outcome.text.is_empty() {
            metrics.first_result_seconds = Some(started.elapsed().as_secs_f64());
        }
        sink.update(
            &outcome.text,
            &outcome.segments,
            outcome.chunks.last().unwrap(),
            range.start_s,
        )
    };
    let mut bytes = [0u8; 8192];
    let mut pcm = Pcm16Decoder::default();
    loop {
        let length = audio
            .read(&mut bytes)
            .context("failed to read decoded PCM")?;
        if length == 0 {
            break;
        }
        let decoded = pcm.push(&bytes[..length]);
        if let Some(writer) = &mut recording {
            for sample in &decoded {
                writer.write_sample(*sample)?;
            }
        }
        let samples: Vec<_> = decoded
            .into_iter()
            .map(|value| value as f32 / i16::MAX as f32)
            .collect();
        chunker.push(&samples, &mut decode)?;
        if text_seen.get() {
            chunker.use_steady_chunks();
        }
    }
    pcm.finish()?;
    decoder.finish()?;
    chunker.finish(&mut decode)?;
    drop(engine);
    drop(spooled);
    if let Some(writer) = recording {
        writer.finalize()?;
    }
    if chunker.received() == 0 {
        bail!("selected media contains no audio samples");
    }
    let result = crate::transcribe::finalize_transcription(
        args,
        &args.input,
        &PathBuf::from(&args.input),
        "streamed-audio-local-model",
        PreparedAudio {
            wav_path,
            source_start_s: range.start_s,
            normalized: true,
            _tempdir: Some(scratch),
        },
        crate::transcribe::CompletedInference {
            outcome,
            load_seconds,
            directory,
            metadata,
            audio_seconds: chunker.received() as f64 / crate::SAMPLE_RATE as f64,
        },
    )?;
    metrics.wall_seconds = started.elapsed().as_secs_f64();
    sink.complete(args, result, &source, title.as_deref(), &metrics)
}

struct Decoder {
    child: Child,
    output: Option<ChildStdout>,
    diagnostics: tempfile::NamedTempFile,
    finished: bool,
}

impl Decoder {
    fn start(
        address: &str,
        range: ResolvedRange,
        network: bool,
        stdin: bool,
        headers: &str,
    ) -> Result<Self> {
        let diagnostics = tempfile::NamedTempFile::new()?;
        let mut command = Command::new("ffmpeg");
        command.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-xerror"]);
        command.args(["-probesize", "32768", "-analyzeduration", "100000"]);
        if network {
            command.args([
                "-rw_timeout",
                "15000000",
                "-protocol_whitelist",
                "http,https,tcp,tls,crypto",
            ]);
            if !headers.is_empty() {
                command.args(["-headers", headers]);
            }
        }
        if range.start_s > 0.0 && !stdin {
            command.args(["-ss", &range.start_s.to_string()]);
        }
        command.args(["-i", address]);
        if range.start_s > 0.0 && stdin {
            command.args(["-ss", &range.start_s.to_string()]);
        }
        if let Some(end) = range.end_s {
            command.args(["-t", &(end - range.start_s).to_string()]);
        }
        command.args([
            "-map",
            "0:a:0",
            "-vn",
            "-sn",
            "-dn",
            "-ar",
            "16000",
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
            "-f",
            "s16le",
            "pipe:1",
        ]);
        command
            .stdin(if stdin {
                Stdio::inherit()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::from(diagnostics.reopen()?));
        let mut child = command
            .spawn()
            .context("failed to run FFmpeg; install ffmpeg for streaming")?;
        let output = child.stdout.take();
        Ok(Self {
            child,
            output,
            diagnostics,
            finished: false,
        })
    }

    fn finish(&mut self) -> Result<()> {
        let status = self.child.wait().context("failed to wait for FFmpeg")?;
        self.finished = true;
        if !status.success() {
            bail!(
                "FFmpeg streaming decode failed{}",
                crate::diagnostics::diagnostic_tail(Some(&self.diagnostics))
            );
        }
        Ok(())
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
