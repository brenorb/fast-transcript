//! Continuous two-track capture. Inference never owns these threads or their queues.

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
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
        let mut capture = Self {
            writer,
            stop,
            ready,
            workers: Vec::new(),
            done,
            watchdog: Some(watchdog),
        };
        for (index, track) in Track::ALL.into_iter().enumerate() {
            let writer = Arc::clone(&capture.writer);
            let stop = Arc::clone(&capture.stop);
            let ready = Arc::clone(&capture.ready[index]);
            capture.workers.push(std::thread::spawn(move || {
                let result = run_source(track, writer, Arc::clone(&stop), ready);
                if result.is_err() {
                    stop.request();
                }
                result
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
        self.join()?;
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
        failure.map_or(Ok(()), Err)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.request();
        let _ = self.join();
    }
}

fn run_source(
    track: Track,
    writer: Arc<Mutex<Writer>>,
    stop: Arc<Stop>,
    ready: Arc<AtomicBool>,
) -> Result<()> {
    if stop.requested() {
        bail!("meeting stopped before {track:?} source acquisition");
    }
    let mut source = match track {
        Track::Microphone => AudioSource::microphone(),
        Track::System => AudioSource::system(),
    }?;
    if stop.requested() {
        source.stop()?;
        bail!("meeting stopped before {track:?} source became ready");
    }
    let mut normalizer = ClockedPcm::new(source.spec, stop.epoch)?;
    let emit = |at, samples: &[i16]| lock(&writer)?.append(track, at, samples);
    ready.store(true, Ordering::Release);
    let mut checkpoint = Instant::now();
    loop {
        if stop.requested() {
            break;
        }
        source.poll(normalizer.receiver(|| stop.frames(crate::SAMPLE_RATE as u32), emit))?;
        if checkpoint.elapsed() >= Duration::from_secs(1) {
            lock(&writer)?.checkpoint()?;
            checkpoint = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    source.stop()?;
    let cutoff = stop
        .frames(crate::SAMPLE_RATE as u32)
        .context("meeting stop timestamp missing")?;
    source.poll(normalizer.receiver(|| Some(cutoff), emit))?;
    normalizer.finish(cutoff, emit)?;
    lock(&writer)?.checkpoint()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
