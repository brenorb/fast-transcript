use anyhow::{anyhow, bail, Context, Result};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::types::{FfprobeOutput, FfprobeStream, PreparedAudio};

const FFMPEG_MISSING: &str = "fscript needs ffmpeg to read audio, but it is not installed. Install ffmpeg (for example: brew install ffmpeg), then try again";
const CONVERT_HINT: &str =
    "Check that it plays in a media player, or convert it to MP3 or WAV and try again";

/// What the user passed: a local file (checked for existence) or media fscript
/// already saved on their behalf (stdin, downloads), described by `label`.
pub(crate) struct MediaInput<'a> {
    pub(crate) path: &'a Path,
    pub(crate) label: &'a str,
    pub(crate) user_path: bool,
}

fn start_ffmpeg_tool(command: &mut Command) -> Result<std::process::Output> {
    command.output().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            anyhow!(FFMPEG_MISSING)
        } else {
            anyhow::Error::new(error).context("could not start ffmpeg to read the audio")
        }
    })
}

fn missing_input_message(path: &Path) -> String {
    let text = path.to_string_lossy();
    let looks_like_link = text.starts_with("www.")
        || ["youtube.com/", "youtu.be/", "vimeo.com/", ".com/", ".org/"]
            .iter()
            .any(|marker| text.contains(marker));
    let link_hint = if looks_like_link {
        ". For a web link, start it with https://"
    } else {
        ""
    };
    format!(
        "cannot find {}; check the file name and folder, then try again{link_hint}",
        path.display()
    )
}

fn check_user_path(path: &Path) -> Result<()> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => bail!(
            "{} is a folder; pass an audio or video file inside it",
            path.display()
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            bail!("{}", missing_input_message(path))
        }
        Err(error) => Err(error).with_context(|| format!("cannot open {}", path.display())),
    }
}

fn probe_audio_with(ffprobe_program: &str, input: &MediaInput<'_>) -> Result<FfprobeStream> {
    if input.user_path {
        check_user_path(input.path)?;
    }
    let path_argument =
        std::path::absolute(input.path).with_context(|| format!("cannot open {}", input.label))?;
    let output = start_ffmpeg_tool(
        Command::new(ffprobe_program)
            .args(["-v", "error", "-print_format", "json", "-show_streams"])
            .arg(path_argument),
    )?;
    let not_media = || {
        anyhow!(
            "{} is not an audio or video file fscript can read. {CONVERT_HINT}",
            input.label
        )
    };
    if !output.status.success() {
        return Err(not_media());
    }
    let parsed: FfprobeOutput = serde_json::from_slice(&output.stdout).map_err(|_| not_media())?;
    parsed
        .streams
        .into_iter()
        .find(|stream| stream.codec_type.as_deref() == Some("audio"))
        .with_context(|| {
            format!(
                "{} has no sound to transcribe; choose a file with an audio track",
                input.label
            )
        })
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
    input: &MediaInput<'_>,
) -> Result<PreparedAudio> {
    let input_path = input.path;
    let stream = probe_audio_with(ffprobe_program, input)?;
    if is_supported_audio(&stream) {
        return Ok(PreparedAudio {
            wav_path: input_path.to_path_buf(),
            normalized: false,
            _tempdir: None,
        });
    }

    let tempdir = crate::errors::temp(tempfile::tempdir())?;
    let stem = input_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("audio");
    let normalized_path = tempdir.path().join(format!("{stem}.16k_mono.wav"));

    eprintln!("normalizing audio...");
    let input_argument =
        std::path::absolute(input_path).with_context(|| format!("cannot open {}", input.label))?;
    // ffmpeg's own error lines are technical; keep them for FSCRIPT_DEBUG only.
    let output = start_ffmpeg_tool(
        Command::new(ffmpeg_program)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostats",
                "-y",
                "-i",
                input_argument.to_string_lossy().as_ref(),
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
            .stdin(Stdio::null()),
    )?;
    if !output.status.success() {
        if crate::errors::debug_enabled() {
            std::eprintln!("{}", String::from_utf8_lossy(&output.stderr).trim());
        }
        bail!(
            "could not convert {} to audio fscript can transcribe; the file may be damaged. {CONVERT_HINT}",
            input.label
        );
    }

    Ok(PreparedAudio {
        wav_path: normalized_path,
        normalized: true,
        _tempdir: Some(tempdir),
    })
}

pub(crate) fn normalize_audio(input: &MediaInput<'_>) -> Result<PreparedAudio> {
    normalize_audio_with("ffprobe", "ffmpeg", input)
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
            &super::MediaInput {
                path: &input,
                label: "input",
                user_path: true,
            },
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
            "#!/bin/sh\nfor out do :; done\nmkdir -p \"$(dirname \"$out\")\"\nprintf '%s' normalized > \"$out\"\n",
        );

        let prepared = normalize_audio_with(
            ffprobe.to_string_lossy().as_ref(),
            ffmpeg.to_string_lossy().as_ref(),
            &super::MediaInput {
                path: &input,
                label: "input",
                user_path: true,
            },
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
