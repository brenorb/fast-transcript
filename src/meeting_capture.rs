//! Continuous two-track capture. Inference never owns these threads or their queues.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::capture::{host_time_ns, AudioSource};
use crate::clocked_pcm::ClockedPcm;
use crate::meeting_store::{SavedSession, Track, Writer};

fn lock(writer: &Mutex<Writer>) -> Result<MutexGuard<'_, Writer>> {
    writer
        .lock()
        .map_err(|_| anyhow::anyhow!("meeting writer panicked"))
}

/// A single immutable stop timestamp shared by both sources, also writable by
/// stdin/duration control while the calling thread is inside model inference.
pub(crate) struct Stop {
    epoch: u128,
    cutoff: AtomicU64,
}

impl Stop {
    fn new() -> Result<Self> {
        Ok(Self {
            epoch: host_time_ns()?,
            cutoff: AtomicU64::new(u64::MAX),
        })
    }

    pub(crate) fn request(&self) {
        let elapsed = host_time_ns()
            .unwrap_or(self.epoch)
            .saturating_sub(self.epoch);
        let elapsed = u64::try_from(elapsed)
            .unwrap_or(u64::MAX - 1)
            .min(u64::MAX - 1);
        let _ = self
            .cutoff
            .compare_exchange(u64::MAX, elapsed, Ordering::SeqCst, Ordering::SeqCst);
    }

    pub(crate) fn requested(&self) -> bool {
        self.cutoff.load(Ordering::SeqCst) != u64::MAX
    }

    fn observed_frame(&self) -> Result<u64> {
        let now: u64 = (host_time_ns()?.saturating_sub(self.epoch) * crate::SAMPLE_RATE as u128
            / 1_000_000_000)
            .try_into()
            .context("meeting clock overflow")?;
        Ok(self
            .frames(crate::SAMPLE_RATE as u32)
            .map_or(now, |cutoff| now.min(cutoff)))
    }

    fn frames(&self, rate: u32) -> Option<u64> {
        let cutoff = self.cutoff.load(Ordering::SeqCst);
        (cutoff != u64::MAX).then(|| (u128::from(cutoff) * u128::from(rate) / 1_000_000_000) as u64)
    }
}

pub(crate) struct Capture {
    writer: Arc<Mutex<Writer>>,
    pub(crate) stop: Arc<Stop>,
    ready: [Arc<AtomicBool>; 2],
    workers: Vec<JoinHandle<Result<()>>>,
    done: Arc<AtomicBool>,
    watchdog: Option<JoinHandle<()>>,
    pub(crate) events: mpsc::Receiver<serde_json::Value>,
    failure: Arc<Mutex<Option<anyhow::Error>>>,
}

impl Capture {
    pub(crate) fn start(
        directory: &Path,
        duration: Option<Duration>,
        signal: fn() -> bool,
    ) -> Result<Self> {
        let writer = Arc::new(Mutex::new(Writer::create(directory)?));
        let stop = Arc::new(Stop::new()?);
        let ready = [
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        ];
        let done = Arc::new(AtomicBool::new(false));
        let watch_stop = Arc::clone(&stop);
        let watch_done = Arc::clone(&done);
        let watchdog = std::thread::spawn(move || {
            while !watch_done.load(Ordering::Acquire) {
                if signal()
                    || duration.is_some_and(|limit| {
                        host_time_ns()
                            .unwrap_or(watch_stop.epoch)
                            .saturating_sub(watch_stop.epoch)
                            >= limit.as_nanos()
                    })
                {
                    watch_stop.request();
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let (notices, events) = mpsc::sync_channel(32);
        let mut capture = Self {
            writer,
            stop,
            ready,
            workers: Vec::new(),
            done,
            watchdog: Some(watchdog),
            events,
            failure: Arc::new(Mutex::new(None)),
        };
        for (index, track) in Track::ALL.into_iter().enumerate() {
            let writer = Arc::clone(&capture.writer);
            let stop = Arc::clone(&capture.stop);
            let ready = Arc::clone(&capture.ready[index]);
            let notices = notices.clone();
            let failure = Arc::clone(&capture.failure);
            capture.workers.push(std::thread::spawn(move || {
                let result = run_source(track, writer, Arc::clone(&stop), ready, notices);
                if let Err(error) = result {
                    let mut first = failure
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if first.is_none() {
                        *first = Some(error);
                    }
                    stop.request();
                }
                Ok(())
            }));
        }
        Ok(capture)
    }

    pub(crate) fn ready(&self) -> bool {
        self.ready.iter().all(|ready| ready.load(Ordering::Acquire))
    }

    pub(crate) fn finished(&self) -> bool {
        self.workers.iter().all(JoinHandle::is_finished)
    }

    /// Only a successful explicit finish marks the recording stopped. Drop,
    /// source failures and killed jobs leave an ownerless recoverable session.
    pub(crate) fn finish(mut self) -> Result<SavedSession> {
        self.stop.request();
        if let Err(error) = self.join() {
            // A handled failure is already unowned and interrupted once this
            // method returns. Crashes/cancel Drop still require explicit repair.
            if let Err(storage) = lock(&self.writer)?.interrupt() {
                return Err(error).context(format!(
                    "also unable to checkpoint interrupted audio: {storage:#}"
                ));
            }
            return Err(error);
        }
        // All capture threads have relinquished the store now.
        let placeholder = Arc::clone(&self.writer);
        let writer = std::mem::replace(&mut self.writer, placeholder);
        // Keep no second reference when taking ownership below.
        drop(self);
        let writer =
            Arc::try_unwrap(writer).map_err(|_| anyhow::anyhow!("meeting writer still in use"))?;
        writer
            .into_inner()
            .map_err(|_| anyhow::anyhow!("meeting writer panicked"))?
            .finish()
    }

    fn join(&mut self) -> Result<()> {
        let mut failure = None;
        for worker in self.workers.drain(..) {
            let result = worker
                .join()
                .map_err(|_| anyhow::anyhow!("meeting capture thread panicked"))
                .and_then(|result| result);
            if let Err(error) = result {
                failure.get_or_insert(error);
            }
        }
        self.done.store(true, Ordering::Release);
        if let Some(watchdog) = self.watchdog.take() {
            let _ = watchdog.join();
        }
        self.failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .or(failure)
            .map_or(Ok(()), Err)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.request();
        let _ = self.join();
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TestRoute {
    after_seconds: f64,
    wav: Option<PathBuf>,
}

struct SourceFactory {
    track: Track,
    began: Instant,
    routes: Vec<TestRoute>,
}
impl SourceFactory {
    fn new(track: Track) -> Result<Self> {
        let routes = if let Some(raw) = std::env::var_os("FSCRIPT_MEETING_TEST_ROUTES") {
            let mut plan: std::collections::BTreeMap<String, Vec<TestRoute>> =
                serde_json::from_str(&raw.to_string_lossy())?;
            plan.remove(if track == Track::Microphone {
                "microphone"
            } else {
                "system"
            })
            .unwrap_or_default()
        } else {
            Vec::new()
        };
        for (index, route) in routes.iter().enumerate() {
            if !route.after_seconds.is_finite()
                || route.after_seconds < 0.0
                || (index == 0 && route.after_seconds != 0.0)
                || (index > 0 && route.after_seconds <= routes[index - 1].after_seconds)
            {
                bail!("test routes must have strictly increasing finite times starting at zero");
            }
        }
        Ok(Self {
            track,
            began: Instant::now(),
            routes,
        })
    }
    fn revision(&self) -> usize {
        self.routes
            .iter()
            .rposition(|route| route.after_seconds <= self.began.elapsed().as_secs_f64())
            .unwrap_or(0)
    }
    fn open(&self, stop: &Stop) -> Result<Generation> {
        let revision = self.revision();
        let source = if let Some(route) = self.routes.get(revision) {
            let path = route.wav.as_deref().ok_or_else(|| {
                crate::capture::RouteUnavailable("scripted device is unavailable".into())
            })?;
            AudioSource::fixture(path)?
        } else {
            match self.track {
                Track::Microphone => AudioSource::microphone()?,
                Track::System => AudioSource::system(&|| stop.requested())?,
            }
        };
        let normalizer = ClockedPcm::new(source.spec, stop.epoch)?;
        Ok(Generation {
            source,
            normalizer,
            revision,
            opened: Instant::now(),
        })
    }
}

struct Generation {
    source: AudioSource,
    normalizer: ClockedPcm,
    revision: usize,
    opened: Instant,
}
struct ClosedGeneration {
    first: Option<u64>,
    route_issue: Option<String>,
}

impl Generation {
    fn close(
        mut self,
        stop: &Stop,
        emit: impl FnMut(u64, &[i16]) -> Result<()> + Copy,
    ) -> Result<ClosedGeneration> {
        // Always attempt the old-format drain and tail flush, even if teardown
        // itself reports an error. Fatal cleanup still prevents successful save.
        let stopped = self.source.stop();
        let drained = self.source.drain(
            self.normalizer
                .receiver(|| stop.frames(crate::SAMPLE_RATE as u32), emit),
        );
        let first = self.normalizer.first_frame();
        let flushed = self
            .normalizer
            .finish_segment(Some(stop.observed_frame()?), emit);
        let mut route_issue = None;
        for error in [stopped.err(), drained.err(), flushed.err()]
            .into_iter()
            .flatten()
        {
            if !crate::capture::retryable(&error) {
                return Err(error);
            }
            route_issue = Some(format!("{error:#}"));
        }
        Ok(ClosedGeneration { first, route_issue })
    }
}

fn run_source(
    track: Track,
    writer: Arc<Mutex<Writer>>,
    stop: Arc<Stop>,
    ready: Arc<AtomicBool>,
    notices: mpsc::SyncSender<serde_json::Value>,
) -> Result<()> {
    let factory = SourceFactory::new(track)?;
    let emit = |at, samples: &[i16]| lock(&writer)?.append(track, at, samples);
    let notify = |state: &str, reason: Option<&str>| {
        // Status cannot backpressure audio. The durable manifest is authoritative.
        let _ = notices.try_send(serde_json::json!({"track":track,"state":state,"reason":reason}));
    };
    let mut generation = None;
    let mut interruption = None;
    let mut episode = None;
    let mut attempts = 0usize;
    let mut next_attempt = Instant::now();
    let mut next_probe = Instant::now();
    let mut checkpoint = Instant::now();
    let mut accepted = false;
    // An explicit fixture override shortens exhaustion tests only. Native runs
    // always use the production budget, even if this variable happens to exist.
    let budget = if !factory.routes.is_empty() {
        std::env::var("FSCRIPT_MEETING_TEST_RETRY_SECONDS")
            .ok()
            .map(|value| value.parse::<f64>())
            .transpose()?
            .filter(|seconds| seconds.is_finite() && *seconds > 0.0 && *seconds <= 15.0)
            .map(Duration::from_secs_f64)
            .unwrap_or(Duration::from_secs(15))
    } else {
        Duration::from_secs(15)
    };
    loop {
        if stop.requested() {
            break;
        }
        if generation.is_none() && Instant::now() >= next_attempt {
            if episode.is_some_and(|began: Instant| began.elapsed() >= budget) || attempts >= 8 {
                bail!("{track:?} could not reconnect within its bounded retry budget; saved audio is recoverable");
            }
            attempts += 1;
            match factory.open(&stop) {
                Ok(source) => {
                    generation = Some(source);
                }
                Err(error) => {
                    if stop.requested()
                        && (crate::capture::retryable(&error)
                            || error.is::<crate::capture::SetupCancelled>())
                    {
                        break;
                    }
                    if !crate::capture::retryable(&error) {
                        return Err(error);
                    }
                    episode.get_or_insert_with(Instant::now);
                    if interruption.is_none() {
                        let at = stop.observed_frame()?;
                        interruption = Some(lock(&writer)?.route_interrupted(
                            track,
                            at,
                            format!("{error:#}"),
                        )?);
                        notify("reconnecting", Some(&format!("{error:#}")));
                    }
                    next_attempt = Instant::now() + Duration::from_secs(1);
                }
            }
        }
        if let Some(current) = generation.as_mut() {
            let polled = current.source.poll(
                current
                    .normalizer
                    .receiver(|| stop.frames(crate::SAMPLE_RATE as u32), emit),
            );
            let route_changed = if Instant::now() >= next_probe {
                next_probe = Instant::now() + Duration::from_millis(250);
                if current.revision != factory.revision() {
                    Ok(true)
                } else {
                    current.source.route_changed()
                }
            } else {
                Ok(false)
            };
            if polled.is_ok()
                && route_changed.as_ref().is_ok_and(|changed| !changed)
                && !ready.swap(true, Ordering::AcqRel)
            {
                notify("ready", None);
            }
            if current.normalizer.first_frame().is_some() {
                accepted = true;
                if let Some(index) = interruption {
                    if lock(&writer)?
                        .try_resume_route(index, current.normalizer.first_frame().unwrap())?
                    {
                        interruption = None;
                        notify("receiving", None);
                    }
                }
            }
            // Transport health and durable coverage are independent. Idle output
            // can be stably reopened without supplying any packets yet.
            if polled.is_ok()
                && route_changed.as_ref().is_ok_and(|changed| !changed)
                && current.opened.elapsed() >= Duration::from_secs(2)
            {
                episode = None;
                attempts = 0;
            }
            let issue = match (polled, route_changed) {
                (Err(error), _) | (_, Err(error)) => Some(error),
                (Ok(()), Ok(true)) => Some(
                    crate::capture::RouteUnavailable(
                        "default audio device or hardware format changed".into(),
                    )
                    .into(),
                ),
                _ => None,
            };
            if let Some(error) = issue {
                let detected = stop.observed_frame()?;
                ready.store(false, Ordering::Release);
                let closed = generation.take().unwrap().close(&stop, emit)?;
                accepted |= closed.first.is_some();
                if let (Some(index), Some(first)) = (interruption, closed.first) {
                    if lock(&writer)?.try_resume_route(index, first)? {
                        interruption = None;
                    }
                }
                if !crate::capture::retryable(&error) {
                    return Err(error);
                }
                episode.get_or_insert_with(Instant::now);
                if interruption.is_none() {
                    interruption = Some(lock(&writer)?.route_interrupted(
                        track,
                        detected,
                        format!("{error:#}"),
                    )?);
                }
                notify("reconnecting", Some(&format!("{error:#}")));
                next_attempt = Instant::now() + Duration::from_millis(500);
            }
        }
        if checkpoint.elapsed() >= Duration::from_secs(1) {
            lock(&writer)?.checkpoint()?;
            checkpoint = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut had_endpoint = generation.is_some();
    if let Some(current) = generation {
        let closed = current.close(&stop, emit)?;
        if let Some(first) = closed.first {
            accepted = true;
            if let Some(index) = interruption {
                if lock(&writer)?.try_resume_route(index, first)? {
                    interruption = None;
                }
            }
        }
        if let Some(reason) = closed.route_issue {
            had_endpoint = false;
            if interruption.is_none() {
                let at = stop
                    .frames(crate::SAMPLE_RATE as u32)
                    .context("missing stop cutoff")?;
                interruption = Some(lock(&writer)?.route_interrupted(track, at, reason)?);
            }
        }
    }
    if !accepted {
        bail!("capture source supplied no accepted audio frames");
    }
    let cutoff = stop
        .frames(crate::SAMPLE_RATE as u32)
        .context("meeting stop timestamp missing")?;
    if let Some(index) = interruption {
        lock(&writer)?.resolve_route(
            index,
            cutoff,
            if had_endpoint {
                "no_post_route_audio"
            } else {
                "stopped_during_reconnect"
            },
        )?;
    }
    finish_track(&writer, track, cutoff)
}

fn finish_track(writer: &Mutex<Writer>, track: Track, cutoff: u64) -> Result<()> {
    let mut writer = lock(writer)?;
    let written = writer.frames(track);
    if written > cutoff {
        bail!("saved meeting audio extends beyond the stop cutoff; retain it as interrupted instead of claiming aligned tracks");
    }
    if cutoff > written {
        writer.append(track, cutoff - 1, &[0])?;
    }
    if writer.frames(track) != cutoff {
        bail!("meeting track did not reach the exact shared cutoff");
    }
    writer.checkpoint()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_during_generation_flush_cannot_publish_misaligned_tracks() {
        let temp = tempfile::tempdir().unwrap();
        let wav = temp.path().join("queued.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut file = hound::WavWriter::create(&wav, spec).unwrap();
        for _ in 0..16000 {
            file.write_sample(12000i16).unwrap();
        }
        file.finalize().unwrap();
        for late_publication in [false, true] {
            let directory = temp.path().join(format!("meeting-{late_publication}"));
            let writer = Mutex::new(Writer::create(&directory).unwrap());
            let stop = Stop::new().unwrap();
            let source = AudioSource::fixture(&wav).unwrap();
            let normalizer = ClockedPcm::new(source.spec, stop.epoch).unwrap();
            let mut generation = Generation {
                source,
                normalizer,
                revision: 0,
                opened: Instant::now(),
            };
            generation
                .source
                .poll(generation.normalizer.receiver(
                    || None,
                    |at, audio| lock(&writer)?.append(Track::Microphone, at, audio),
                ))
                .unwrap();
            // The first fixture packet contains audio beyond this moment. A
            // close must cap it, and a delayed cutoff publication must not make
            // an already longer prefix into a successful unequal-length save.
            std::thread::sleep(Duration::from_millis(3));
            if !late_publication {
                stop.request();
            }
            generation
                .close(&stop, |at, audio| {
                    if late_publication {
                        stop.cutoff.store(1_000_000, Ordering::SeqCst);
                    }
                    lock(&writer)?.append(Track::Microphone, at, audio)
                })
                .unwrap();
            let cutoff = stop.frames(16000).unwrap();
            let result = finish_track(&writer, Track::Microphone, cutoff);
            if late_publication {
                assert!(result.is_err());
                lock(&writer).unwrap().interrupt().unwrap();
                assert_eq!(
                    crate::meeting_store::inspect(&directory).unwrap().state,
                    crate::meeting_store::State::Interrupted
                );
            } else {
                result.unwrap();
                assert_eq!(lock(&writer).unwrap().frames(Track::Microphone), cutoff);
            }
            assert!(stop.observed_frame().unwrap() <= cutoff);
        }
    }

    #[test]
    fn stop_timestamp_is_shared_idempotent_and_not_reset_by_a_later_request() {
        let stop = Stop::new().unwrap();
        assert!(!stop.requested());
        stop.request();
        let first = stop.cutoff.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(2));
        stop.request();
        assert!(stop.requested());
        assert_eq!(stop.cutoff.load(Ordering::SeqCst), first);
        for rate in [8000, 16000, 48000] {
            assert_eq!(
                stop.frames(rate),
                Some((u128::from(first) * u128::from(rate) / 1_000_000_000) as u64)
            );
        }
    }
}
