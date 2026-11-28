//! Passive call evidence and one shared, monotonic detection policy. No capture.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CONFIRM_MS: u64 = 5_000;
const END_MS: u64 = 20_000;
const CONTINUITY_MS: u64 = 5_000;
const INTERVAL: Duration = Duration::from_secs(2);

// Match actual audio-process identities, including dotted helper identities.
const APPS: &[(&str, &str)] = &[
    ("us.zoom.xos", "Zoom"),
    ("com.microsoft.teams2", "Microsoft Teams"),
    ("com.microsoft.teams", "Microsoft Teams"),
    ("com.apple.FaceTime", "FaceTime"),
    ("com.apple.avconferenced", "FaceTime"),
    ("Cisco-Systems.Spark", "Webex"),
    ("com.cisco.webexmeetingsapp", "Webex"),
    ("com.webex.meetingmanager", "Webex"),
    ("com.tinyspeck.slackmacgap", "Slack"),
    ("com.hnc.Discord", "Discord"),
    ("net.whatsapp.WhatsApp", "WhatsApp"),
    ("desktop.WhatsApp", "WhatsApp"),
    ("org.whispersystems.signal-desktop", "Signal"),
    ("ru.keepcoder.Telegram", "Telegram"),
    ("org.telegram.desktop", "Telegram"),
    ("com.tdesktop.Telegram", "Telegram"),
    ("app.tuple.app", "Tuple"),
    ("co.around.Around", "Around"),
    ("ca.illusive.openphone", "OpenPhone"),
    ("com.infomaniak.meet", "kMeet"),
    ("com.google.Chrome", "Google Chrome"),
    ("company.thebrowser.Browser", "Arc"),
    ("company.thebrowser.dia", "Dia"),
    ("com.brave.Browser", "Brave"),
    ("com.microsoft.edgemac", "Microsoft Edge"),
    ("org.mozilla.firefox", "Firefox"),
    ("org.mozilla.plugincontainer", "Firefox"),
    ("com.vivaldi.Vivaldi", "Vivaldi"),
    ("com.operasoftware.Opera", "Opera"),
    ("org.chromium.Chromium", "Chromium"),
    ("com.apple.Safari", "Safari"),
    ("com.apple.WebKit", "Safari"),
];

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct AudioClient {
    pub pid: i32,
    pub bundle_id: String,
    pub input: bool,
    pub output: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Snapshot {
    pub complete: bool,
    pub clients: Vec<AudioClient>,
    #[serde(default)]
    pub unavailable_processes: usize,
}

fn app_name(bundle: &str) -> Option<&'static str> {
    let bundle = bundle.to_ascii_lowercase();
    APPS.iter().find_map(|(id, name)| {
        let id = id.to_ascii_lowercase();
        (bundle == id
            || bundle
                .strip_prefix(&id)
                .is_some_and(|suffix| suffix.starts_with('.')))
        .then_some(*name)
    })
}

impl Snapshot {
    fn activity(&self) -> (BTreeSet<&'static str>, BTreeSet<&'static str>) {
        let mut input = BTreeSet::new();
        let mut output = BTreeSet::new();
        for client in &self.clients {
            if let Some(name) = app_name(&client.bundle_id) {
                if client.input {
                    input.insert(name);
                }
                if client.output {
                    output.insert(name);
                }
            }
        }
        (input, output)
    }

    pub(crate) fn has_call_input(&self) -> bool {
        !self.activity().0.is_empty()
    }

    fn candidates(&self) -> Vec<&'static str> {
        let (input, output) = self.activity();
        input.intersection(&output).copied().collect()
    }
}

#[derive(Default)]
struct Detector {
    candidates: BTreeMap<&'static str, u64>,
    active: BTreeMap<&'static str, Option<u64>>,
    previous_ms: Option<u64>,
}

impl Detector {
    fn update(
        &mut self,
        now: u64,
        snapshot: &Snapshot,
    ) -> Result<Vec<(&'static str, &'static str)>> {
        if self.previous_ms.is_some_and(|previous| now < previous) {
            bail!("call observation time moved backwards");
        }
        if !snapshot.complete
            || self
                .previous_ms
                .is_some_and(|previous| now - previous > CONTINUITY_MS)
        {
            self.candidates.clear();
            for missing_since in self.active.values_mut() {
                *missing_since = None;
            }
        }
        self.previous_ms = Some(now);
        if !snapshot.complete {
            return Ok(Vec::new());
        }
        let (input, output) = snapshot.activity();
        let corroborated: BTreeSet<_> = input.intersection(&output).copied().collect();
        let mut events = Vec::new();
        self.active.retain(|app, missing_since| {
            if input.contains(app) {
                *missing_since = None;
            } else if now - *missing_since.get_or_insert(now) >= END_MS {
                events.push(("ended", *app));
                return false;
            }
            true
        });
        self.candidates.retain(|app, _| corroborated.contains(app));
        for app in corroborated {
            if self.active.contains_key(app) {
                continue;
            }
            let since = self.candidates.entry(app).or_insert(now);
            if now - *since >= CONFIRM_MS {
                self.candidates.remove(app);
                self.active.insert(app, None);
                events.push(("started", app));
            }
        }
        // Stable event order: endings precede starts, alphabetically per group.
        events.sort_by_key(|(kind, app)| (*kind == "started", *app));
        Ok(events)
    }
}

#[derive(Deserialize)]
struct Frame {
    at_ms: u64,
    #[serde(flatten)]
    snapshot: Snapshot,
}

#[derive(Deserialize)]
struct Trace {
    frames: Vec<Frame>,
    #[serde(default)]
    hold: bool,
    #[serde(default)]
    interval_ms: u64,
}

/// One evidence source and timing policy for inspection and automatic meetings.
/// Explicit traces may compress wall time; native observations always use Instant.
pub(crate) struct Monitor {
    trace: Option<Trace>,
    began: Instant,
    next: Instant,
    index: usize,
    detector: Detector,
}

pub(crate) struct Observation {
    pub at_ms: u64,
    pub snapshot: Snapshot,
    pub transitions: Vec<(&'static str, &'static str)>,
    pub active: Vec<&'static str>,
}

impl Monitor {
    pub(crate) fn new() -> Result<Self> {
        let trace = std::env::var_os("FSCRIPT_CALLS_TEST_TRACE")
            .map(|path| -> Result<Trace> {
                let bytes = std::fs::read(path).context("cannot read call test trace")?;
                let trace: Trace =
                    serde_json::from_slice(&bytes).context("invalid call test trace")?;
                if trace.frames.is_empty() {
                    bail!("call test trace has no frames");
                }
                Ok(trace)
            })
            .transpose()?;
        let began = Instant::now();
        Ok(Self {
            trace,
            began,
            next: began,
            index: 0,
            detector: Detector::default(),
        })
    }

    pub(crate) fn finished(&self) -> bool {
        self.trace
            .as_ref()
            .is_some_and(|trace| !trace.hold && self.index >= trace.frames.len())
    }

    pub(crate) fn poll(&mut self) -> Result<Option<Observation>> {
        if Instant::now() < self.next {
            return Ok(None);
        }
        let (at_ms, snapshot, interval) = if let Some(trace) = &self.trace {
            let Some(frame) = trace.frames.get(self.index) else {
                return Ok(None);
            };
            (
                frame.at_ms,
                frame.snapshot.clone(),
                Duration::from_millis(trace.interval_ms),
            )
        } else {
            (
                u64::try_from(self.began.elapsed().as_millis())
                    .context("call observation clock overflow")?,
                native_snapshot()?,
                INTERVAL,
            )
        };
        let transitions = self.detector.update(at_ms, &snapshot)?;
        self.index += 1;
        self.next = Instant::now()
            .checked_add(interval)
            .context("call observation interval is too large")?;
        Ok(Some(Observation {
            at_ms,
            snapshot,
            transitions,
            active: self.detector.active.keys().copied().collect(),
        }))
    }
}

pub(crate) fn run(watch: bool, duration: Option<&str>, json: bool) -> Result<()> {
    let duration = duration
        .map(|value| -> Result<Duration> {
            let seconds: f64 = value
                .parse()
                .context("duration must be positive finite seconds")?;
            if !seconds.is_finite() || seconds <= 0.0 {
                bail!("duration must be positive finite seconds");
            }
            Duration::try_from_secs_f64(seconds).context("duration is too large")
        })
        .transpose()?;
    let began = Instant::now();
    let mut monitor = Monitor::new()?;
    let mut sequence = 0u64;
    let mut index = 0usize;
    let mut stdout = std::io::stdout().lock();
    loop {
        if watch && index > 0 && duration.is_some_and(|duration| began.elapsed() >= duration) {
            break;
        }
        let Some(observation) = monitor.poll()? else {
            if monitor.finished() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
            continue;
        };
        let Observation {
            at_ms,
            snapshot,
            transitions,
            active,
        } = observation;
        let candidates = snapshot.candidates();
        if !watch {
            if json {
                serde_json::to_writer_pretty(
                    &mut stdout,
                    &serde_json::json!({
                        "version":1,"snapshot":snapshot,"candidates":candidates,
                        "observed_at_unix_ms":SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis()
                    }),
                )?;
                writeln!(stdout)?;
            } else if candidates.is_empty() {
                writeln!(stdout, "No call audio detected in this snapshot.")?;
            } else {
                writeln!(stdout, "Call audio candidates: {}", candidates.join(", "))?;
            }
            if !snapshot.complete && !json {
                writeln!(
                    stdout,
                    "Some audio-process metadata is unavailable; detection is uncertain."
                )?;
            }
            stdout.flush()?;
            return Ok(());
        }
        for (kind, app) in transitions {
            if json {
                event(
                    &mut stdout,
                    &mut sequence,
                    kind,
                    serde_json::json!({"app":app,"observed_ms":at_ms}),
                )?;
            } else {
                writeln!(stdout, "Call {kind}: {app}")?;
                stdout.flush()?;
            }
        }
        if json {
            event(
                &mut stdout,
                &mut sequence,
                "snapshot",
                serde_json::json!({
                    "observed_ms":at_ms,"snapshot":snapshot,"candidates":candidates,
                    "active":active
                }),
            )?;
        } else if index == 0 {
            eprintln!(
                "Watching call audio activity; Ctrl-C stops inspection. No audio is recorded."
            );
        }
        index += 1;
    }
    if json {
        event(
            &mut stdout,
            &mut sequence,
            "completed",
            serde_json::json!({}),
        )?;
    }
    Ok(())
}

fn event(
    out: &mut impl Write,
    sequence: &mut u64,
    kind: &str,
    data: serde_json::Value,
) -> Result<()> {
    serde_json::to_writer(
        &mut *out,
        &serde_json::json!({"version":1,"sequence":*sequence,"type":kind,"data":data}),
    )?;
    writeln!(out)?;
    out.flush()?;
    *sequence = sequence
        .checked_add(1)
        .context("call event sequence overflow")?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn native_snapshot() -> Result<Snapshot> {
    bail!("native call inspection requires macOS 14.2 or later")
}

#[cfg(target_os = "macos")]
fn native_snapshot() -> Result<Snapshot> {
    use crate::system_capture::{object_ids, property};
    use objc2_core_audio::*;
    use objc2_core_foundation::{CFRetained, CFString};
    use std::ptr::NonNull;
    objc2::rc::autoreleasepool(|_| {
        let objects = unsafe { object_ids(kAudioHardwarePropertyProcessObjectList)? };
        let mut snapshot = Snapshot {
            complete: true,
            clients: Vec::new(),
            unavailable_processes: 0,
        };
        for object in objects {
            let read = || -> Result<AudioClient> {
                let pid: i32 = unsafe { property(object, kAudioProcessPropertyPID, None)? };
                let input: u32 =
                    unsafe { property(object, kAudioProcessPropertyIsRunningInput, None)? };
                let output: u32 =
                    unsafe { property(object, kAudioProcessPropertyIsRunningOutput, None)? };
                let raw: *mut CFString =
                    unsafe { property(object, kAudioProcessPropertyBundleID, None)? };
                let bundle_id = NonNull::new(raw)
                    .map(|raw| unsafe { CFRetained::from_raw(raw) }.to_string())
                    .unwrap_or_default();
                Ok(AudioClient {
                    pid,
                    bundle_id,
                    input: input != 0,
                    output: output != 0,
                })
            };
            match read() {
                Ok(client)
                    if client.pid != std::process::id() as i32
                        && (client.input || client.output) =>
                {
                    snapshot.clients.push(client)
                }
                Ok(_) => (),
                Err(_) => snapshot.unavailable_processes += 1,
            }
        }
        // A vanished process is normal. Consumers must not count unavailable
        // metadata as known silence or use it to confirm a call/automatic stop.
        snapshot.complete = snapshot.unavailable_processes == 0;
        snapshot
            .clients
            .sort_by(|a, b| (&a.bundle_id, a.pid).cmp(&(&b.bundle_id, b.pid)));
        Ok(snapshot)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(input: bool, output: bool) -> Snapshot {
        Snapshot {
            complete: true,
            unavailable_processes: 0,
            clients: vec![AudioClient {
                pid: 12,
                bundle_id: "us.zoom.xos.helper".into(),
                input,
                output,
            }],
        }
    }

    #[test]
    fn confirmation_and_grace_require_continuous_known_activity() {
        let mut detector = Detector::default();
        for time in [0, 2_000, 4_000] {
            assert!(detector
                .update(time, &sample(true, true))
                .unwrap()
                .is_empty());
        }
        assert_eq!(
            detector.update(6_000, &sample(true, true)).unwrap(),
            [("started", "Zoom")]
        );
        assert!(detector
            .update(8_000, &sample(false, false))
            .unwrap()
            .is_empty());
        for time in (10_000..28_000).step_by(2_000) {
            assert!(detector
                .update(time, &sample(false, false))
                .unwrap()
                .is_empty());
        }
        assert_eq!(
            detector.update(28_000, &sample(false, false)).unwrap(),
            [("ended", "Zoom")]
        );
        assert!(detector.update(27_000, &sample(false, false)).is_err());
    }

    #[test]
    fn unknown_intervals_and_sleep_never_confirm_or_end_calls() {
        let mut detector = Detector::default();
        detector.update(0, &sample(true, true)).unwrap();
        assert!(detector
            .update(60_000, &sample(true, true))
            .unwrap()
            .is_empty());
        for time in [62_000, 64_000] {
            detector.update(time, &sample(true, true)).unwrap();
        }
        assert_eq!(
            detector.update(66_000, &sample(true, true)).unwrap(),
            [("started", "Zoom")]
        );
        let mut unknown = sample(false, false);
        unknown.complete = false;
        assert!(detector.update(68_000, &unknown).unwrap().is_empty());
        assert!(detector
            .update(100_000, &sample(false, false))
            .unwrap()
            .is_empty());
        assert!(detector.active.contains_key("Zoom"));
    }

    #[test]
    fn property_unrelated_apps_and_bundle_prefixes_cannot_corroborate() {
        for index in 0..1_000 {
            let mut snapshot = sample(true, false);
            snapshot.clients.push(AudioClient {
                pid: 20,
                bundle_id: format!("com.unknown.{index}"),
                input: false,
                output: true,
            });
            assert!(snapshot.candidates().is_empty());
            snapshot.clients[1].bundle_id = "com.microsoft.teams".into();
            assert!(snapshot.candidates().is_empty());
            snapshot.clients[1].bundle_id = "US.ZOOM.XOS.helper.output".into();
            assert_eq!(snapshot.candidates(), ["Zoom"]);
            assert!(app_name(&format!("us.zoom.xos{index}")).is_none());
            assert!(app_name("com.brenorb.fast-transcript.cli").is_none());
        }
    }

    #[test]
    fn differential_random_traces_match_independent_sample_counters() {
        let mut transitions = [0usize; 2];
        for seed in 1..=128u64 {
            let mut random = seed;
            let mut detector = Detector::default();
            let mut state = [0u64; 2];
            let mut confirmed = [false; 2];
            let mut positive = [0; 2];
            let mut missing = [0; 2];
            for tick in 0..512u64 {
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                for (index, bits) in state.iter_mut().enumerate() {
                    if tick % (13 + index as u64 * 18) == 0 {
                        *bits = (random >> (index * 5 + 13)) & 3;
                    }
                }
                let complete = random % 127 != 0;
                let snapshot = Snapshot {
                    complete,
                    unavailable_processes: usize::from(!complete),
                    clients: vec![
                        AudioClient {
                            pid: 1,
                            bundle_id: "us.zoom.xos".into(),
                            input: state[0] & 1 != 0,
                            output: state[0] & 2 != 0,
                        },
                        AudioClient {
                            pid: 2,
                            bundle_id: "com.microsoft.teams2.helper".into(),
                            input: state[1] & 1 != 0,
                            output: state[1] & 2 != 0,
                        },
                    ],
                };
                let mut expected = Vec::new();
                for (index, name) in ["Zoom", "Microsoft Teams"].iter().enumerate() {
                    if !complete {
                        positive[index] = 0;
                        missing[index] = 0;
                        continue;
                    }
                    if confirmed[index] {
                        if state[index] & 1 == 0 {
                            missing[index] += 1;
                        } else {
                            missing[index] = 0;
                        }
                        if missing[index] >= 21 {
                            confirmed[index] = false;
                            positive[index] = 0;
                            expected.push(("ended", *name));
                        }
                    } else {
                        if state[index] == 3 {
                            positive[index] += 1;
                        } else {
                            positive[index] = 0;
                        }
                        if positive[index] >= 6 {
                            confirmed[index] = true;
                            missing[index] = 0;
                            expected.push(("started", *name));
                        }
                    }
                }
                expected.sort_by_key(|(kind, app)| (*kind == "started", *app));
                for (kind, _) in &expected {
                    transitions[usize::from(*kind == "ended")] += 1;
                }
                assert_eq!(
                    detector.update(tick * 1_000, &snapshot).unwrap(),
                    expected,
                    "seed {seed}, tick {tick}"
                );
            }
        }
        assert!(transitions.into_iter().all(|count| count > 100));
    }
}
