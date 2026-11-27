//! Microphone transport. Callbacks never block on disk or silently drop audio.

use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) struct Capture {
    source: AudioSource,
    writer: hound::WavWriter<BufWriter<File>>,
    samples: usize,
    channels: u16,
}

/// Native-format PCM plus capture time on the shared host clock. On macOS CPAL
/// and Core Audio taps both use mach_absolute_time converted to nanoseconds.
pub(crate) struct AudioPacket {
    pub(crate) samples: Vec<i16>,
    pub(crate) host_time_ns: u128,
}

pub(crate) struct AudioSource {
    source: Source,
    pub(crate) spec: hound::WavSpec,
    cutoff: Option<Instant>,
}

enum Source {
    #[cfg(target_os = "macos")]
    Microphone(NativeCapture),
    Fixture {
        reader: hound::WavReader<BufReader<File>>,
        next: Instant,
        packet_samples: usize,
        epoch_ns: u128,
        delivered_samples: u64,
    },
}

pub(crate) fn host_time_ns() -> Result<u128> {
    #[cfg(target_os = "macos")]
    {
        // CPAL's Core Audio capture timestamps use this same clock/timebase.
        host_ticks_to_ns(unsafe { mach2::mach_time::mach_absolute_time() })
    }
    #[cfg(not(target_os = "macos"))]
    {
        static ORIGIN: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        Ok(ORIGIN.get_or_init(Instant::now).elapsed().as_nanos())
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn host_ticks_to_ns(ticks: u64) -> Result<u128> {
    static TIMEBASE: std::sync::OnceLock<Option<(u32, u32)>> = std::sync::OnceLock::new();
    let (numer, denom) = TIMEBASE
        .get_or_init(|| {
            let mut info = mach2::mach_time::mach_timebase_info { numer: 0, denom: 0 };
            // A valid writable timebase pointer; no capture or permission request.
            let status = unsafe { mach2::mach_time::mach_timebase_info(&mut info) };
            (status == 0 && info.denom != 0).then_some((info.numer, info.denom))
        })
        .context("cannot inspect audio host clock")?;
    Ok(ticks as u128 * numer as u128 / denom as u128)
}

pub(crate) fn check_platform() -> Result<()> {
    #[cfg(not(target_os = "macos"))]
    if std::env::var_os("FSCRIPT_CAPTURE_TEST_WAV").is_none() {
        bail!("microphone recording currently requires macOS; file and URL transcription work on this platform");
    }
    Ok(())
}

impl Capture {
    pub(crate) fn start(path: &Path) -> Result<Self> {
        let source = AudioSource::microphone()?;
        let writer =
            hound::WavWriter::create(path, source.spec).context("cannot create captured audio")?;
        let channels = source.spec.channels;
        Ok(Self {
            source,
            writer,
            samples: 0,
            channels,
        })
    }

    pub(crate) fn poll(&mut self) -> Result<()> {
        let writer = &mut self.writer;
        let count = &mut self.samples;
        self.source.poll(|packet| {
            for sample in packet.samples {
                writer.write_sample(sample)?;
                *count += 1;
            }
            Ok(())
        })
    }

    pub(crate) fn finish(mut self) -> Result<()> {
        self.source.stop();
        self.poll()?;
        if self.samples / self.channels as usize == 0 {
            bail!("no microphone audio was captured; check microphone permission and the selected system input");
        }
        self.writer
            .finalize()
            .context("cannot finalize captured audio")
    }
}

impl AudioSource {
    /// The same transport feeds dictation's WAV sink and meeting's durable sink.
    pub(crate) fn microphone() -> Result<Self> {
        check_platform()?;
        // Explicit deterministic transport for process tests; never selected automatically.
        let (source, spec) = if let Some(input) = std::env::var_os("FSCRIPT_CAPTURE_TEST_WAV") {
            let reader = hound::WavReader::open(input).context("cannot open capture test WAV")?;
            let spec = reader.spec();
            if spec.sample_format != hound::SampleFormat::Int
                || spec.bits_per_sample != 16
                || spec.channels == 0
                || spec.sample_rate == 0
            {
                bail!(
                    "capture test WAV must contain 16-bit PCM with a valid rate and channel count"
                );
            }
            let packet_samples = ((spec.sample_rate / 50).max(1) * spec.channels as u32) as usize;
            (
                Source::Fixture {
                    reader,
                    next: Instant::now(),
                    packet_samples,
                    epoch_ns: host_time_ns()?,
                    delivered_samples: 0,
                },
                spec,
            )
        } else {
            #[cfg(target_os = "macos")]
            {
                let microphone = NativeCapture::start()?;
                let spec = microphone.spec;
                (Source::Microphone(microphone), spec)
            }
            #[cfg(not(target_os = "macos"))]
            bail!("microphone recording currently requires macOS; file and URL transcription work on this platform");
        };
        Ok(Self {
            source,
            spec,
            cutoff: None,
        })
    }

    /// Drain bounded native queues, or catch a fixture up to elapsed time. A sink
    /// failure propagates immediately instead of consuming subsequent packets.
    pub(crate) fn poll(
        &mut self,
        mut receive: impl FnMut(AudioPacket) -> Result<()>,
    ) -> Result<()> {
        match &mut self.source {
            #[cfg(target_os = "macos")]
            Source::Microphone(capture) => {
                capture.check()?;
                for _ in 0..128 {
                    let Ok(packet) = capture.audio.try_recv() else {
                        break;
                    };
                    receive(packet)?;
                }
            }
            Source::Fixture {
                reader,
                next,
                packet_samples,
                epoch_ns,
                delivered_samples,
            } => {
                // Match elapsed recording time even when a loaded host delays
                // the controller loop. One packet per poll truncates fixtures
                // on slower CI machines and breaks batch parity.
                let now = self.cutoff.unwrap_or_else(Instant::now);
                while now >= *next {
                    let samples = reader
                        .samples::<i16>()
                        .take(*packet_samples)
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    if samples.is_empty() {
                        *next = now + Duration::from_millis(20);
                        break;
                    }
                    let host_time_ns = *epoch_ns
                        + (*delivered_samples as u128 * 1_000_000_000)
                            / (self.spec.sample_rate as u128 * self.spec.channels as u128);
                    *next += Duration::from_secs_f64(
                        samples.len() as f64
                            / (self.spec.sample_rate as f64 * self.spec.channels as f64),
                    );
                    *delivered_samples += samples.len() as u64;
                    receive(AudioPacket {
                        samples,
                        host_time_ns,
                    })?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn stop(&mut self) {
        self.cutoff.get_or_insert_with(Instant::now);
        #[cfg(target_os = "macos")]
        if let Source::Microphone(capture) = &mut self.source {
            // Stop callbacks before draining the final packets.
            capture.stream.take();
        }
    }
}

#[cfg(target_os = "macos")]
struct NativeCapture {
    stream: Option<cpal::Stream>,
    audio: std::sync::mpsc::Receiver<AudioPacket>,
    errors: std::sync::mpsc::Receiver<String>,
    overflow: std::sync::Arc<std::sync::atomic::AtomicBool>,
    spec: hound::WavSpec,
}

#[cfg(target_os = "macos")]
impl NativeCapture {
    fn start() -> Result<Self> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let device = cpal::default_host().default_input_device().context(
            "no microphone input is available; choose an input in System Settings > Sound",
        )?;
        let config = device.default_input_config().context("cannot inspect microphone input; check System Settings > Privacy & Security > Microphone")?;
        let spec = hound::WavSpec {
            channels: config.channels() as u16,
            sample_rate: config.sample_rate(),
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let (sender, audio) = std::sync::mpsc::sync_channel(128);
        let (error_sender, errors) = std::sync::mpsc::sync_channel(1);
        let overflow = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let full = overflow.clone();
        let callback = move |data: &cpal::Data, info: &cpal::InputCallbackInfo| {
            let packet: Option<Vec<i16>> = match data.sample_format() {
                cpal::SampleFormat::F32 => data
                    .as_slice::<f32>()
                    .map(|samples| samples.iter().map(|v| quantize(*v as f64)).collect()),
                cpal::SampleFormat::F64 => data
                    .as_slice::<f64>()
                    .map(|samples| samples.iter().map(|v| quantize(*v)).collect()),
                cpal::SampleFormat::I16 => data.as_slice::<i16>().map(|samples| samples.to_vec()),
                cpal::SampleFormat::I32 => data
                    .as_slice::<i32>()
                    .map(|samples| samples.iter().map(|v| (v >> 16) as i16).collect()),
                _ => None,
            };
            if packet.is_none_or(|samples| {
                sender
                    .try_send(AudioPacket {
                        samples,
                        host_time_ns: info.timestamp().capture.as_nanos(),
                    })
                    .is_err()
            }) {
                full.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        };
        let stream = device.build_input_stream_raw(config.config(), config.sample_format(), callback, move |error| { let _ = error_sender.try_send(error.to_string()); }, Some(Duration::from_secs(5)))
            .context("cannot open microphone; allow microphone access in System Settings > Privacy & Security > Microphone")?;
        stream.play().context("cannot start microphone")?;
        Ok(Self {
            stream: Some(stream),
            audio,
            errors,
            overflow,
            spec,
        })
    }

    fn check(&self) -> Result<()> {
        if self.overflow.load(std::sync::atomic::Ordering::Relaxed) {
            bail!("microphone transport overflowed or returned an unsupported format; recording was stopped instead of losing audio silently");
        }
        if let Ok(error) = self.errors.try_recv() {
            bail!("microphone stopped: {error}");
        }
        Ok(())
    }
}

#[cfg(any(target_os = "macos", test))]
fn quantize(value: f64) -> i16 {
    if value.is_finite() {
        (value.clamp(-1.0, 1.0) * 32768.0)
            .clamp(-32768.0, 32767.0)
            .round() as i16
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delayed_fixture_poll_catches_up_without_extending_audio() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.wav");
        let output = directory.path().join("captured.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&source, spec).unwrap();
        let expected: Vec<i16> = (0..16000).map(|sample| sample as i16).collect();
        for sample in &expected {
            writer.write_sample(*sample).unwrap();
        }
        writer.finalize().unwrap();
        let mut capture = Capture {
            source: AudioSource {
                source: Source::Fixture {
                    reader: hound::WavReader::open(source).unwrap(),
                    next: Instant::now() - Duration::from_secs(2),
                    packet_samples: 320,
                    epoch_ns: 0,
                    delivered_samples: 0,
                },
                spec,
                cutoff: None,
            },
            writer: hound::WavWriter::create(&output, spec).unwrap(),
            samples: 0,
            channels: 1,
        };
        capture.poll().unwrap();
        assert_eq!(capture.samples, expected.len());
        capture.finish().unwrap();
        let actual: Vec<i16> = hound::WavReader::open(output)
            .unwrap()
            .samples::<i16>()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn capture_quantization_is_monotone_bounded_and_preserves_pcm_values() {
        for value in i16::MIN..=i16::MAX {
            assert_eq!(quantize(value as f64 / 32768.0), value);
            if value < i16::MAX {
                assert!(
                    quantize(value as f64 / 32768.0) <= quantize((value as f64 + 0.5) / 32768.0)
                );
            }
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(quantize(value), 0);
        }
        assert_eq!(quantize(10.0), i16::MAX);
        assert_eq!(quantize(-10.0), i16::MIN);
    }

    fn fixture_source(path: &Path, rate: u32, channels: u16) -> (AudioSource, Vec<i16>) {
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let samples: Vec<i16> = (0..(rate as usize / 7 * channels as usize))
            .map(|n| n as i16)
            .collect();
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for sample in &samples {
            writer.write_sample(*sample).unwrap();
        }
        writer.finalize().unwrap();
        let source = AudioSource {
            source: Source::Fixture {
                reader: hound::WavReader::open(path).unwrap(),
                next: Instant::now() - Duration::from_secs(2),
                packet_samples: (rate / 50).max(1) as usize * channels as usize,
                epoch_ns: 9_000_000_000,
                delivered_samples: 0,
            },
            spec,
            cutoff: None,
        };
        (source, samples)
    }

    #[test]
    fn delayed_packet_timestamps_preserve_pcm_and_channel_clock_for_multiple_rates() {
        let temp = tempfile::tempdir().unwrap();
        for rate in [8000, 11025, 16000, 44100, 48000] {
            for channels in [1, 2] {
                let path = temp.path().join(format!("{rate}-{channels}.wav"));
                let (mut source, expected) = fixture_source(&path, rate, channels);
                let mut actual = Vec::new();
                let mut packets = 0;
                source
                    .poll(|packet| {
                        assert_eq!(
                            packet.host_time_ns,
                            9_000_000_000
                                + actual.len() as u128 * 1_000_000_000
                                    / (rate as u128 * channels as u128)
                        );
                        assert!(!packet.samples.is_empty());
                        assert_eq!(packet.samples.len() % channels as usize, 0);
                        actual.extend(packet.samples);
                        packets += 1;
                        Ok(())
                    })
                    .unwrap();
                assert!(packets > 1);
                assert_eq!(actual, expected);
                source.stop();
                source
                    .poll(|_| bail!("EOF must never synthesize additional packets"))
                    .unwrap();
            }
        }
    }

    #[test]
    fn stopped_fixture_does_not_advance_while_a_consumer_finishes() {
        let temp = tempfile::tempdir().unwrap();
        let (mut source, _) = fixture_source(&temp.path().join("source.wav"), 16000, 1);
        source.stop();
        let stopped = source.cutoff.unwrap();
        match &mut source.source {
            Source::Fixture { next, .. } => *next = stopped + Duration::from_nanos(1),
            #[cfg(target_os = "macos")]
            Source::Microphone(_) => unreachable!("explicit fixture cannot open a microphone"),
        }
        std::thread::sleep(Duration::from_millis(1));
        source.stop();
        assert_eq!(
            source.cutoff,
            Some(stopped),
            "repeated stop cannot extend capture"
        );
        source
            .poll(|_| bail!("stop must freeze fixture arrival time"))
            .unwrap();
    }

    #[test]
    fn consumer_failure_stops_draining_before_later_packets_are_consumed() {
        let temp = tempfile::tempdir().unwrap();
        let (mut source, expected) = fixture_source(&temp.path().join("source.wav"), 16000, 1);
        let mut first = Vec::new();
        let result = source.poll(|packet| {
            first.extend(packet.samples);
            bail!("disk consumer failed")
        });
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("disk consumer failed"));
        assert_eq!(first.len(), 320);
        let mut remaining = Vec::new();
        source
            .poll(|packet| {
                remaining.extend(packet.samples);
                Ok(())
            })
            .unwrap();
        first.extend(remaining);
        assert_eq!(first, expected);
    }
}
