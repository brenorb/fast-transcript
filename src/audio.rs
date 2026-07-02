use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

use crate::types::{FfprobeOutput, FfprobeStream, PreparedAudio};

fn probe_audio_with(ffprobe_program: &str, path: &Path) -> Result<FfprobeStream> {
    let output = Command::new(ffprobe_program)
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

fn probe_audio(path: &Path) -> Result<FfprobeStream> {
    probe_audio_with("ffprobe", path)
}

pub(crate) fn is_supported_audio(stream: &FfprobeStream) -> bool {
    let sample_rate_ok = stream.sample_rate.as_deref() == Some("16000");
    let channels_ok = stream.channels == Some(1);
    let codec_ok = stream.codec_name.as_deref() == Some("pcm_s16le");
    let bits_ok = stream.bits_per_sample == Some(16) || stream.sample_fmt.as_deref() == Some("s16");
    sample_rate_ok && channels_ok && codec_ok && bits_ok
}

fn normalize_audio_with(
    ffprobe_program: &str,
    ffmpeg_program: &str,
    input_path: &Path,
) -> Result<PreparedAudio> {
    let stream = probe_audio_with(ffprobe_program, input_path)?;
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
    let status = Command::new(ffmpeg_program)
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

pub(crate) fn normalize_audio(input_path: &Path) -> Result<PreparedAudio> {
    normalize_audio_with("ffprobe", "ffmpeg", input_path)
}

#[cfg(test)]
mod tests {
    use super::{is_supported_audio, normalize_audio_with};
    use crate::types::FfprobeStream;
    use std::fs;
    use tempfile::tempdir;

    fn write_shell_script(path: &std::path::Path, body: &str) {
        fs::write(path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms).unwrap();
        }
    }

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

    #[test]
    fn normalize_audio_reuses_supported_audio_without_running_ffmpeg() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("input.wav");
        let ffprobe = dir.path().join("ffprobe");
        let ffmpeg = dir.path().join("ffmpeg");
        let marker = dir.path().join("ffmpeg-ran");

        fs::write(&input, "audio").unwrap();
        write_shell_script(
            &ffprobe,
            "#!/bin/sh\ncat <<'JSON'\n{\"streams\":[{\"codec_type\":\"audio\",\"codec_name\":\"pcm_s16le\",\"sample_rate\":\"16000\",\"channels\":1,\"bits_per_sample\":16,\"sample_fmt\":\"s16\"}]}\nJSON\n",
        );
        write_shell_script(
            &ffmpeg,
            &format!(
                "#!/bin/sh\nprintf '%s' ran > \"{}\"\nexit 1\n",
                marker.display()
            ),
        );

        let prepared = normalize_audio_with(
            ffprobe.to_string_lossy().as_ref(),
            ffmpeg.to_string_lossy().as_ref(),
            &input,
        )
        .unwrap();

        assert_eq!(prepared.wav_path, input);
        assert!(!prepared.normalized);
        assert!(prepared._tempdir.is_none());
        assert!(!marker.exists());
    }

    #[test]
    fn normalize_audio_converts_unsupported_audio_with_ffmpeg() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("input.mp3");
        let ffprobe = dir.path().join("ffprobe");
        let ffmpeg = dir.path().join("ffmpeg");

        fs::write(&input, "audio").unwrap();
        write_shell_script(
            &ffprobe,
            "#!/bin/sh\ncat <<'JSON'\n{\"streams\":[{\"codec_type\":\"audio\",\"codec_name\":\"mp3\",\"sample_rate\":\"44100\",\"channels\":2,\"bits_per_sample\":0,\"sample_fmt\":\"fltp\"}]}\nJSON\n",
        );
        write_shell_script(
            &ffmpeg,
            "#!/bin/sh\nout=\"${@: -1}\"\nmkdir -p \"$(dirname \"$out\")\"\nprintf '%s' normalized > \"$out\"\n",
        );

        let prepared = normalize_audio_with(
            ffprobe.to_string_lossy().as_ref(),
            ffmpeg.to_string_lossy().as_ref(),
            &input,
        )
        .unwrap();

        assert!(prepared.normalized);
        assert!(prepared.wav_path.ends_with("input.16k_mono.wav"));
        assert!(prepared.wav_path.exists());
        assert_eq!(
            fs::read_to_string(&prepared.wav_path).unwrap(),
            "normalized"
        );
        assert!(prepared._tempdir.is_some());
    }
}
