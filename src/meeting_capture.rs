//! Continuous two-track capture. Inference never owns these threads or their queues.

use anyhow::{bail, Context, Result};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::capture::{host_time_ns, AudioPacket, AudioSource};
use crate::meeting_store::{Manifest, Track, Writer};
use crate::pcm_stream::Pcm16Decoder;

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
        let children = Arc::new(Mutex::new(Vec::<Weak<Mutex<Child>>>::new()));
        let done = Arc::new(AtomicBool::new(false));
        let watch_children = Arc::clone(&children);
        let watch_stop = Arc::clone(&stop);
        let watch_done = Arc::clone(&done);
        let watchdog = std::thread::spawn(move || {
            let mut stopping = None;
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
                if watch_stop.requested() {
                    let since = stopping.get_or_insert_with(Instant::now);
                    if since.elapsed() >= Duration::from_secs(2) {
                        for child in watch_children
                            .lock()
                            .unwrap()
                            .iter()
                            .filter_map(Weak::upgrade)
                        {
                            let _ = child.lock().unwrap().kill();
                        }
                    }
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
            let children = Arc::clone(&children);
            capture.workers.push(std::thread::spawn(move || {
                let result = run_source(track, writer, Arc::clone(&stop), ready, children);
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
    pub(crate) fn finish(mut self) -> Result<Manifest> {
        self.stop.request();
        self.join()?;
        // All capture/decoder readers have relinquished the store now.
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

/// Preserve one continuous resampling context per track. The reader writes
/// directly to disk: no output queue grows when recognition falls behind.
struct Normalizer {
    child: Arc<Mutex<Child>>,
    input: Option<ChildStdin>,
    reader: Option<JoinHandle<Result<()>>>,
    diagnostics: tempfile::NamedTempFile,
}

impl Normalizer {
    fn start(spec: hound::WavSpec, track: Track, writer: Arc<Mutex<Writer>>) -> Result<Self> {
        let diagnostics = tempfile::NamedTempFile::new()?;
        let mut child = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-xerror",
                "-probesize",
                "32",
                "-analyzeduration",
                "0",
                "-f",
                "s16le",
                "-ar",
                &spec.sample_rate.to_string(),
                "-ac",
                &spec.channels.to_string(),
                "-i",
                "pipe:0",
                "-ar",
                "16000",
                "-ac",
                "1",
                "-c:a",
                "pcm_s16le",
                "-f",
                "s16le",
                "-flush_packets",
                "1",
                "pipe:1",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(diagnostics.reopen()?))
            .spawn()
            .context("cannot start meeting normalization; install ffmpeg")?;
        let input = child.stdin.take();
        let mut output = child
            .stdout
            .take()
            .context("missing meeting decoder stdout")?;
        let reader = std::thread::spawn(move || {
            let mut pcm = Pcm16Decoder::default();
            let mut bytes = [0; 8192];
            let mut position = 0;
            loop {
                let size = output.read(&mut bytes)?;
                if size == 0 {
                    break;
                }
                let samples = pcm.push(&bytes[..size]);
                lock(&writer)?.append(track, position, &samples)?;
                position += samples.len() as u64;
            }
            pcm.finish()
        });
        Ok(Self {
            child: Arc::new(Mutex::new(child)),
            input,
            reader: Some(reader),
            diagnostics,
        })
    }

    fn write(&mut self, samples: &[i16]) -> Result<()> {
        let mut bytes = [0u8; 8192];
        for block in samples.chunks(bytes.len() / 2) {
            for (sample, pair) in block.iter().zip(bytes.chunks_exact_mut(2)) {
                pair.copy_from_slice(&sample.to_le_bytes());
            }
            self.input
                .as_mut()
                .context("meeting decoder closed")?
                .write_all(&bytes[..block.len() * 2])?;
        }
        Ok(())
    }

    fn silence(&mut self, mut samples: u64) -> Result<()> {
        let zeros = [0; 4096];
        while samples > 0 {
            let size = samples.min(zeros.len() as u64) as usize;
            self.write(&zeros[..size])?;
            samples -= size as u64;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<()> {
        self.input.take();
        let status = loop {
            if let Some(status) = self.child.lock().unwrap().try_wait()? {
                break status;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let read = self
            .reader
            .take()
            .unwrap()
            .join()
            .map_err(|_| anyhow::anyhow!("meeting decoder reader panicked"))?;
        read?;
        if !status.success() {
            bail!(
                "meeting normalization failed{}",
                crate::diagnostics::diagnostic_tail(Some(&self.diagnostics))
            );
        }
        Ok(())
    }
}

impl Drop for Normalizer {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.lock().unwrap().kill();
        let _ = self.child.lock().unwrap().wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

struct Timeline {
    rate: u32,
    channels: u16,
    epoch: u128,
    position: u64,
    first: bool,
}

impl Timeline {
    fn packet(
        &mut self,
        packet: AudioPacket,
        cutoff: Option<u64>,
        normalizer: &mut Normalizer,
    ) -> Result<()> {
        let channels = usize::from(self.channels);
        if packet.samples.len() % channels != 0 {
            bail!("incomplete meeting capture frame");
        }
        let at =
            packet.host_time_ns.saturating_sub(self.epoch) * u128::from(self.rate) / 1_000_000_000;
        let at = u64::try_from(at).context("meeting capture clock overflow")?;
        let before_epoch =
            self.epoch.saturating_sub(packet.host_time_ns) * u128::from(self.rate) / 1_000_000_000;
        let skip = usize::try_from(before_epoch)
            .context("meeting capture clock overflow")?
            .min(packet.samples.len() / channels);
        let samples = &packet.samples[skip * channels..];
        if samples.is_empty() || cutoff.is_some_and(|end| at.max(self.position) >= end) {
            return Ok(());
        }
        // Compare against the cumulative frame position, never re-anchor on each
        // callback. Small timestamp jitter does not create holes in speech.
        let tolerance = u64::from(self.rate) / 10;
        if self.first || at > self.position + tolerance {
            let target = cutoff.map_or(at, |end| at.min(end));
            if target > self.position {
                normalizer.silence((target - self.position) * u64::from(self.channels))?;
                self.position = target;
            }
        }
        if at + tolerance < self.position {
            bail!("meeting capture clock moved backwards; saved audio is recoverable");
        }
        let frames = (samples.len() / channels) as u64;
        let frames = cutoff.map_or(frames, |end| frames.min(end.saturating_sub(self.position)));
        if frames == 0 {
            return Ok(());
        }
        normalizer.write(&samples[..frames as usize * channels])?;
        self.first = false;
        self.position += frames;
        Ok(())
    }
}

fn run_source(
    track: Track,
    writer: Arc<Mutex<Writer>>,
    stop: Arc<Stop>,
    ready: Arc<AtomicBool>,
    children: Arc<Mutex<Vec<Weak<Mutex<Child>>>>>,
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
    let mut normalizer = Normalizer::start(source.spec, track, Arc::clone(&writer))?;
    children
        .lock()
        .unwrap()
        .push(Arc::downgrade(&normalizer.child));
    let mut timeline = Timeline {
        rate: source.spec.sample_rate,
        channels: source.spec.channels,
        epoch: stop.epoch,
        position: 0,
        first: true,
    };
    ready.store(true, Ordering::Release);
    let mut checkpoint = Instant::now();
    loop {
        if stop.requested() {
            break;
        }
        source.poll(|packet| timeline.packet(packet, None, &mut normalizer))?;
        if checkpoint.elapsed() >= Duration::from_secs(1) {
            lock(&writer)?.checkpoint()?;
            checkpoint = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    source.stop()?;
    source.poll(|packet| timeline.packet(packet, stop.frames(timeline.rate), &mut normalizer))?;
    if timeline.first {
        bail!("{track:?} source supplied no audio packets; refusing to mark an empty capture successful");
    }
    if let Some(end) = stop.frames(timeline.rate) {
        if end > timeline.position {
            normalizer.silence((end - timeline.position) * u64::from(timeline.channels))?;
        }
    }
    normalizer.finish()?;
    lock(&writer)?.checkpoint()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragmented_continuous_conversion_matches_independent_ffmpeg_for_native_rates() {
        for rate in [8000, 11025, 16000, 44100, 48000] {
            for channels in [1, 2] {
                let root = tempfile::tempdir().unwrap();
                let directory = root.path().join("meeting");
                let writer = Arc::new(Mutex::new(Writer::create(&directory).unwrap()));
                let spec = hound::WavSpec {
                    channels,
                    sample_rate: rate,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                };
                let samples: Vec<_> = (0..rate as usize / 3 * channels as usize)
                    .map(|i| (i.wrapping_mul(33191)) as i16)
                    .collect();
                let raw = root.path().join("native.pcm");
                std::fs::write(
                    &raw,
                    samples
                        .iter()
                        .flat_map(|value| value.to_le_bytes())
                        .collect::<Vec<_>>(),
                )
                .unwrap();
                let reference = Command::new("ffmpeg")
                    .args([
                        "-v",
                        "error",
                        "-f",
                        "s16le",
                        "-ar",
                        &rate.to_string(),
                        "-ac",
                        &channels.to_string(),
                        "-i",
                    ])
                    .arg(&raw)
                    .args([
                        "-ar",
                        "16000",
                        "-ac",
                        "1",
                        "-c:a",
                        "pcm_s16le",
                        "-f",
                        "s16le",
                        "pipe:1",
                    ])
                    .output()
                    .unwrap();
                assert!(reference.status.success());
                let mut normalizer =
                    Normalizer::start(spec, Track::System, Arc::clone(&writer)).unwrap();
                let mut at = 0;
                let mut random = 19u64;
                while at < samples.len() {
                    random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                    let end =
                        (at + (random as usize % 719 + 1) * channels as usize).min(samples.len());
                    normalizer.write(&samples[at..end]).unwrap();
                    at = end;
                }
                normalizer.finish().unwrap();
                assert_eq!(
                    std::fs::read(directory.join("system.pcm")).unwrap(),
                    reference.stdout,
                    "rate {rate}, channels {channels}"
                );
                assert_eq!(lock(&writer).unwrap().frames(Track::Microphone), 0);
            }
        }
    }

    #[test]
    fn timeline_preserves_jittered_packets_and_fills_a_real_gap_before_cutoff() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("meeting");
        let writer = Arc::new(Mutex::new(Writer::create(&directory).unwrap()));
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut normalizer = Normalizer::start(spec, Track::Microphone, writer).unwrap();
        let epoch = 10_000_000_000;
        let mut timeline = Timeline {
            rate: 16000,
            channels: 1,
            epoch,
            position: 0,
            first: true,
        };
        let mut reference = vec![0; 1600];
        for i in 0..64u64 {
            let value = i as i16 + 1;
            let at = 1600 + i * 320;
            // Fixed initial anchor with alternating ±1ms callback jitter.
            let jitter = if i == 0 {
                0
            } else if i % 2 == 0 {
                1_000_000
            } else {
                -1_000_000
            };
            timeline
                .packet(
                    AudioPacket {
                        samples: vec![value; 320],
                        host_time_ns: (epoch as i128 + at as i128 * 62500 + jitter) as u128,
                    },
                    None,
                    &mut normalizer,
                )
                .unwrap();
            reference.extend([value; 320]);
        }
        let gap = 3200;
        let at = timeline.position + gap;
        timeline
            .packet(
                AudioPacket {
                    samples: vec![1000; 320],
                    host_time_ns: epoch + at as u128 * 62500,
                },
                Some(at + 160),
                &mut normalizer,
            )
            .unwrap();
        reference.extend(vec![0; gap as usize]);
        reference.extend([1000; 160]);
        normalizer.finish().unwrap();
        let expected: Vec<_> = reference
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        assert_eq!(
            std::fs::read(directory.join("microphone.pcm")).unwrap(),
            expected
        );
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
