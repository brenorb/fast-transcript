//! Microphone transport. Callbacks never block on disk or silently drop audio.

use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) struct Capture {
    source: Source,
    writer: hound::WavWriter<BufWriter<File>>,
    samples: usize,
    channels: u16,
}

enum Source {
    #[cfg(target_os = "macos")]
    Microphone(NativeCapture),
    Fixture {
        reader: hound::WavReader<BufReader<File>>,
        next: Instant,
        packet_samples: usize,
    },
}

impl Capture {
    pub(crate) fn start(path: &Path) -> Result<Self> {
        // Explicit deterministic transport for process tests; never selected automatically.
        let (source, spec) = if let Some(input) = std::env::var_os("FSCRIPT_CAPTURE_TEST_WAV") {
            let reader = hound::WavReader::open(input).context("cannot open capture test WAV")?;
            let spec = reader.spec();
            if spec.sample_format != hound::SampleFormat::Int || spec.bits_per_sample != 16 {
                bail!("capture test WAV must contain 16-bit PCM");
            }
            let packet_samples = ((spec.sample_rate / 50).max(1) * spec.channels as u32) as usize;
            (
                Source::Fixture {
                    reader,
                    next: Instant::now(),
                    packet_samples,
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
        let writer =
            hound::WavWriter::create(path, spec).context("cannot create captured audio")?;
        Ok(Self {
            source,
            writer,
            samples: 0,
            channels: spec.channels,
        })
    }

    pub(crate) fn poll(&mut self) -> Result<()> {
        match &mut self.source {
            #[cfg(target_os = "macos")]
            Source::Microphone(capture) => {
                capture.check()?;
                for _ in 0..128 {
                    let Ok(packet) = capture.audio.try_recv() else {
                        break;
                    };
                    for sample in packet {
                        self.writer.write_sample(sample)?;
                        self.samples += 1;
                    }
                }
            }
            Source::Fixture {
                reader,
                next,
                packet_samples,
            } => {
                if Instant::now() >= *next {
                    for sample in reader.samples::<i16>().take(*packet_samples) {
                        self.writer.write_sample(sample?)?;
                        self.samples += 1;
                    }
                    *next += Duration::from_millis(20);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<()> {
        #[cfg(target_os = "macos")]
        if let Source::Microphone(capture) = &mut self.source {
            // Stop callbacks before draining the final packets.
            capture.stream.take();
        }
        self.poll()?;
        if self.samples / self.channels as usize == 0 {
            bail!("no microphone audio was captured; check microphone permission and the selected system input");
        }
        self.writer
            .finalize()
            .context("cannot finalize captured audio")
    }
}

#[cfg(target_os = "macos")]
struct NativeCapture {
    stream: Option<cpal::Stream>,
    audio: std::sync::mpsc::Receiver<Vec<i16>>,
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
        let callback = move |data: &cpal::Data, _: &cpal::InputCallbackInfo| {
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
            if packet.is_none_or(|packet| sender.try_send(packet).is_err()) {
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
}
