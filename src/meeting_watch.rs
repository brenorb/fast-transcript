//! Automatic meeting orchestration. Capture and recognition remain in `meeting`.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub(crate) fn preferences_path() -> PathBuf {
    std::env::var_os("FSCRIPT_MEETING_CONFIG_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::settings::config_directory().join("meetings.json"))
}

#[derive(Default, Deserialize, Serialize)]
struct Preferences {
    version: u8,
    enabled: bool,
}

pub(crate) fn preference(action: &str, json_output: bool) -> Result<()> {
    let path = preferences_path();
    let mut preferences = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice::<Preferences>(&bytes)
            .with_context(|| format!("invalid meeting preferences in {}", path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Preferences {
            version: 1,
            enabled: false,
        },
        Err(error) => return Err(error).context("cannot read meeting preferences"),
    };
    if preferences.version != 1 {
        bail!(
            "unsupported meeting preference version {}",
            preferences.version
        );
    }
    match action {
        "enable" | "disable" => {
            preferences.enabled = action == "enable";
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| std::path::Path::new("."));
            std::fs::create_dir_all(parent)?;
            let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
            serde_json::to_writer_pretty(&mut temporary, &preferences)?;
            writeln!(temporary)?;
            temporary.as_file().sync_all()?;
            temporary
                .persist(&path)
                .context("cannot save meeting preferences")?;
        }
        "status" => {}
        _ => bail!("unknown automatic meeting preference action"),
    }
    crate::delivery::write_stdout(
        &mut std::io::stdout().lock(),
        format_args!(
            "{}\n",
            if json_output {
                serde_json::to_string(&preferences)?
            } else {
                format!("Automatic meetings: {}. Run fscript meeting watch to record detected calls in the foreground.", if preferences.enabled { "enabled for desktop startup" } else { "disabled for desktop startup" })
            }
        ),
    )
}

pub(crate) struct Options {
    pub control: bool,
    pub json: bool,
    pub duration: Option<Duration>,
    pub meeting_arguments: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Accept { episode: u64 },
    Skip { episode: u64 },
    Stop { episode: u64 },
    Cancel { episode: u64 },
    Suppress,
    Shutdown,
}

fn input() -> mpsc::Receiver<Result<Request>> {
    let (sender, receiver) = mpsc::sync_channel(16);
    std::thread::spawn(move || {
        let mut input = std::io::stdin().lock();
        loop {
            let mut line = String::new();
            let request = (|| {
                let count = (&mut input).take(65537).read_line(&mut line)?;
                if count == 0 {
                    bail!("automatic meeting control closed");
                }
                if count > 65536 || !line.ends_with('\n') {
                    bail!("control requests must be newline-terminated JSON of at most 64 KiB");
                }
                serde_json::from_str(&line).context("invalid automatic meeting control request")
            })();
            let terminal = request.is_err();
            if sender.send(request).is_err() || terminal {
                break;
            }
        }
    });
    receiver
}

/// A confirmed continuous call interval is one episode, even across app handoff.
/// An accepted/skipped/stopped episode remains consumed until observed absence.
#[derive(Default)]
struct Policy {
    episode: u64,
    active: bool,
    consumed: bool,
    suppressed: bool,
}

impl Policy {
    fn observe(&mut self, observation: &crate::calls::Observation) -> Result<()> {
        let active = !observation.active.is_empty();
        if active && !self.active {
            self.episode = self
                .episode
                .checked_add(1)
                .context("meeting episode overflow")?;
            self.consumed = false;
        }
        self.active = active;
        if observation.snapshot.complete && !active && !observation.snapshot.has_call_input() {
            self.suppressed = false;
        }
        Ok(())
    }

    fn offered(&self) -> Option<u64> {
        (self.active && !self.consumed && !self.suppressed).then_some(self.episode)
    }

    fn accept(&mut self, episode: u64) -> bool {
        if self.offered() != Some(episode) {
            return false;
        }
        self.consumed = true;
        true
    }

    fn suppress(&mut self) {
        self.suppressed = true;
        self.consumed = true;
    }
}

struct Session {
    episode: u64,
    child: Child,
    input: Option<ChildStdin>,
    events: mpsc::Receiver<Result<Value>>,
    reader: Option<std::thread::JoinHandle<()>>,
    started: bool,
    stop_requested: bool,
    terminal: bool,
    sequence: u64,
    output_done: bool,
}

impl Session {
    fn start(episode: u64, arguments: &[String]) -> Result<Self> {
        // Deliberately inherit JOB_ENV and the outer worker's process group.
        // One supervisor owns this child, ASR, converters, and model helpers.
        let mut child = Command::new(std::env::current_exe()?)
            .args(["meeting", "--control", "--json"])
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("cannot start automatic meeting service")?;
        let input = child.stdin.take();
        let output = child
            .stdout
            .take()
            .context("meeting output pipe is missing")?;
        let (send, events) = mpsc::sync_channel(16);
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let event = line
                    .context("cannot read meeting output")
                    .and_then(|line| serde_json::from_str(&line).context("invalid meeting event"));
                let terminal = event.is_err();
                if send.send(event).is_err() || terminal {
                    break;
                }
            }
        });
        Ok(Self {
            episode,
            child,
            input,
            events,
            reader: Some(reader),
            started: false,
            stop_requested: false,
            terminal: false,
            sequence: 0,
            output_done: false,
        })
    }

    fn command(&mut self, action: &str) -> Result<()> {
        let input = self
            .input
            .as_mut()
            .context("meeting control pipe is closed")?;
        serde_json::to_writer(&mut *input, &json!({"action":action}))?;
        input.write_all(b"\n")?;
        input.flush()?;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.stop_requested = true;
        if self.started {
            self.command("stop")?;
            self.started = false;
        }
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        // On cancellation/error the entire watch returns: its outer supervisor
        // kills the group. Do not start another session behind a hung reader.
    }
}

fn emit(events: &mut crate::session_events::Events, kind: &str, data: Value) -> Result<()> {
    events.send(kind, None, data)
}

fn capture_available() -> Result<bool> {
    match crate::meeting_store::Lock::profile(
        "capture",
        "meeting capture is already active for this profile",
    ) {
        Ok(_probe) => Ok(true),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock) =>
        {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn run(options: Options) -> Result<()> {
    if std::env::var_os("FSCRIPT_CALLS_TEST_TRACE").is_some()
        && [
            "FSCRIPT_CAPTURE_TEST_WAV",
            "FSCRIPT_SYSTEM_CAPTURE_TEST_WAV",
        ]
        .iter()
        .any(|name| std::env::var_os(name).is_none())
    {
        bail!("automatic meeting call traces require both explicit capture WAV fixtures");
    }
    let _lease = crate::meeting_store::Lock::profile(
        "watch",
        "an automatic meeting watcher already owns this profile",
    )?;
    let mut monitor = crate::calls::Monitor::new()?;
    let requests = options.control.then(input);
    let mut events = crate::session_events::Events {
        json: options.json || options.control,
        sequence: 0,
    };
    let mut policy = Policy::default();
    let mut session: Option<Session> = None;
    let began = Instant::now();
    let mut ending = false;
    emit(
        &mut events,
        "watch_ready",
        json!({"requires_accept":options.control}),
    )?;
    if !events.json {
        eprintln!("Automatic meeting recording is armed. Ctrl-C cancels; audio is retained.");
    }
    loop {
        if !ending
            && options
                .duration
                .is_some_and(|limit| began.elapsed() >= limit)
        {
            ending = true;
            if let Some(current) = session.as_mut() {
                current.stop()?;
            }
        }
        if let Some(requests) = &requests {
            while let Ok(request) = requests.try_recv() {
                match request? {
                    Request::Accept { episode } => {
                        if !ending && session.is_none() && policy.accept(episode) {
                            session = Some(Session::start(episode, &options.meeting_arguments)?);
                            emit(&mut events, "accepted", json!({"episode":episode}))?;
                        } else {
                            emit(&mut events, "offer_expired", json!({"episode":episode}))?;
                        }
                    }
                    Request::Skip { episode } if policy.episode == episode => {
                        policy.consumed = true;
                    }
                    Request::Stop { episode } => {
                        if let Some(current) = session.as_mut().filter(|s| s.episode == episode) {
                            policy.suppress();
                            current.stop()?;
                        }
                    }
                    Request::Cancel { episode }
                        if session.as_ref().is_some_and(|s| s.episode == episode) =>
                    {
                        emit(&mut events, "watch_cancelled", json!({"episode":episode}))?;
                        return Ok(());
                    }
                    Request::Suppress => {
                        policy.suppress();
                        emit(&mut events, "suppressed", json!({}))?;
                    }
                    Request::Shutdown => {
                        emit(&mut events, "watch_cancelled", json!({}))?;
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
        if !ending {
            if let Some(observation) = monitor.poll()? {
                policy.observe(&observation)?;
                emit(
                    &mut events,
                    "call_status",
                    json!({"observed_ms":observation.at_ms,"complete":observation.snapshot.complete,"active":observation.active,"episode":policy.episode}),
                )?;
                if !policy.active {
                    if let Some(current) = session.as_mut() {
                        current.stop()?;
                    }
                }
                if session.is_none() {
                    if let Some(episode) = policy.offered() {
                        if !capture_available()? {
                            policy.consumed = true;
                            emit(
                                &mut events,
                                "covered",
                                json!({"episode":episode,"reason":"manual_recording"}),
                            )?;
                        } else if options.control {
                            emit(
                                &mut events,
                                "offer",
                                json!({"episode":episode,"apps":observation.active}),
                            )?;
                        } else if policy.accept(episode) {
                            session = Some(Session::start(episode, &options.meeting_arguments)?);
                            emit(&mut events, "accepted", json!({"episode":episode}))?;
                        }
                    }
                }
            }
            ending = monitor.finished()
                || options
                    .duration
                    .is_some_and(|limit| began.elapsed() >= limit);
            if ending {
                if let Some(current) = session.as_mut() {
                    current.stop()?;
                }
            }
        }
        if let Some(current) = session.as_mut() {
            loop {
                let event = match current.events.try_recv() {
                    Ok(event) => event?,
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        current.output_done = true;
                        break;
                    }
                };
                if event["version"] != 1 || event["sequence"].as_u64() != Some(current.sequence) {
                    bail!("out-of-order automatic meeting event");
                }
                current.sequence += 1;
                if !events.json && event["type"] == "update" {
                    if let Some(addition) = event["data"]["addition"].as_str() {
                        eprint!("{addition}");
                    }
                }
                if event["type"] == "ready" {
                    current.command("start")?;
                    current.started = true;
                    if current.stop_requested {
                        current.stop()?;
                    }
                }
                if event["type"] == "completed" {
                    current.terminal = true;
                    if !events.json {
                        if let Some(path) = event["data"]["output_path"].as_str() {
                            crate::delivery::write_stdout(
                                &mut std::io::stdout().lock(),
                                format_args!("{path}\n"),
                            )?;
                        }
                    }
                }
                emit(
                    &mut events,
                    "meeting",
                    json!({"episode":current.episode,"event":event}),
                )?;
            }
            if let Some(status) = current.child.try_wait()? {
                if current.output_done {
                    if !status.success() || !current.terminal {
                        bail!("automatic meeting ended without successful completion ({status}); saved audio is retained; re-enable watching explicitly");
                    }
                    let episode = current.episode;
                    current
                        .reader
                        .take()
                        .unwrap()
                        .join()
                        .map_err(|_| anyhow::anyhow!("meeting output reader panicked"))?;
                    session.take();
                    emit(
                        &mut events,
                        "session_exit",
                        json!({"episode":episode,"status":0}),
                    )?;
                }
            }
        }
        if ending && session.is_none() {
            emit(&mut events, "watch_completed", json!({}))?;
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(active: bool, input: bool, complete: bool) -> crate::calls::Observation {
        crate::calls::Observation {
            at_ms: 0,
            active: if active { vec!["Zoom"] } else { vec![] },
            transitions: vec![],
            snapshot: crate::calls::Snapshot {
                complete,
                unavailable_processes: 0,
                clients: if input {
                    vec![crate::calls::AudioClient {
                        pid: 1,
                        bundle_id: "us.zoom.xos".into(),
                        input: true,
                        output: true,
                    }]
                } else {
                    vec![]
                },
            },
        }
    }

    #[test]
    fn suppression_includes_preconfirmation_and_unknown_observations_do_not_clear_it() {
        let mut policy = Policy::default();
        policy.suppress();
        policy.observe(&observation(false, true, true)).unwrap();
        policy.observe(&observation(true, true, true)).unwrap();
        assert_eq!(policy.offered(), None);
        policy.observe(&observation(true, false, false)).unwrap();
        assert_eq!(policy.offered(), None);
        policy.observe(&observation(false, false, true)).unwrap();
        policy.observe(&observation(true, true, true)).unwrap();
        assert_eq!(policy.offered(), Some(2));
        assert!(!policy.accept(1));
        assert!(policy.accept(2));
        assert!(!policy.accept(2));
    }

    #[test]
    fn property_each_continuous_episode_is_accepted_at_most_once() {
        for seed in 1..=256u64 {
            let mut policy = Policy::default();
            let mut random = seed;
            let mut accepted = std::collections::BTreeSet::new();
            for _ in 0..512 {
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                let active = random % 7 != 0;
                policy.observe(&observation(active, active, true)).unwrap();
                if random % 11 == 0 {
                    policy.suppress();
                }
                if let Some(episode) = policy.offered() {
                    assert!(active);
                    assert!(policy.accept(episode));
                    assert!(accepted.insert(episode));
                    assert!(!policy.accept(episode));
                }
                assert!(!policy.accept(policy.episode.saturating_add(1)));
            }
        }
    }
}
