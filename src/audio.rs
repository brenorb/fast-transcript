use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

use crate::types::{FfprobeOutput, FfprobeStream, PreparedAudio};

fn probe_audio(path: &Path) -> Result<FfprobeStream> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_streams",
            path.to_string_lossy().as_ref(),
        ])
        .output()
        .with_context(|| "failed to run ffprobe; install ffmpeg/ffprobe to inspect audio")?;
    if !output.status.success() {
        bail!(
            "ffprobe failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let parsed: FfprobeOutput = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("failed to parse ffprobe output for {}", path.display()))?;
    parsed
        .streams
        .into_iter()
        .find(|stream| stream.codec_type.as_deref() == Some("audio"))
        .with_context(|| format!("no audio stream found in {}", path.display()))
}

pub(crate) fn is_supported_audio(stream: &FfprobeStream) -> bool {
    let sample_rate_ok = stream.sample_rate.as_deref() == Some("16000");
    let channels_ok = stream.channels == Some(1);
    let codec_ok = stream.codec_name.as_deref() == Some("pcm_s16le");
    let bits_ok = stream.bits_per_sample == Some(16) || stream.sample_fmt.as_deref() == Some("s16");
    sample_rate_ok && channels_ok && codec_ok && bits_ok
}

pub(crate) fn normalize_audio(input_path: &Path) -> Result<PreparedAudio> {
    let stream = probe_audio(input_path)?;
    if is_supported_audio(&stream) {
        return Ok(PreparedAudio {
            wav_path: input_path.to_path_buf(),
            normalized: false,
            _tempdir: None,
        });
    }

    let tempdir = tempfile::tempdir().context("failed to create temp dir for normalized audio")?;
    let stem = input_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("audio");
    let normalized_path = tempdir.path().join(format!("{stem}.16k_mono.wav"));

    eprintln!("normalizing audio...");
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostats",
            "-y",
            "-i",
            input_path.to_string_lossy().as_ref(),
            "-vn",
            "-sn",
            "-dn",
            "-ar",
            "16000",
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
            normalized_path.to_string_lossy().as_ref(),
        ])
        .status()
        .with_context(|| "failed to run ffmpeg; install ffmpeg to normalize audio")?;
    if !status.success() {
        bail!("ffmpeg failed while converting {}", input_path.display());
    }

    Ok(PreparedAudio {
        wav_path: normalized_path,
        normalized: true,
        _tempdir: Some(tempdir),
    })
}

#[cfg(test)]
mod tests {
    use super::is_supported_audio;
    use crate::types::FfprobeStream;

    #[test]
    fn supported_audio_requires_16k_mono_pcm_s16le() {
        let ok = FfprobeStream {
            codec_type: Some("audio".to_string()),
            codec_name: Some("pcm_s16le".to_string()),
            sample_rate: Some("16000".to_string()),
            channels: Some(1),
            bits_per_sample: Some(16),
            sample_fmt: Some("s16".to_string()),
        };
        let bad = FfprobeStream {
            codec_type: Some("audio".to_string()),
            codec_name: Some("mp3".to_string()),
            sample_rate: Some("44100".to_string()),
            channels: Some(2),
            bits_per_sample: Some(0),
            sample_fmt: Some("fltp".to_string()),
        };
        assert!(is_supported_audio(&ok));
        assert!(!is_supported_audio(&bad));
    }
}
