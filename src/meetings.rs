//! Foreground meeting service. The desktop uses this exact CLI protocol.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::io::{self, BufRead, BufReader, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::engine::LoadedEngine;
use crate::inference::InferenceOutcome;
use crate::meeting_capture::{Capture, Stop};
use crate::meeting_store::Track;
use crate::pcm_stream::PcmChunker;
use crate::session_events::Events;
use crate::types::{BenchmarkResult, CliArgs, PreparedAudio};

const FINAL_CHUNK_SECONDS: f64 = 30.0;
const FINAL_OVERLAP_SECONDS: f64 = 2.0;

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn signal(_: std::ffi::c_int) {
    STOP.store(true, Ordering::Relaxed);
}
fn signalled() -> bool {
    STOP.load(Ordering::Relaxed)
}

pub(crate) struct Options {
    pub(crate) args: CliArgs,
    pub(crate) control: bool,
    pub(crate) json: bool,
    pub(crate) duration: Option<Duration>,
    pub(crate) output_directory: PathBuf,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Start,
    Stop,
    Cancel,
    Shutdown,
}

#[derive(Default)]
struct Controller {
    stop: Mutex<Option<Arc<Stop>>>,
    requested: AtomicBool,
    cancelled: AtomicBool,
}

impl Controller {
    fn finish(&self, cancel: bool) {
        let current = self.stop.lock().unwrap();
        if cancel {
            self.cancelled.store(true, Ordering::SeqCst);
        }
        self.requested.store(true, Ordering::SeqCst);
        if let Some(stop) = current.as_ref() {
            stop.request();
        }
    }

    fn attach(&self, stop: Arc<Stop>) {
        let mut current = self.stop.lock().unwrap();
        if self.requested.load(Ordering::SeqCst) {
            stop.request();
        }
        *current = Some(stop);
    }
}

fn input(controller: Arc<Controller>) -> mpsc::Receiver<Result<Request>> {
    let (send, receive) = mpsc::sync_channel(16);
    std::thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        loop {
            let mut line = String::new();
            let request = match (&mut stdin).take(65537).read_line(&mut line) {
                Ok(0) if controller.requested.load(Ordering::SeqCst) => break,
                Ok(0) => Err(anyhow::anyhow!(
                    "meeting controller closed before completion"
                )),
                Ok(_) if line.len() > 65536 || !line.ends_with('\n') => Err(anyhow::anyhow!(
                    "control requests must be newline-terminated JSON of at most 64 KiB"
                )),
                Ok(_) => serde_json::from_str::<Request>(&line)
                    .context("invalid meeting control request"),
                Err(error) => Err(error.into()),
            };
            match &request {
                Ok(Request::Stop) => controller.finish(false),
                Ok(Request::Cancel | Request::Shutdown) | Err(_) => controller.finish(true),
                _ => {}
            }
            let terminal = request.is_err();
            if send.send(request).is_err() || terminal {
                break;
            }
        }
    });
    receive
}

pub(crate) fn run(options: Options) -> Result<()> {
    if !options.control && options.duration.is_none() && !io::stdin().is_terminal() {
        bail!(
            "meeting needs a terminal, --duration SECONDS, or --control for unattended start/stop"
        );
    }
    crate::session_events::install_stop_handler(signal)?;
    let mut events = Events {
        json: options.json || options.control,
        sequence: 0,
    };
    let result = run_session(&options, &mut events);
    if let Err(error) = &result {
        let _ = events.send(
            "error",
            None,
            serde_json::json!({"message":format!("{error:#}"),"fatal":true}),
        );
    }
    result
}

fn run_session(options: &Options, events: &mut Events) -> Result<()> {
    let controller = Arc::new(Controller::default());
    let input = options.control.then(|| input(Arc::clone(&controller)));
    events.send(
        "ready",
        None,
        serde_json::json!({"source":"meeting","requires_start":options.control}),
    )?;
    if let Some(input) = &input {
        match input.recv().context("meeting controller disconnected")?? {
            Request::Start => {}
            _ => bail!("meeting control requires start before stop or cancel"),
        }
    }
    std::fs::create_dir_all(&options.output_directory)
        .context("cannot create meetings output directory")?;
    let id = format!(
        "{}-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
        std::process::id()
    );
    let directory = options.output_directory.join(&id);
    let capture = Capture::start(&directory, options.duration, signalled)?;
    controller.attach(Arc::clone(&capture.stop));
    events.send(
        "started",
        Some(&id),
        serde_json::json!({"directory":directory,"tracks":["Microphone","System"]}),
    )?;
    eprintln!(
        "saving meeting to {}; {}",
        directory.display(),
        if options.control {
            "send stop to finish; Ctrl-C cancels"
        } else {
            "Ctrl-C finishes; a second interrupt cancels"
        }
    );

    let mut capture = Some(capture);
    let mut recognition_error = None;
    let mut worker = match Worker::start(&options.args, &directory) {
        Ok(worker) => Some(worker),
        Err(error) => {
            recognition_error = Some(format!("cannot start recognition: {error:#}"));
            events.send(
                "recognition_error",
                Some(&id),
                serde_json::json!({"message":recognition_error,"recording_continues":true}),
            )?;
            None
        }
    };
    let mut announced = false;
    let mut saved = false;
    let mut completed = false;
    loop {
        if !announced && capture.as_ref().is_some_and(Capture::ready) {
            events.send(
                "recording",
                Some(&id),
                serde_json::json!({"directory":directory}),
            )?;
            announced = true;
        }
        if let Some(input) = &input {
            while let Ok(request) = input.try_recv() {
                match request? {
                    Request::Start => bail!(
                        "a meeting is already active; start a new process for the next meeting"
                    ),
                    Request::Stop | Request::Cancel | Request::Shutdown => {}
                }
            }
        }
        if controller.cancelled.load(Ordering::SeqCst) {
            drop(worker);
            drop(capture);
            let _ = std::fs::remove_file(directory.join(".transcript.pending.json"));
            events.send("cancelled", Some(&id), serde_json::json!({"directory":directory,"recoverable":!saved,"audio_stopped":saved}))?;
            return Ok(());
        }
        let mut worker_ended = false;
        if let Some(current) = worker.as_mut() {
            while let Ok(event) = current.events.try_recv() {
                current.pending.fetch_sub(1, Ordering::SeqCst);
                let (mut kind, mut data) = event?;
                if kind == "recognition_error" {
                    data["recording_continues"] = serde_json::json!(!saved);
                    recognition_error = data
                        .get("message")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned);
                    eprintln!(
                        "recognition failed; continuing to save meeting audio: {}",
                        recognition_error.as_deref().unwrap_or("unknown error")
                    );
                }
                if kind == "transcribed" {
                    // Cancellation and final publication share one decision lock.
                    // Once promoted, completion wins any later cancel request.
                    let _decision = controller.stop.lock().unwrap();
                    if controller.cancelled.load(Ordering::SeqCst) {
                        break;
                    }
                    if !saved {
                        bail!("recognition finished before meeting audio was saved");
                    }
                    std::fs::rename(
                        directory.join(".transcript.pending.json"),
                        directory.join("transcript.json"),
                    )?;
                    std::fs::File::open(&directory)?.sync_all()?;
                    kind = "completed".into();
                    completed = true;
                }
                events.send(&kind, Some(&id), data.clone())?;
                if !events.json && kind == "update" {
                    if let Some(text) = data.get("addition").and_then(serde_json::Value::as_str) {
                        eprint!("{text}");
                    }
                }
                if completed {
                    break;
                }
            }
            if completed {
                break;
            }
            if let Some(status) = current.child.try_wait()? {
                if current.events_done.load(Ordering::Acquire)
                    && current.pending.load(Ordering::SeqCst) == 0
                {
                    worker_ended = true;
                    if recognition_error.is_none() {
                        let message = format!(
                            "recognition worker exited without completion ({status}){}",
                            crate::diagnostics::diagnostic_tail(Some(&current.diagnostics))
                        );
                        recognition_error = Some(message.clone());
                        events.send(
                            "recognition_error",
                            Some(&id),
                            serde_json::json!({"message":message,"recording_continues":!saved}),
                        )?;
                    }
                }
            }
        }
        if worker_ended {
            worker.take();
        }
        if capture.as_ref().is_some_and(Capture::finished) {
            let manifest = capture.take().unwrap().finish()?;
            saved = true;
            events.send(
                "saved",
                Some(&id),
                serde_json::json!({"directory":directory,"manifest":manifest}),
            )?;
            if recognition_error.is_none() {
                if let Some(current) = worker.as_mut() {
                    if let Err(error) = current.finish_capture(manifest.track_frames) {
                        recognition_error = Some(format!("cannot finish recognition: {error:#}"));
                        events.send("recognition_error", Some(&id), serde_json::json!({"message":recognition_error,"recording_continues":false}))?;
                        worker.take();
                    }
                }
            }
        }
        if saved && worker.is_none() {
            bail!(
                "meeting audio saved to {}; recognition failed: {}",
                directory.display(),
                recognition_error
                    .as_deref()
                    .unwrap_or("recognition unavailable")
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !events.json {
        crate::delivery::write_stdout(
            &mut io::stdout().lock(),
            format_args!("{}\n", directory.join("transcript.json").display()),
        )?;
    }
    Ok(())
}

// The inference service is a child in the existing supervised job group. Keeping
// it out of the controller lets cancel/EOF stop even a blocked native inference.
// This environment entry point is internal; it is not another public command.
pub(crate) const WORKER_ENV: &str = "FSCRIPT_INTERNAL_MEETING_TRANSCRIBE";

struct Worker {
    child: Child,
    input: Option<ChildStdin>,
    events: mpsc::Receiver<Result<(String, serde_json::Value)>>,
    events_done: Arc<AtomicBool>,
    pending: Arc<std::sync::atomic::AtomicUsize>,
    diagnostics: tempfile::NamedTempFile,
}

impl Worker {
    fn start(args: &CliArgs, directory: &Path) -> Result<Self> {
        let mut command = Command::new(std::env::current_exe()?);
        command.arg("meeting-audio").args([
            "--stream",
            "--no-diarization",
            "--device",
            &args.model.device,
        ]);
        if let Some(preset) = args.model.preset {
            command.args(["--model", preset.name]);
        } else {
            command.args(["--model-url", &args.model.url]);
        }
        if !args.model.directory.as_os_str().is_empty() {
            command.arg("--model-dir").arg(&args.model.directory);
        }
        if args.model.preset.is_none() {
            command.arg("--model-package").arg(&args.model.package);
        }
        let diagnostics = tempfile::NamedTempFile::new()?;
        command
            .env(WORKER_ENV, directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(diagnostics.reopen()?));
        let mut child = command
            .spawn()
            .context("cannot start meeting recognition worker")?;
        let input = child.stdin.take();
        let output = child
            .stdout
            .take()
            .context("missing meeting recognition stdout")?;
        let (send, events) = mpsc::sync_channel(16);
        let events_done = Arc::new(AtomicBool::new(false));
        let done = Arc::clone(&events_done);
        let pending = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let queued = Arc::clone(&pending);
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let event = (|| {
                    let value: serde_json::Value = serde_json::from_str(&line?)?;
                    let kind = value
                        .get("type")
                        .and_then(serde_json::Value::as_str)
                        .context("invalid recognition event type")?
                        .to_owned();
                    let data = value
                        .get("data")
                        .context("missing recognition event data")?
                        .clone();
                    Ok((kind, data))
                })();
                queued.fetch_add(1, Ordering::SeqCst);
                if send.send(event).is_err() {
                    break;
                }
            }
            done.store(true, Ordering::Release);
        });
        Ok(Self {
            child,
            input,
            events,
            events_done,
            pending,
            diagnostics,
        })
    }

    fn finish_capture(&mut self, frames: [u64; 2]) -> Result<()> {
        let input = self.input.as_mut().context("recognition stdin closed")?;
        serde_json::to_writer(&mut *input, &frames)?;
        input.write_all(b"\n")?;
        input.flush()?;
        self.input.take();
        Ok(())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(crate) fn run_worker(raw: Vec<String>) -> Result<()> {
    let directory =
        PathBuf::from(std::env::var_os(WORKER_ENV).context("missing meeting directory")?);
    // Descendant model workers must not accidentally enter this service.
    std::env::remove_var(WORKER_ENV);
    let args = crate::cli::parse_args_with_default_model(&raw, || Ok(None))?;
    let options = Options {
        args,
        control: false,
        json: true,
        duration: None,
        output_directory: directory.clone(),
    };
    let mut events = Events {
        json: true,
        sequence: 0,
    };
    let result = transcribe_meeting(&options, &directory, &mut events);
    if let Err(error) = &result {
        let _ = events.send(
            "recognition_error",
            None,
            serde_json::json!({"message":format!("{error:#}"),"recording_continues":true}),
        );
    }
    result
}

fn transcribe_meeting(options: &Options, directory: &Path, events: &mut Events) -> Result<()> {
    let (send, finish) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = (|| {
            let length = io::stdin().lock().take(1025).read_line(&mut line)?;
            if length == 0 || length > 1024 || !line.ends_with('\n') {
                bail!("meeting capture control closed before finish");
            }
            Ok(serde_json::from_str::<[u64; 2]>(&line)?)
        })();
        let _ = send.send(result);
    });
    let mut loaded = crate::engine::load_engine(&options.args.model)?;
    events.send(
        "model_ready",
        None,
        serde_json::json!({"model":loaded.metadata,"load_seconds":loaded.load_seconds}),
    )?;
    let mut tracks = [
        LiveTrack::new(Track::Microphone)?,
        LiveTrack::new(Track::System)?,
    ];
    let mut final_frames = None;
    loop {
        let mut progress = false;
        for index in 0..2 {
            if let Ok(frames) = finish.try_recv() {
                final_frames = Some(frames?);
            }
            let available = if let Some(frames) = final_frames {
                frames[index]
            } else {
                std::fs::metadata(directory.join(if index == 0 {
                    "microphone.pcm"
                } else {
                    "system.pcm"
                }))?
                .len()
                    / 2
            };
            if tracks[index].position < available {
                progress = true;
            }
            if tracks[index].feed(directory, available, &mut loaded, final_frames.is_none())? {
                publish(options, events, directory, &mut tracks, &loaded)?;
            }
        }
        if let Some(frames) = final_frames {
            if tracks
                .iter()
                .zip(frames)
                .all(|(track, end)| track.position >= end)
            {
                for track in &mut tracks {
                    track.refiner.finish(|start, samples| {
                        track
                            .refined
                            .decode_chunk(loaded.engine.as_mut(), samples, start, true)
                    })?;
                }
                let result = snapshot(options, directory, &tracks, &loaded, true)?;
                save(directory, &result, false)?;
                events.send("transcribed", None, serde_json::json!({"directory":directory,"output_path":directory.join("transcript.json"),"result":result}))?;
                return Ok(());
            }
        }
        if !progress {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

struct LiveTrack {
    track: Track,
    position: u64,
    chunker: PcmChunker,
    outcome: InferenceOutcome,
    // Feed the same bounded reads to both profiles. Preview text is provisional;
    // only the longer-context outcome may occupy the final transcript filename.
    refiner: PcmChunker,
    refined: InferenceOutcome,
    printed: String,
}

impl LiveTrack {
    fn new(track: Track) -> Result<Self> {
        let mut chunker = PcmChunker::new(
            8 * crate::SAMPLE_RATE,
            crate::SAMPLE_RATE / 2,
            2 * crate::SAMPLE_RATE,
        )?;
        chunker.hold_opening();
        Ok(Self {
            track,
            position: 0,
            chunker,
            outcome: InferenceOutcome::default(),
            refiner: PcmChunker::new(
                (FINAL_CHUNK_SECONDS * crate::SAMPLE_RATE as f64) as usize,
                (FINAL_OVERLAP_SECONDS * crate::SAMPLE_RATE as f64) as usize,
                (FINAL_CHUNK_SECONDS * crate::SAMPLE_RATE as f64) as usize,
            )?,
            refined: InferenceOutcome::default(),
            printed: String::new(),
        })
    }

    fn feed(
        &mut self,
        directory: &Path,
        available: u64,
        engine: &mut LoadedEngine,
        preview: bool,
    ) -> Result<bool> {
        let count = available
            .saturating_sub(self.position)
            .min(crate::SAMPLE_RATE as u64 / 10) as usize;
        if count == 0 {
            return Ok(false);
        }
        let samples =
            crate::meeting_store::read_frames(directory, self.track, self.position, count)?;
        self.position += samples.len() as u64;
        let samples: Vec<_> = samples
            .into_iter()
            .map(|value| value as f32 / i16::MAX as f32)
            .collect();
        let previous = self.outcome.chunks.len();
        if preview {
            self.chunker.push(&samples, |start, samples| {
                self.outcome
                    .decode_chunk(engine.engine.as_mut(), samples, start, true)
            })?;
        }
        self.refiner.push(&samples, |start, samples| {
            self.refined
                .decode_chunk(engine.engine.as_mut(), samples, start, true)
        })?;
        if !self.outcome.text.is_empty() {
            self.chunker.use_steady_chunks();
        }
        Ok(self.outcome.chunks.len() != previous)
    }
}

fn snapshot(
    options: &Options,
    directory: &Path,
    tracks: &[LiveTrack; 2],
    loaded: &LoadedEngine,
    finalized: bool,
) -> Result<BenchmarkResult> {
    let mut outcome = InferenceOutcome::default();
    let preview_seconds: f64 = tracks
        .iter()
        .map(|track| track.outcome.transcribe_seconds)
        .sum();
    for (track, label) in tracks.iter().zip(["Microphone", "System"]) {
        let current = if finalized {
            &track.refined
        } else {
            &track.outcome
        };
        outcome.transcribe_seconds += current.transcribe_seconds;
        outcome.chunks.extend(current.chunks.iter().cloned());
        outcome
            .segments
            .extend(current.segments.iter().cloned().map(|mut segment| {
                segment.speaker = Some(label.into());
                segment
            }));
    }
    outcome
        .segments
        .sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
    outcome
        .chunks
        .sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
    for (index, chunk) in outcome.chunks.iter_mut().enumerate() {
        chunk.index = index;
    }
    outcome.text = outcome
        .segments
        .iter()
        .map(|segment| format!("{}: {}", segment.speaker.as_deref().unwrap(), segment.text))
        .collect::<Vec<_>>()
        .join("\n");
    if finalized {
        outcome.transcribe_seconds += preview_seconds;
    }
    let mut result = crate::transcribe::finalize_transcription(
        &options.args,
        "meeting",
        directory,
        "local-meeting-channels",
        PreparedAudio {
            wav_path: directory.into(),
            source_start_s: 0.0,
            // This legacy field specifically reports FFmpeg use. Meeting PCM
            // is normalized by the shared Rust resampler.
            normalized: false,
            _tempdir: None,
        },
        crate::transcribe::CompletedInference {
            outcome,
            load_seconds: loaded.load_seconds,
            directory: loaded.directory.clone(),
            metadata: loaded.metadata.clone(),
            audio_seconds: tracks.iter().map(|track| track.position).max().unwrap_or(0) as f64
                / crate::SAMPLE_RATE as f64,
        },
    )?;
    if finalized {
        result.chunk_seconds = Some(FINAL_CHUNK_SECONDS);
        result.chunk_overlap_seconds = FINAL_OVERLAP_SECONDS;
        result.preview_transcribe_seconds = Some(preview_seconds);
    }
    Ok(if options.args.clean_output {
        crate::output::cleaned_benchmark_result(&result)
    } else {
        result
    })
}

fn save(directory: &Path, result: &BenchmarkResult, partial: bool) -> Result<()> {
    crate::delivery::write_output_file(
        &serde_json::to_string_pretty(result)?,
        &directory.join(if partial {
            "transcript.partial.json"
        } else {
            ".transcript.pending.json"
        }),
    )?;
    Ok(())
}

fn publish(
    options: &Options,
    events: &mut Events,
    directory: &Path,
    tracks: &mut [LiveTrack; 2],
    loaded: &LoadedEngine,
) -> Result<()> {
    let result = snapshot(options, directory, tracks, loaded, false)?;
    save(directory, &result, true)?;
    events.send("update", None, serde_json::json!({"provisional":true,"text":result.text,"segments":result.segments,"addition":additions(tracks)?,"output_path":directory.join("transcript.partial.json")}))?;
    Ok(())
}

fn additions(tracks: &mut [LiveTrack; 2]) -> Result<String> {
    let mut output = String::new();
    for (track, label) in tracks.iter_mut().zip(["Microphone", "System"]) {
        let addition = track
            .outcome
            .text
            .strip_prefix(&track.printed)
            .context("meeting transcript prefix changed unexpectedly")?;
        if !addition.trim().is_empty() {
            output.push_str(&format!("{label}: {}\n", addition.trim()));
        }
        track.printed.clone_from(&track.outcome.text);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::SpeechEngine;
    use crate::types::ModelMetadata;
    use transcribe_rs::{TranscriptionResult, TranscriptionSegment};

    struct DigestEngine;
    impl SpeechEngine for DigestEngine {
        fn transcribe(&mut self, samples: &[f32]) -> Result<TranscriptionResult> {
            assert!(samples.len() <= 30 * crate::SAMPLE_RATE);
            let hash = samples
                .iter()
                .fold(14695981039346656037u64, |hash, sample| {
                    (hash ^ sample.to_bits() as u64).wrapping_mul(1099511628211)
                });
            let text = format!("{hash:x}");
            Ok(TranscriptionResult {
                text: text.clone(),
                segments: Some(vec![TranscriptionSegment {
                    start: 0.0,
                    end: samples.len() as f32 / crate::SAMPLE_RATE as f32,
                    text,
                }]),
            })
        }
    }

    fn engine() -> LoadedEngine {
        LoadedEngine {
            engine: Box::new(DigestEngine),
            load_seconds: 0.25,
            directory: "fixture".into(),
            metadata: ModelMetadata {
                name: "fixture".into(),
                source_url: "fixture".into(),
                runtime: "test".into(),
                device: "cpu".into(),
                revision: None,
                artifact_sha256: None,
            },
        }
    }

    #[test]
    fn property_refined_windows_match_batch_across_arrival_and_stop_boundaries() {
        for (seed, length) in [
            0,
            1600,
            30 * crate::SAMPLE_RATE,
            30 * crate::SAMPLE_RATE + 1,
            58 * crate::SAMPLE_RATE,
            74 * crate::SAMPLE_RATE + 123,
        ]
        .into_iter()
        .enumerate()
        {
            let root = tempfile::tempdir().unwrap();
            let directory = root.path().join("meeting");
            let mut writer = crate::meeting_store::Writer::create(&directory).unwrap();
            let mut random = seed as u64 + 1;
            let samples: Vec<i16> = (0..length)
                .map(|_| {
                    random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                    random as i16
                })
                .collect();
            writer.append(Track::Microphone, 0, &samples).unwrap();
            writer.finish().unwrap();
            let normalized: Vec<_> = samples
                .iter()
                .map(|value| *value as f32 / i16::MAX as f32)
                .collect();
            let expected = crate::inference::transcribe_samples(
                &mut DigestEngine,
                &normalized,
                Some(30.0),
                2.0,
            )
            .unwrap();
            let mut track = LiveTrack::new(Track::Microphone).unwrap();
            let mut loaded = engine();
            while track.position < length as u64 {
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                let available = (track.position + random % 17000 + 1).min(length as u64);
                let preview = track.position < length as u64 * 3 / 4;
                track
                    .feed(&directory, available, &mut loaded, preview)
                    .unwrap();
            }
            track
                .refiner
                .finish(|start, samples| {
                    track
                        .refined
                        .decode_chunk(loaded.engine.as_mut(), samples, start, true)
                })
                .unwrap();
            assert_eq!(track.refiner.received(), length);
            assert_eq!(track.refined.text, expected.text);
            assert_eq!(track.refined.segments, expected.segments);
            let chunks = |outcome: &InferenceOutcome| {
                outcome
                    .chunks
                    .iter()
                    .map(|chunk| (chunk.index, chunk.start_s, chunk.end_s, chunk.text.clone()))
                    .collect::<Vec<_>>()
            };
            assert_eq!(chunks(&track.refined), chunks(&expected));
            assert!(track
                .outcome
                .chunks
                .iter()
                .all(|chunk| chunk.audio_seconds <= 8.0));
            if length >= 3 * crate::SAMPLE_RATE {
                assert!(!track.outcome.chunks.is_empty());
            }
        }
    }

    #[test]
    fn final_metadata_counts_preview_compute_and_preserves_legacy_json() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("meeting");
        let mut writer = crate::meeting_store::Writer::create(&directory).unwrap();
        writer
            .append(Track::Microphone, 0, &vec![123; 3 * crate::SAMPLE_RATE])
            .unwrap();
        writer.finish().unwrap();
        let mut tracks = [
            LiveTrack::new(Track::Microphone).unwrap(),
            LiveTrack::new(Track::System).unwrap(),
        ];
        let mut loaded = engine();
        while tracks[0].position < 3 * crate::SAMPLE_RATE as u64 {
            tracks[0]
                .feed(&directory, 3 * crate::SAMPLE_RATE as u64, &mut loaded, true)
                .unwrap();
        }
        for track in &mut tracks {
            track
                .refiner
                .finish(|start, samples| {
                    track
                        .refined
                        .decode_chunk(loaded.engine.as_mut(), samples, start, true)
                })
                .unwrap();
        }
        let options = Options {
            args: crate::cli::parse_args_with_default_model(
                &[
                    "meeting".into(),
                    "--stream".into(),
                    "--no-diarization".into(),
                ],
                || Ok(None),
            )
            .unwrap(),
            control: false,
            json: true,
            duration: None,
            output_directory: root.path().into(),
        };
        let result = snapshot(&options, &directory, &tracks, &loaded, true).unwrap();
        let preview = result.preview_transcribe_seconds.unwrap();
        assert!(preview > 0.0);
        assert_eq!(result.chunk_seconds, Some(30.0));
        assert_eq!(result.chunk_overlap_seconds, 2.0);
        let final_seconds: f64 = result
            .chunks
            .iter()
            .map(|chunk| chunk.transcribe_seconds)
            .sum();
        assert!((result.transcribe_seconds - final_seconds - preview).abs() < 1e-9);
        assert!(
            (result.total_inside_seconds - result.load_seconds - result.transcribe_seconds).abs()
                < 1e-9
        );
        let mut legacy = serde_json::to_value(&result).unwrap();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("preview_transcribe_seconds");
        let parsed: BenchmarkResult = serde_json::from_value(legacy).unwrap();
        assert!(parsed.preview_transcribe_seconds.is_none());
    }
}
