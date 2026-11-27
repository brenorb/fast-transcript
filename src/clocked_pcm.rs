//! Band-limited, continuously clocked meeting PCM. All positions use host time.

use anyhow::{bail, Context, Result};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
    calculate_cutoff, Adjustable, Async, FixedAsync, Indexing, Resampler,
    SincInterpolationParameters, WindowFunction,
};
use std::collections::VecDeque;

use crate::capture::AudioPacket;

const RATE: f64 = crate::SAMPLE_RATE as f64;
const BLOCK: usize = 1024;
const BASE_SINC: usize = 256;
const SINC_PHASES: usize = 128;
const HOLD: usize = crate::SAMPLE_RATE / 50;
const MAX_CORRECTION: f64 = 0.005;
const GAP_SECONDS: f64 = 0.1;

/// A phase/frequency loop referenced to cumulative input frames, not callback
/// arrival time. Frequency integration removes steady independent-clock skew;
/// low-pass phase feedback rejects timestamp jitter. Ratios change continuously.
struct Clock {
    position: f64,
    previous: f64,
    previous_frames: usize,
    filtered_error: f64,
    frequency: f64,
    relative: f64,
}

impl Clock {
    fn new(at: f64) -> Self {
        Self {
            position: at,
            previous: at,
            previous_frames: 0,
            filtered_error: 0.0,
            frequency: 0.0,
            relative: 1.0,
        }
    }

    fn observe(&mut self, at: f64, nominal: f64) -> Result<bool> {
        let dt = self.previous_frames as f64 * nominal / RATE;
        let jump = (at - self.previous) / RATE - dt * self.relative;
        if jump < -GAP_SECONDS {
            bail!("meeting capture clock moved backwards; saved audio is recoverable");
        }
        if jump > GAP_SECONDS {
            return Ok(true);
        }
        if dt > 0.0 {
            let error = (at - self.position) / RATE;
            if error.abs() > GAP_SECONDS {
                bail!("capture clock drift exceeds the supported correction window; saved audio is recoverable");
            }
            self.filtered_error += dt / (0.5 + dt) * (error - self.filtered_error);
            self.frequency = (self.frequency + 0.01 * self.filtered_error * dt)
                .clamp(-MAX_CORRECTION, MAX_CORRECTION);
            let target = (1.0 + self.frequency + 0.2 * self.filtered_error)
                .clamp(1.0 - MAX_CORRECTION, 1.0 + MAX_CORRECTION);
            // A hard derivative limit winds the integrator up on phase steps.
            // Smooth the actuator; Rubato also ramps each processing block.
            self.relative += dt / (1.0 + dt) * (target - self.relative);
        }
        self.previous = at;
        Ok(false)
    }

    fn advance(&mut self, frames: usize, nominal: f64) {
        self.position += frames as f64 * nominal * self.relative;
        self.previous_frames = frames;
    }
}

pub(crate) struct ClockedPcm {
    epoch: u128,
    channels: usize,
    nominal: f64,
    converter: Async<f32>,
    input: Vec<f32>,
    output: Vec<f32>,
    pending: usize,
    delay: usize,
    preroll: usize,
    startup_trim: usize,
    clock: Option<Clock>,
    queue: VecDeque<i16>,
    queue_start: u64,
    accepted: u64,
}

impl ClockedPcm {
    pub(crate) fn new(spec: hound::WavSpec, epoch: u128) -> Result<Self> {
        if !(8000..=384000).contains(&spec.sample_rate)
            || !(1..=64).contains(&spec.channels)
            || spec.bits_per_sample != 16
            || spec.sample_format != hound::SampleFormat::Int
        {
            bail!("meeting capture requires 8–384 kHz audio with 1–64 channels");
        }
        let nominal = RATE / spec.sample_rate as f64;
        // Preserve the physical transition width when decimating high-rate audio.
        let length = BASE_SINC * spec.sample_rate.div_ceil(48000) as usize;
        let window = WindowFunction::BlackmanHarris2;
        let parameters = SincInterpolationParameters::new(length, window)
            .oversampling_factor(SINC_PHASES)
            .f_cutoff(calculate_cutoff::<f32>(BASE_SINC, window) * (1.0 - MAX_CORRECTION) as f32);
        let converter = Async::new_sinc(nominal, 1.01, &parameters, BLOCK, 1, FixedAsync::Input)?;
        // Rubato's integer output_delay describes a different initial sampling
        // grid. Synthetic input preroll puts the sinc center on our output grid
        // before cropping. Remaining table quantization is less than 1us.
        let mut a = spec.sample_rate;
        let mut b = crate::SAMPLE_RATE as u32;
        while b != 0 {
            (a, b) = (b, a % b);
        }
        let period = spec.sample_rate as usize / a as usize;
        let preroll = (period - length / 2 % period) % period;
        let startup_trim =
            (((preroll + length / 2) as f64 * nominal).round() as usize).saturating_sub(1);
        let capacity = converter.output_frames_max();
        Ok(Self {
            epoch,
            channels: spec.channels as usize,
            nominal,
            converter,
            input: vec![0.0; BLOCK],
            output: vec![0.0; capacity],
            pending: 0,
            delay: startup_trim,
            preroll,
            startup_trim,
            clock: None,
            queue: VecDeque::new(),
            queue_start: 0,
            accepted: 0,
        })
    }

    /// Read the shared cutoff for every delivered packet, including packets that
    /// arrive while a native queue is being drained into a slow disk sink.
    pub(crate) fn receiver<'a>(
        &'a mut self,
        cutoff: impl Fn() -> Option<u64> + 'a,
        mut emit: impl FnMut(u64, &[i16]) -> Result<()> + 'a,
    ) -> impl FnMut(AudioPacket) -> Result<()> + 'a {
        move |packet| self.push(packet, cutoff(), &mut emit)
    }

    fn push(
        &mut self,
        packet: AudioPacket,
        cutoff: Option<u64>,
        mut emit: impl FnMut(u64, &[i16]) -> Result<()>,
    ) -> Result<()> {
        if packet.samples.len() % self.channels != 0 {
            bail!("incomplete meeting capture frame");
        }
        if packet.samples.is_empty() {
            return Ok(());
        }
        let relative = self.clock.as_ref().map_or(1.0, |clock| clock.relative);
        let step = self.nominal * relative;
        let mut at = if packet.host_time_ns >= self.epoch {
            (packet.host_time_ns - self.epoch) as f64 * RATE / 1e9
        } else {
            -((self.epoch - packet.host_time_ns) as f64 * RATE / 1e9)
        };
        let skip = if at < 0.0 {
            (-at / step).ceil() as usize
        } else {
            0
        };
        let frames = packet.samples.len() / self.channels;
        if skip >= frames {
            return Ok(());
        }
        at += skip as f64 * step;
        if cutoff.is_some_and(|end| at >= end as f64) {
            return Ok(());
        }
        let count = cutoff
            .map_or(frames - skip, |end| {
                ((end as f64 - at) / step).ceil().max(0.0) as usize
            })
            .min(frames - skip);
        if count == 0 {
            return Ok(());
        }

        let gap = self
            .clock
            .as_mut()
            .map(|clock| clock.observe(at, self.nominal))
            .transpose()?
            .unwrap_or(false);
        if gap {
            let end = self.clock.as_ref().unwrap().position.round().max(0.0) as u64;
            self.flush(end, &mut emit)?;
            self.clock = None;
        }
        if self.clock.is_none() {
            self.converter.reset();
            self.pending = 0;
            self.delay = self.startup_trim;
            self.input.fill(0.0);
            self.queue.clear();
            for _ in 0..self.preroll / BLOCK {
                self.process(None)?;
            }
            self.pending = self.preroll % BLOCK;
            self.queue_start = at.round().max(0.0) as u64;
            self.clock = Some(Clock::new(at));
        }
        let relative = self.clock.as_ref().unwrap().relative;
        self.converter.set_resample_ratio_relative(relative, true)?;
        for frame in packet.samples[skip * self.channels..(skip + count) * self.channels]
            .chunks_exact(self.channels)
        {
            let mono = frame.iter().map(|value| *value as f64).sum::<f64>() / self.channels as f64;
            self.input[self.pending] = mono as f32 / i16::MAX as f32;
            self.pending += 1;
            if self.pending == BLOCK {
                self.process(None)?;
                self.publish(HOLD, &mut emit)?;
            }
        }
        self.clock.as_mut().unwrap().advance(count, self.nominal);
        self.accepted += count as u64;
        Ok(())
    }

    fn process(&mut self, partial: Option<usize>) -> Result<()> {
        let input = InterleavedSlice::new(&self.input, 1, BLOCK)?;
        let capacity = self.output.len();
        let mut output = InterleavedSlice::new_mut(&mut self.output, 1, capacity)?;
        let indexing = partial.map(|count| Indexing::new().partial_len(count));
        let (_, length) =
            self.converter
                .process_into_buffer(&input, &mut output, indexing.as_ref())?;
        let skip = self.delay.min(length);
        self.delay -= skip;
        self.queue
            .extend(self.output[skip..length].iter().map(|sample| {
                (sample * i16::MAX as f32)
                    .round()
                    .clamp(i16::MIN as f32, i16::MAX as f32) as i16
            }));
        self.pending = 0;
        Ok(())
    }

    fn publish(
        &mut self,
        retain: usize,
        emit: &mut impl FnMut(u64, &[i16]) -> Result<()>,
    ) -> Result<()> {
        let count = self.queue.len().saturating_sub(retain);
        if count > 0 {
            let (first, second) = self.queue.as_slices();
            let take = count.min(first.len());
            emit(self.queue_start, &first[..take])?;
            if count > take {
                emit(self.queue_start + take as u64, &second[..count - take])?;
            }
            self.queue.drain(..count);
            self.queue_start += count as u64;
        }
        Ok(())
    }

    fn flush(&mut self, end: u64, emit: &mut impl FnMut(u64, &[i16]) -> Result<()>) -> Result<()> {
        if end < self.queue_start {
            bail!("meeting cutoff precedes already saved audio");
        }
        if self.pending > 0 {
            self.process(Some(self.pending))?;
        }
        let maximum_tail = self.startup_trim + self.output.len() * 2;
        let mut padded = 0;
        while self.queue_start + (self.queue.len() as u64) < end {
            if padded >= maximum_tail {
                bail!("meeting clock/resampler tail exceeds its bounded history");
            }
            self.process(Some(0))?;
            padded += self.output.len();
        }
        self.queue
            .truncate(usize::try_from(end - self.queue_start).context("meeting clock overflow")?);
        self.publish(0, emit)
    }

    pub(crate) fn finish(
        mut self,
        cutoff: u64,
        mut emit: impl FnMut(u64, &[i16]) -> Result<()>,
    ) -> Result<()> {
        if self.accepted == 0 {
            bail!("capture source supplied no accepted audio frames");
        }
        let end = (self.clock.as_ref().unwrap().position.round().max(0.0) as u64).min(cutoff);
        self.flush(end, &mut emit)?;
        // Use the sink's gap-fill semantics; do not allocate the silent tail.
        if cutoff > end {
            emit(cutoff - 1, &[0])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn spec(rate: u32, channels: u16) -> hound::WavSpec {
        hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        }
    }

    fn collect(audio: &mut Vec<i16>, at: u64, samples: &[i16]) -> Result<()> {
        let at = at as usize;
        assert!(
            at >= audio.len(),
            "immutable audio prefix cannot be overwritten"
        );
        audio.resize(at, 0);
        audio.extend_from_slice(samples);
        Ok(())
    }

    #[test]
    fn property_hour_clocks_remain_aligned_with_jitter_and_changing_skew() {
        for seed in 0..32u64 {
            let mut random = seed + 1;
            let mut clock = Clock::new(0.0);
            let rate = 48000.0;
            let nominal = RATE / rate;
            let mut native = 0u64;
            let initial = (seed as f64 - 16.0) * 0.000025;
            let slope = -initial / 3600.0;
            while native < (3600.0 * rate) as u64 {
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                let frames =
                    (random as usize % 1024 + 256).min((3600.0 * rate) as usize - native as usize);
                let x = native as f64 / rate;
                let actual = (x * (1.0 + initial) + 0.5 * slope * x * x) * RATE;
                let jitter = if native == 0 {
                    0.0
                } else {
                    ((random >> 32) as i32 as f64 / i32::MAX as f64) * 0.0005 * RATE
                };
                assert!(!clock.observe(actual + jitter, nominal).unwrap());
                assert!(
                    (clock.position - actual).abs() < RATE * 0.003,
                    "seed {seed}, t={x}, error={}ms",
                    (clock.position - actual) * 1000.0 / RATE
                );
                clock.advance(frames, nominal);
                native += frames as u64;
            }
            assert!(
                (clock.relative - 1.0).abs() < 0.0001,
                "seed {seed}: {}",
                clock.relative
            );
        }
    }

    #[test]
    fn positive_and_negative_timestamp_steps_settle_without_integrator_oscillation() {
        for step in [-0.08, -0.01, 0.01, 0.08] {
            let mut clock = Clock::new(0.0);
            let mut maximum = 0.0f64;
            for packet in 0..10000 {
                let time = packet as f64 * 0.02;
                let wanted = (time + if time >= 1.0 { step } else { 0.0 }) * RATE;
                assert!(!clock.observe(wanted, 1.0).unwrap());
                let error = (clock.position - wanted).abs() / RATE;
                maximum = maximum.max(error);
                assert!(
                    error <= step.abs() + 0.0001,
                    "step {step}, t={time}, phase error {error}"
                );
                if time >= 120.0 {
                    assert!(error < 0.00005, "step {step}, t={time}, residual {error}");
                }
                clock.advance(320, 1.0);
            }
            assert!(maximum >= step.abs() * 0.99);
        }
    }

    #[test]
    fn untrackable_clock_is_rejected_instead_of_silently_diverging() {
        let mut clock = Clock::new(0.0);
        let mut failed = false;
        for packet in 0..1000 {
            if clock
                .observe(packet as f64 * 0.02 * 1.02 * RATE, 1.0)
                .is_err()
            {
                failed = true;
                break;
            }
            clock.advance(320, 1.0);
        }
        assert!(
            failed,
            "a 2% clock error exceeds the ±0.5% correction range"
        );
    }

    #[test]
    fn constant_rate_sines_match_independent_ffmpeg_and_preserve_fragment_boundaries() {
        for rate in [8000, 11025, 16000, 44100, 48000, 96000, 192000, 384000] {
            for channels in [1, 2] {
                let root = tempfile::tempdir().unwrap();
                let frames = rate as usize / 3;
                let samples: Vec<i16> = (0..frames)
                    .flat_map(|i| {
                        let value = (10000.0
                            * (std::f64::consts::TAU * 300.0 * i as f64 / rate as f64).sin())
                        .round() as i16;
                        std::iter::repeat_n(value, channels as usize)
                    })
                    .collect();
                let raw = root.path().join("native.pcm");
                std::fs::write(
                    &raw,
                    samples
                        .iter()
                        .flat_map(|v| v.to_le_bytes())
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
                    .arg(raw)
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
                let expected: Vec<_> = reference
                    .stdout
                    .chunks_exact(2)
                    .map(|v| i16::from_le_bytes([v[0], v[1]]))
                    .collect();
                let mut converter = ClockedPcm::new(spec(rate, channels), 0).unwrap();
                let mut output = Vec::new();
                let mut at = 0;
                let mut random = 39u64;
                while at < frames {
                    random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                    let end = (at + (random as usize % 1493 + 1)).min(frames);
                    converter
                        .push(
                            AudioPacket {
                                host_time_ns: at as u128 * 1_000_000_000 / rate as u128,
                                samples: samples[at * channels as usize..end * channels as usize]
                                    .to_vec(),
                            },
                            None,
                            |at, samples| collect(&mut output, at, samples),
                        )
                        .unwrap();
                    assert!(converter.pending < BLOCK);
                    assert!(converter.queue.len() <= HOLD);
                    at = end;
                }
                let count = (frames as f64 * RATE / rate as f64).round() as u64;
                converter
                    .finish(count, |at, samples| collect(&mut output, at, samples))
                    .unwrap();
                assert_eq!(output.len(), count as usize);
                let mut squared_error = 0.0;
                let mut energy = 0.0;
                for (actual, expected) in output[256..output.len() - 256]
                    .iter()
                    .zip(&expected[256..expected.len() - 256])
                {
                    squared_error += (*actual as f64 - *expected as f64).powi(2);
                    energy += (*expected as f64).powi(2);
                }
                let snr = 10.0 * (energy / squared_error).log10();
                assert!(snr > 45.0, "rate {rate}, channels {channels}, SNR {snr}dB");
            }
        }
    }

    #[test]
    fn downsampling_rejects_above_nyquist_audio_instead_of_aliasing_it() {
        for (rate, frequency) in [
            (48000, 12000.0),
            (96000, 9000.0),
            (192000, 9000.0),
            (384000, 9000.0),
        ] {
            let mut converter = ClockedPcm::new(spec(rate, 1), 0).unwrap();
            let mut output = Vec::new();
            for packet in 0..50 {
                let length = rate / 50;
                let start = packet * length;
                let samples = (start..start + length)
                    .map(|i| {
                        (15000.0
                            * (std::f64::consts::TAU * frequency * i as f64 / rate as f64).sin())
                        .round() as i16
                    })
                    .collect();
                converter
                    .push(
                        AudioPacket {
                            samples,
                            host_time_ns: start as u128 * 1_000_000_000 / rate as u128,
                        },
                        None,
                        |at, samples| collect(&mut output, at, samples),
                    )
                    .unwrap();
            }
            converter
                .finish(16000, |at, samples| collect(&mut output, at, samples))
                .unwrap();
            let rms = (output[256..output.len() - 256]
                .iter()
                .map(|v| (*v as f64).powi(2))
                .sum::<f64>()
                / (output.len() - 512) as f64)
                .sqrt();
            assert!(rms < 2.0, "rate {rate}, aliased signal RMS {rms}");
        }
    }

    #[test]
    fn empty_clipped_sources_invalid_frames_and_sink_failure_cannot_succeed() {
        let mut empty = ClockedPcm::new(spec(16000, 1), 1_000_000_000).unwrap();
        empty
            .push(
                AudioPacket {
                    samples: vec![100; 160],
                    host_time_ns: 0,
                },
                Some(160),
                |_, _| panic!("entire packet is before epoch"),
            )
            .unwrap();
        empty
            .push(
                AudioPacket {
                    samples: vec![100; 160],
                    host_time_ns: 2_000_000_000,
                },
                Some(160),
                |_, _| panic!("entire packet is after cutoff"),
            )
            .unwrap();
        assert!(empty
            .finish(160, |_, _| panic!("empty input must not produce silence"))
            .is_err());
        let mut stereo = ClockedPcm::new(spec(48000, 2), 0).unwrap();
        assert!(stereo
            .push(
                AudioPacket {
                    samples: vec![1],
                    host_time_ns: 0
                },
                None,
                |_, _| Ok(())
            )
            .is_err());
        let mut failed = ClockedPcm::new(spec(16000, 1), 0).unwrap();
        assert!(failed
            .push(
                AudioPacket {
                    samples: vec![100; 4096],
                    host_time_ns: 0
                },
                None,
                |_, _| bail!("disk full")
            )
            .is_err());
    }

    #[test]
    fn queue_drain_observes_a_stop_requested_by_an_in_progress_sink() {
        use std::cell::{Cell, RefCell};
        let mut converter = ClockedPcm::new(spec(16000, 1), 0).unwrap();
        let cutoff = Cell::new(None);
        let output = RefCell::new(Vec::new());
        {
            let mut receive = converter.receiver(
                || cutoff.get(),
                |at, samples| {
                    collect(&mut output.borrow_mut(), at, samples)?;
                    cutoff.set(Some(1280));
                    Ok(())
                },
            );
            // Native polling may still drain up to 128 packets after the first
            // disk write. Those after the newly selected cutoff must be ignored.
            for index in 0..128 {
                receive(AudioPacket {
                    samples: vec![1000; 320],
                    host_time_ns: index * 20_000_000,
                })
                .unwrap();
            }
        }
        assert_eq!(cutoff.get(), Some(1280));
        converter
            .finish(1280, |at, samples| {
                collect(&mut output.borrow_mut(), at, samples)
            })
            .unwrap();
        assert_eq!(output.borrow().len(), 1280);
    }

    #[test]
    fn genuine_gaps_remain_silent_and_backwards_discontinuity_is_rejected() {
        let mut converter = ClockedPcm::new(spec(16000, 1), 0).unwrap();
        let mut output = Vec::new();
        converter
            .push(
                AudioPacket {
                    samples: vec![1000; 4096],
                    host_time_ns: 0,
                },
                None,
                |at, samples| collect(&mut output, at, samples),
            )
            .unwrap();
        converter
            .push(
                AudioPacket {
                    samples: vec![2000; 3200],
                    host_time_ns: 700_000_000,
                },
                None,
                |at, samples| collect(&mut output, at, samples),
            )
            .unwrap();
        let saved = output.clone();
        assert!(converter
            .push(
                AudioPacket {
                    samples: vec![3000; 320],
                    host_time_ns: 400_000_000
                },
                None,
                |_, _| panic!("backwards source must not append")
            )
            .is_err());
        assert_eq!(output, saved);
        converter
            .finish(14400, |at, samples| collect(&mut output, at, samples))
            .unwrap();
        assert_eq!(output.len(), 14400);
        assert!(output[4096..11200].iter().all(|&value| value == 0));
        assert!((output[800] - 1000).abs() <= 1);
        assert!((output[12800] - 2000).abs() <= 1);
    }

    /// Actual 16 kHz output, bounded marker probes, no whole-recording buffer.
    /// Run explicitly in release mode; default unit tests cover the same clock
    /// loop over 32 randomized hour-long clocks without the expensive DSP pass.
    #[test]
    #[ignore = "full hour DSP acceptance; run cargo test --release simulated_hour_markers"]
    fn simulated_hour_markers_stay_aligned_across_two_clocks_and_a_real_gap() {
        for (rate, initial, slope) in [(16000u32, 0.00015, -0.0003 / 3600.0), (44100, -0.0001, 0.0)]
        {
            let mut converter = ClockedPcm::new(spec(rate, 1), 0).unwrap();
            let mut native = 0u64;
            let mut random = 19u64;
            let mut end = 0u64;
            let mut peaks = [(0i16, 0u64); 60];
            let host = |frame: u64| {
                let x = frame as f64 / rate as f64;
                x * (1.0 + initial) + 0.5 * slope * x * x
            };
            let mut emit = |at: u64, samples: &[i16]| -> Result<()> {
                assert!(at >= end);
                for (offset, &sample) in samples.iter().enumerate() {
                    let frame = at + offset as u64;
                    let seconds = frame as f64 / RATE;
                    let marker = ((seconds - 10.0) / 60.0).round() as i64;
                    if (0..60).contains(&marker)
                        && (seconds - (10.0 + marker as f64 * 60.0)).abs() < 0.02
                    {
                        let peak = &mut peaks[marker as usize];
                        if sample > peak.0 {
                            *peak = (sample, frame);
                        }
                    }
                    if (1800.05..1800.30).contains(&seconds) {
                        assert_eq!(sample, 0, "a genuine gap must remain silence");
                    }
                }
                end = at + samples.len() as u64;
                Ok(())
            };
            while host(native) < 3600.0 {
                if (1800.0..1800.35).contains(&host(native)) {
                    native += 1;
                    continue;
                }
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                let frames = random as usize % 1024 + 256;
                let start = host(native);
                let mut samples = Vec::with_capacity(frames);
                for _ in 0..frames {
                    let time = host(native);
                    if time >= 3600.0 || (1800.0..1800.35).contains(&time) {
                        break;
                    }
                    let marker = ((time - 10.0) / 60.0).round();
                    let distance = time - (10.0 + marker * 60.0);
                    samples.push(if distance.abs() < 0.006 {
                        (30000.0 * (-0.5 * (distance / 0.001).powi(2)).exp()).round() as i16
                    } else {
                        0
                    });
                    native += 1;
                }
                let jitter = if start == 0.0 {
                    0.0
                } else {
                    ((random >> 32) as i32 as f64 / i32::MAX as f64) * 0.0005
                };
                converter
                    .push(
                        AudioPacket {
                            samples,
                            host_time_ns: ((start + jitter).max(0.0) * 1e9).round() as u128,
                        },
                        None,
                        &mut emit,
                    )
                    .unwrap();
                assert!(converter.pending < BLOCK);
                assert!(converter.queue.len() <= HOLD);
            }
            converter.finish((3600.0 * RATE) as u64, &mut emit).unwrap();
            assert_eq!(end, (3600.0 * RATE) as u64);
            for (index, (amplitude, frame)) in peaks.into_iter().enumerate() {
                let wanted = (10.0 + index as f64 * 60.0) * RATE;
                let error_ms = (frame as f64 - wanted).abs() * 1000.0 / RATE;
                assert!(
                    amplitude > 25000,
                    "missing interior marker {index} at {rate}Hz: {amplitude}"
                );
                assert!(
                    error_ms < 3.0,
                    "rate {rate}, marker {index}: error {error_ms}ms"
                );
            }
        }
    }
}
