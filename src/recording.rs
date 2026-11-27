//! Reusable dictation session. The CLI and desktop use the same capture and model.

use crate::session_events::Events;
use anyhow::{bail, Result};
use serde::Deserialize;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::capture::Capture;
use crate::engine::{load_engine, LoadedEngine};
use crate::types::CliArgs;

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn stop(_: std::ffi::c_int) {
    STOP.store(true, Ordering::Relaxed);
}

pub(crate) struct Options {
    pub(crate) args: CliArgs,
    pub(crate) control: bool,
    pub(crate) hotkey: Option<String>,
    pub(crate) json: bool,
    pub(crate) duration: Option<Duration>,
    pub(crate) output_directory: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Start,
    Stop,
    Cancel,
    Shutdown,
}

enum Input {
    Request(Request),
    Closed,
    Invalid(String),
}

struct Active {
    id: String,
    capture: Capture,
    scratch: tempfile::TempDir,
    started: Instant,
    shortcut: bool,
}
impl Active {
    fn start(shortcut: bool) -> Result<Self> {
        let scratch = tempfile::Builder::new()
            .prefix("fscript-dictation-")
            .tempdir()?;
        let id = format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        );
        let capture = Capture::start(&scratch.path().join("microphone.wav"))?;
        Ok(Self {
            id,
            capture,
            scratch,
            started: Instant::now(),
            shortcut,
        })
    }
}

pub(crate) fn run(options: Options) -> Result<()> {
    if !options.control
        && options.hotkey.is_none()
        && options.duration.is_none()
        && !io::stdin().is_terminal()
    {
        bail!(
            "record needs a terminal, --duration SECONDS, or --control for unattended start/stop"
        );
    }
    crate::capture::check_platform()?;
    crate::session_events::install_stop_handler(stop)?;
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

fn input(control: bool) -> mpsc::Receiver<Input> {
    let (sender, receiver) = mpsc::sync_channel(32);
    if control {
        std::thread::spawn(move || {
            let mut stdin = io::stdin().lock();
            loop {
                let mut line = String::new();
                let value = match (&mut stdin).take(65537).read_line(&mut line) {
                    Ok(0) => Input::Closed,
                    Ok(_) if line.len() > 65536 || !line.ends_with('\n') => Input::Invalid(
                        "control requests must be newline-terminated JSON of at most 64 KiB".into(),
                    ),
                    Ok(_) => match serde_json::from_str(&line) {
                        Ok(request) => Input::Request(request),
                        Err(error) => {
                            Input::Invalid(format!("invalid recording control request: {error}"))
                        }
                    },
                    Err(error) => Input::Invalid(format!("cannot read recording control: {error}")),
                };
                let terminal = matches!(value, Input::Closed | Input::Invalid(_));
                if sender.send(value).is_err() || terminal {
                    break;
                }
            }
        });
    }
    receiver
}

fn run_session(options: &Options, events: &mut Events) -> Result<()> {
    let input = input(options.control);
    eprintln!("loading dictation model...");
    let mut loaded = load_engine(&options.args.model)?;
    let mut shortcut_error = None;
    let mut shortcut = if options.hotkey.is_some() {
        match crate::shortcut::Shortcut::register(options.hotkey.as_deref().unwrap()) {
            Ok(shortcut) => Some(shortcut),
            Err(error) if options.control => {
                shortcut_error = Some(format!("{error:#}"));
                None
            }
            Err(error) => return Err(error),
        }
    } else {
        None
    };
    events.send(
        "ready",
        None,
        serde_json::json!({"model":loaded.metadata,"load_seconds":loaded.load_seconds,"shortcut_available":shortcut.is_some(),"shortcut_error":shortcut_error,"shortcut_label":shortcut.as_ref().map(crate::shortcut::Shortcut::label)}),
    )?;
    if !events.json {
        if let Some(shortcut) = &shortcut {
            eprintln!(
                "hold {} to record; release to transcribe; Ctrl-C to cancel and exit",
                shortcut.label()
            );
        }
    }
    let mut active: Option<Active> = None;
    let mut next = (!options.control && options.hotkey.is_none()).then_some(Request::Start);
    let mut from_shortcut = false;
    let mut completed = 0usize;
    loop {
        if let Some(shortcut) = shortcut.as_mut() {
            for held in shortcut.poll()? {
                if held && active.is_none() {
                    next = Some(Request::Start);
                    from_shortcut = true;
                } else if !held && active.as_ref().is_some_and(|current| current.shortcut) {
                    next = Some(Request::Stop);
                } else if !held
                    && next
                        .as_ref()
                        .is_some_and(|request| matches!(request, Request::Start))
                    && from_shortcut
                {
                    next = None;
                    from_shortcut = false;
                }
            }
        }
        if !options.control && options.hotkey.is_none() && STOP.swap(false, Ordering::Relaxed) {
            if active.is_none() {
                bail!("recording stopped before the microphone started");
            }
            next = Some(Request::Stop);
        }
        if let Some(current) = active.as_mut() {
            current.capture.poll()?;
            if options
                .duration
                .is_some_and(|duration| current.started.elapsed() >= duration)
            {
                next = Some(Request::Stop);
            }
        }
        let request = if let Some(request) = next.take() {
            Some(request)
        } else {
            match input.recv_timeout(Duration::from_millis(10)) {
                Ok(Input::Request(request)) => {
                    from_shortcut = false;
                    Some(request)
                }
                Ok(Input::Closed) => {
                    if active.is_some() {
                        bail!("recording controller closed before stop; unfinished audio was discarded");
                    }
                    return Ok(());
                }
                Ok(Input::Invalid(message)) => bail!("{message}"),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    std::thread::sleep(Duration::from_millis(10));
                    None
                }
                Err(mpsc::RecvTimeoutError::Timeout) => None,
            }
        };
        let Some(request) = request else { continue };
        match request {
            Request::Start if active.is_none() => match Active::start(from_shortcut) {
                Ok(current) => {
                    events.send(
                        "recording",
                        Some(&current.id),
                        serde_json::json!({"source":"microphone","trigger":if from_shortcut {"shortcut"} else {"control"}}),
                    )?;
                    eprintln!(
                        "recording microphone... {}",
                        if options.control {
                            "send stop to transcribe".to_string()
                        } else if let Some(shortcut) = &shortcut {
                            format!("release {} to finish", shortcut.label())
                        } else {
                            "press Ctrl-C to finish; press again to cancel".to_string()
                        }
                    );
                    active = Some(current);
                    from_shortcut = false;
                }
                Err(error) if options.control || options.hotkey.is_some() => {
                    from_shortcut = false;
                    let _ = writeln!(io::stderr().lock(), "error: {error:#}");
                    events.send(
                        "error",
                        None,
                        serde_json::json!({"message":format!("{error:#}"),"fatal":false}),
                    )?;
                }
                Err(error) => return Err(error),
            },
            Request::Stop if active.is_some() => {
                let current = active.take().unwrap();
                let id = current.id.clone();
                events.send("processing", Some(&id), serde_json::json!({}))?;
                match finish(current, options, &mut loaded, completed == 0) {
                    Ok((result, path)) => {
                        completed += 1;
                        events.send("completed", Some(&id), serde_json::json!({"result":result,"output_path":path,"model_reused":completed > 1}))?;
                        if !events.json {
                            let text = crate::output::render_output(
                                &result,
                                options.args.output_format,
                                false,
                            )?;
                            crate::delivery::write_stdout(
                                &mut io::stdout().lock(),
                                format_args!("{text}\n"),
                            )?;
                        }
                    }
                    Err(error) if options.control || options.hotkey.is_some() => {
                        events.send(
                            "error",
                            Some(&id),
                            serde_json::json!({"message":format!("{error:#}"),"fatal":false}),
                        )?;
                    }
                    Err(error) => return Err(error),
                }
                if let Some(shortcut) = shortcut.as_mut() {
                    let _ = shortcut.poll()?;
                }
                if !options.control && options.hotkey.is_none() {
                    return Ok(());
                }
            }
            Request::Cancel => {
                if let Some(current) = active.take() {
                    events.send("cancelled", Some(&current.id), serde_json::json!({}))?;
                } else {
                    events.send(
                        "error",
                        None,
                        serde_json::json!({"message":"no recording is active","fatal":false}),
                    )?;
                }
            }
            Request::Shutdown => {
                if let Some(current) = active.take() {
                    events.send("cancelled", Some(&current.id), serde_json::json!({}))?;
                }
                return Ok(());
            }
            _ => {
                events.send("error", active.as_ref().map(|current| current.id.as_str()), serde_json::json!({"message":"start requires idle state; stop requires an active recording","fatal":false}))?;
            }
        }
    }
}

fn finish(
    current: Active,
    options: &Options,
    loaded: &mut LoadedEngine,
    first: bool,
) -> Result<(crate::types::BenchmarkResult, Option<PathBuf>)> {
    let Active {
        capture,
        scratch,
        id,
        ..
    } = current;
    capture.finish()?;
    let audio = scratch.path().join("microphone.wav");
    let prepared = crate::audio::normalize_audio(&audio)?;
    let samples = transcribe_rs::audio::read_wav_samples(&prepared.wav_path)?;
    let outcome = crate::inference::transcribe_samples(
        loaded.engine.as_mut(),
        &samples,
        options.args.chunk_seconds,
        options.args.chunk_overlap_seconds,
    )?;
    let result = crate::transcribe::finalize_transcription(
        &options.args,
        "microphone",
        &audio,
        "local-microphone",
        prepared,
        crate::transcribe::CompletedInference {
            outcome,
            load_seconds: if first { loaded.load_seconds } else { 0.0 },
            directory: loaded.directory.clone(),
            metadata: loaded.metadata.clone(),
            audio_seconds: samples.len() as f64 / crate::SAMPLE_RATE as f64,
        },
    )?;
    let result = crate::output::cleaned_benchmark_result(&result);
    let path = if let Some(directory) = &options.output_directory {
        let content =
            crate::output::render_output(&result, crate::types::OutputFormat::Json, false)?;
        Some(crate::delivery::write_output_file(
            &content,
            &directory.join(id).join("transcript.json"),
        )?)
    } else {
        None
    };
    Ok((result, path))
}
