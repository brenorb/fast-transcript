use anyhow::{bail, Context, Result};
use directories::ProjectDirs;
use std::env;
use std::path::PathBuf;

use crate::diarization::{DiarizationBackend, DiarizationRequest};
use crate::types::{CliArgs, OutputFormat, ScriptFormat, SubtitleFormat, TextFormat};

pub(crate) fn usage() -> String {
    format!(
        "usage: fscript <audio-or-url> [output-path | - | --stdout] [--script [plain|timestamps] | --text [plain|timestamps] | --srt | --vtt] [--prefer-local-for-remote] [-d [{}|{}] | --diarize [{}|{}]] [-n N | --num-speakers N] [-t N | --threshold N] [--model-dir PATH] [--model-package PATH] [--model-url URL] [--chunk-seconds N] [--chunk-overlap-seconds N]\n\
aliases:\n\
  -d [{}|{}]\n\
  -c, --clean\n\
  --script [plain|timestamps]\n\
  --text [plain|timestamps]\n\
  --srt\n\
  --vtt\n\
  -n, --num-speakers <count>\n\
  -t, --threshold <value>\n\
defaults:\n\
  --script timestamps\n\
  --text plain\n\
  --model-dir {}\n\
  --model-package {}\n\
  --chunk-seconds 120\n\
  --chunk-overlap-seconds 2",
        DiarizationBackend::Coreml.cli_name(),
        DiarizationBackend::LseendDihard3.cli_name(),
        DiarizationBackend::Coreml.cli_name(),
        DiarizationBackend::LseendDihard3.cli_name(),
        DiarizationBackend::Coreml.cli_name(),
        DiarizationBackend::LseendDihard3.cli_name(),
        default_model_dir().display(),
        default_model_package().display()
    )
}

pub(crate) fn version_string() -> String {
    format!("fscript {}", env!("CARGO_PKG_VERSION"))
}

fn default_app_data_dir() -> PathBuf {
    if let Some(project_dirs) = ProjectDirs::from("", "", "fast-transcript") {
        return project_dirs.data_local_dir().to_path_buf();
    }

    env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".local").join("share").join("fast-transcript"))
        .unwrap_or_else(|| PathBuf::from(crate::DEFAULT_DATA_DIR_FALLBACK))
}

fn default_app_cache_dir() -> PathBuf {
    if let Some(project_dirs) = ProjectDirs::from("", "", "fast-transcript") {
        return project_dirs.cache_dir().to_path_buf();
    }

    env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".cache").join("fast-transcript"))
        .unwrap_or_else(|| PathBuf::from(crate::DEFAULT_CACHE_DIR_FALLBACK))
}

pub(crate) fn default_model_dir() -> PathBuf {
    env::var_os("FSCRIPT_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            default_app_data_dir()
                .join(crate::DEFAULT_MODEL_SUBDIR)
                .join(crate::DEFAULT_MODEL_BASENAME)
        })
}

pub(crate) fn default_model_package() -> PathBuf {
    env::var_os("FSCRIPT_MODEL_PACKAGE")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_app_cache_dir().join(crate::DEFAULT_MODEL_PACKAGE_NAME))
}

fn default_model_url() -> String {
    env::var("FSCRIPT_MODEL_URL").unwrap_or_else(|_| crate::DEFAULT_MODEL_URL.to_string())
}

pub(crate) fn parse_args(raw_args: &[String]) -> Result<CliArgs> {
    if raw_args.is_empty() {
        bail!("{}", usage());
    }

    let mut model_dir = default_model_dir();
    let mut model_package = default_model_package();
    let mut model_url = default_model_url();
    let mut input = None;
    let mut output_path = None;
    let mut output_to_stdout = false;
    let mut output_format = OutputFormat::Json;
    let mut clean_output = false;
    let mut prefer_local_for_remote = false;
    let mut chunk_seconds_override = None;
    let mut chunk_overlap_seconds_override = None;
    let mut diarization_requested = false;
    let mut diarization_backend = DiarizationBackend::Coreml;
    let mut diarization_num_speakers = None;
    let mut diarization_threshold = None;
    let mut index = 0usize;

    while index < raw_args.len() {
        match raw_args[index].as_str() {
            "--model-dir" => {
                let value = raw_args
                    .get(index + 1)
                    .with_context(|| format!("missing value for --model-dir\n{}", usage()))?;
                model_dir = PathBuf::from(value);
                index += 2;
            }
            "--model-package" => {
                let value = raw_args
                    .get(index + 1)
                    .with_context(|| format!("missing value for --model-package\n{}", usage()))?;
                model_package = PathBuf::from(value);
                index += 2;
            }
            "--model-url" => {
                let value = raw_args
                    .get(index + 1)
                    .with_context(|| format!("missing value for --model-url\n{}", usage()))?;
                model_url = value.to_string();
                index += 2;
            }
            "--stdout" => {
                output_to_stdout = true;
                index += 1;
            }
            "-c" | "--clean" => {
                clean_output = true;
                index += 1;
            }
            "--script" => {
                output_format = OutputFormat::Script(ScriptFormat::Timestamped);
                if let Some(value) = raw_args.get(index + 1) {
                    if let Some(script_format) = ScriptFormat::from_cli_value(value) {
                        output_format = OutputFormat::Script(script_format);
                        index += 2;
                        continue;
                    }
                }
                index += 1;
            }
            "--text" => {
                output_format = OutputFormat::Text(TextFormat::Plain);
                if let Some(value) = raw_args.get(index + 1) {
                    if let Some(text_format) = TextFormat::from_cli_value(value) {
                        output_format = OutputFormat::Text(text_format);
                        index += 2;
                        continue;
                    }
                }
                index += 1;
            }
            "--srt" => {
                output_format = OutputFormat::Subtitle(SubtitleFormat::Srt);
                index += 1;
            }
            "--vtt" => {
                output_format = OutputFormat::Subtitle(SubtitleFormat::Vtt);
                index += 1;
            }
            "--prefer-local-for-remote" => {
                prefer_local_for_remote = true;
                index += 1;
            }
            "-d" | "--diarize" => {
                diarization_requested = true;
                if let Some(value) = raw_args.get(index + 1) {
                    if let Some(backend) = DiarizationBackend::from_cli_value(value) {
                        diarization_backend = backend;
                        index += 2;
                        continue;
                    }
                }
                index += 1;
            }
            "--num-speakers" | "-n" => {
                let value = raw_args
                    .get(index + 1)
                    .with_context(|| format!("missing value for --num-speakers\n{}", usage()))?;
                let parsed = value.parse::<usize>().with_context(|| {
                    format!("invalid --num-speakers value {value:?}\n{}", usage())
                })?;
                if parsed == 0 {
                    bail!("--num-speakers must be >= 1");
                }
                diarization_num_speakers = Some(parsed);
                index += 2;
            }
            "-t" | "--threshold" => {
                let value = raw_args
                    .get(index + 1)
                    .with_context(|| format!("missing value for --threshold\n{}", usage()))?;
                let parsed = value
                    .parse::<f64>()
                    .with_context(|| format!("invalid --threshold value {value:?}\n{}", usage()))?;
                diarization_threshold = Some(parsed);
                index += 2;
            }
            "--chunk-seconds" => {
                let value = raw_args
                    .get(index + 1)
                    .with_context(|| format!("missing value for --chunk-seconds\n{}", usage()))?;
                let parsed = value.parse::<f64>().with_context(|| {
                    format!("invalid --chunk-seconds value {value:?}\n{}", usage())
                })?;
                if parsed < 0.0 {
                    bail!("--chunk-seconds must be >= 0");
                }
                chunk_seconds_override = Some(parsed);
                index += 2;
            }
            "--chunk-overlap-seconds" => {
                let value = raw_args.get(index + 1).with_context(|| {
                    format!("missing value for --chunk-overlap-seconds\n{}", usage())
                })?;
                let parsed = value.parse::<f64>().with_context(|| {
                    format!(
                        "invalid --chunk-overlap-seconds value {value:?}\n{}",
                        usage()
                    )
                })?;
                if parsed < 0.0 {
                    bail!("--chunk-overlap-seconds must be >= 0");
                }
                chunk_overlap_seconds_override = Some(parsed);
                index += 2;
            }
            flag if flag.starts_with("--") => bail!("unknown argument {flag:?}\n{}", usage()),
            value => {
                if input.is_none() {
                    input = Some(value.to_string());
                } else if output_path.is_none() && !output_to_stdout {
                    if value == "-" {
                        output_to_stdout = true;
                    } else {
                        output_path = Some(PathBuf::from(value));
                    }
                } else {
                    bail!("unexpected positional argument {value:?}\n{}", usage());
                }
                index += 1;
            }
        }
    }

    let input = input.with_context(|| format!("missing audio path\n{}", usage()))?;
    let output_path = if output_to_stdout { None } else { output_path };
    if diarization_num_speakers.is_some() && !diarization_requested {
        bail!("--num-speakers requires -d or --diarize");
    }
    if diarization_threshold.is_some() && !diarization_requested {
        bail!("--threshold requires -d or --diarize");
    }
    if diarization_num_speakers.is_some()
        && diarization_backend == DiarizationBackend::LseendDihard3
    {
        bail!("--num-speakers is not supported with lseend-dihard3; use -t or --threshold instead");
    }

    let requested_chunk_seconds = chunk_seconds_override.unwrap_or(crate::DEFAULT_CHUNK_SECONDS);
    let (chunk_seconds, chunk_overlap_seconds) = if requested_chunk_seconds == 0.0 {
        let overlap = chunk_overlap_seconds_override.unwrap_or(0.0);
        if overlap > 0.0 {
            bail!("--chunk-overlap-seconds requires chunking to stay enabled");
        }
        (None, 0.0)
    } else {
        let overlap =
            chunk_overlap_seconds_override.unwrap_or(crate::DEFAULT_CHUNK_OVERLAP_SECONDS);
        if overlap >= requested_chunk_seconds {
            bail!("--chunk-overlap-seconds must be smaller than --chunk-seconds");
        }
        (Some(requested_chunk_seconds), overlap)
    };

    Ok(CliArgs {
        model_dir,
        model_package,
        model_url,
        input,
        output_path,
        output_to_stdout,
        output_format,
        clean_output,
        prefer_local_for_remote,
        chunk_seconds,
        chunk_overlap_seconds,
        diarization: diarization_requested.then_some(DiarizationRequest {
            backend: diarization_backend,
            num_speakers: diarization_num_speakers,
            threshold: diarization_threshold,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::{default_model_dir, default_model_package, parse_args, version_string};
    use crate::diarization::{DiarizationBackend, DiarizationRequest};
    use crate::types::{OutputFormat, ScriptFormat, SubtitleFormat, TextFormat};
    use std::path::Path;
    use std::path::PathBuf;

    fn path_ends_with(path: &Path, suffix: &[&str]) -> bool {
        let mut current = path;
        for expected in suffix.iter().rev() {
            let Some(name) = current.file_name().and_then(|value| value.to_str()) else {
                return false;
            };
            if name != *expected {
                return false;
            }
            let Some(parent) = current.parent() else {
                return false;
            };
            current = parent;
        }
        true
    }

    #[test]
    fn parse_args_defaults_to_easy_mode() {
        let args = vec!["audio.mp3".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.input, "audio.mp3");
        assert_eq!(parsed.output_path, None);
        assert!(!parsed.output_to_stdout);
        assert_eq!(parsed.output_format, OutputFormat::Json);
        assert!(!parsed.clean_output);
        assert_eq!(parsed.chunk_seconds, Some(120.0));
        assert_eq!(parsed.chunk_overlap_seconds, 2.0);
        assert_eq!(parsed.diarization, None);
        assert!(path_ends_with(
            &parsed.model_dir,
            &["models", crate::DEFAULT_MODEL_BASENAME]
        ));
        assert!(path_ends_with(
            &parsed.model_package,
            &[crate::DEFAULT_MODEL_PACKAGE_NAME]
        ));
    }

    #[test]
    fn default_model_paths_use_persistent_user_dirs() {
        assert!(path_ends_with(
            &default_model_dir(),
            &["models", crate::DEFAULT_MODEL_BASENAME]
        ));
        assert!(path_ends_with(
            &default_model_package(),
            &[crate::DEFAULT_MODEL_PACKAGE_NAME]
        ));
    }

    #[test]
    fn parse_args_supports_clean_flag() {
        let args = vec![
            "audio.mp3".to_string(),
            "--clean".to_string(),
            "--text".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert!(parsed.clean_output);
        assert_eq!(parsed.output_format, OutputFormat::Text(TextFormat::Plain));
    }

    #[test]
    fn parse_args_supports_clean_short_flag() {
        let args = vec![
            "audio.mp3".to_string(),
            "-c".to_string(),
            "--script".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert!(parsed.clean_output);
        assert_eq!(
            parsed.output_format,
            OutputFormat::Script(ScriptFormat::Timestamped)
        );
    }

    #[test]
    fn parse_args_accepts_optional_output_and_chunk_seconds() {
        let args = vec![
            "audio.wav".to_string(),
            "out.json".to_string(),
            "--model-dir".to_string(),
            "custom-model".to_string(),
            "--chunk-seconds".to_string(),
            "60".to_string(),
            "--chunk-overlap-seconds".to_string(),
            "1.5".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.output_path, Some(PathBuf::from("out.json")));
        assert_eq!(parsed.model_dir, PathBuf::from("custom-model"));
        assert_eq!(parsed.chunk_seconds, Some(60.0));
        assert_eq!(parsed.chunk_overlap_seconds, 1.5);
    }

    #[test]
    fn parse_args_supports_stdout_shortcut() {
        let args = vec!["audio.wav".to_string(), "-".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(parsed.output_to_stdout);
        assert_eq!(parsed.output_path, None);
    }

    #[test]
    fn parse_args_supports_stdout_flag() {
        let args = vec!["audio.wav".to_string(), "--stdout".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(parsed.output_to_stdout);
        assert_eq!(parsed.output_path, None);
    }

    #[test]
    fn parse_args_supports_script_output_defaulting_to_timestamps() {
        let args = vec!["audio.wav".to_string(), "--script".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.output_format,
            OutputFormat::Script(ScriptFormat::Timestamped)
        );
    }

    #[test]
    fn parse_args_supports_plain_script_output() {
        let args = vec![
            "audio.wav".to_string(),
            "--script".to_string(),
            "plain".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.output_format,
            OutputFormat::Script(ScriptFormat::Plain)
        );
    }

    #[test]
    fn parse_args_supports_plain_text_output() {
        let args = vec!["audio.wav".to_string(), "--text".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.output_format, OutputFormat::Text(TextFormat::Plain));
    }

    #[test]
    fn parse_args_supports_timestamped_text_output() {
        let args = vec![
            "audio.wav".to_string(),
            "--text".to_string(),
            "timestamps".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.output_format,
            OutputFormat::Text(TextFormat::Timestamped)
        );
    }

    #[test]
    fn parse_args_supports_srt_output() {
        let args = vec!["audio.wav".to_string(), "--srt".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.output_format,
            OutputFormat::Subtitle(SubtitleFormat::Srt)
        );
    }

    #[test]
    fn parse_args_supports_vtt_output() {
        let args = vec!["audio.wav".to_string(), "--vtt".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.output_format,
            OutputFormat::Subtitle(SubtitleFormat::Vtt)
        );
    }

    #[test]
    fn parse_args_supports_prefer_local_for_remote() {
        let args = vec![
            "https://www.youtube.com/watch?v=QSdh8Gj0mEg".to_string(),
            "--prefer-local-for-remote".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert!(parsed.prefer_local_for_remote);
    }

    #[test]
    fn parse_args_supports_optional_diarization() {
        let args = vec!["audio.wav".to_string(), "-d".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.diarization,
            Some(DiarizationRequest {
                backend: DiarizationBackend::Coreml,
                num_speakers: None,
                threshold: None,
            })
        );
    }

    #[test]
    fn parse_args_passes_num_speakers_when_diarization_is_enabled() {
        let args = vec![
            "audio.wav".to_string(),
            "--diarize".to_string(),
            "-n".to_string(),
            "2".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.diarization,
            Some(DiarizationRequest {
                backend: DiarizationBackend::Coreml,
                num_speakers: Some(2),
                threshold: None,
            })
        );
    }

    #[test]
    fn parse_args_rejects_num_speakers_without_diarization() {
        let args = vec!["audio.wav".to_string(), "-n".to_string(), "2".to_string()];
        let err = parse_args(&args).unwrap_err();
        assert!(err
            .to_string()
            .contains("--num-speakers requires -d or --diarize"));
    }

    #[test]
    fn parse_args_supports_num_speakers_long_flag() {
        let args = vec![
            "audio.wav".to_string(),
            "-d".to_string(),
            "--num-speakers".to_string(),
            "2".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.diarization,
            Some(DiarizationRequest {
                backend: DiarizationBackend::Coreml,
                num_speakers: Some(2),
                threshold: None,
            })
        );
    }

    #[test]
    fn parse_args_supports_diarization_backend_argument() {
        let args = vec![
            "audio.wav".to_string(),
            "-d".to_string(),
            "lseend-dihard3".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.diarization,
            Some(DiarizationRequest {
                backend: DiarizationBackend::LseendDihard3,
                num_speakers: None,
                threshold: None,
            })
        );
    }

    #[test]
    fn parse_args_supports_threshold_short_flag() {
        let args = vec![
            "audio.wav".to_string(),
            "-d".to_string(),
            "lseend-dihard3".to_string(),
            "-t".to_string(),
            "0.3".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.diarization,
            Some(DiarizationRequest {
                backend: DiarizationBackend::LseendDihard3,
                num_speakers: None,
                threshold: Some(0.3),
            })
        );
    }

    #[test]
    fn parse_args_rejects_threshold_without_diarization() {
        let args = vec![
            "audio.wav".to_string(),
            "--threshold".to_string(),
            "0.3".to_string(),
        ];
        let err = parse_args(&args).unwrap_err();
        assert!(err
            .to_string()
            .contains("--threshold requires -d or --diarize"));
    }

    #[test]
    fn parse_args_rejects_num_speakers_with_lseend_backend() {
        let args = vec![
            "audio.wav".to_string(),
            "-d".to_string(),
            "lseend-dihard3".to_string(),
            "--num-speakers".to_string(),
            "2".to_string(),
        ];
        let err = parse_args(&args).unwrap_err();
        assert!(err.to_string().contains(
            "--num-speakers is not supported with lseend-dihard3; use -t or --threshold instead"
        ));
    }

    #[test]
    fn version_string_matches_package_version() {
        assert_eq!(
            version_string(),
            format!("fscript {}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn parse_args_can_disable_chunking() {
        let args = vec![
            "audio.wav".to_string(),
            "--chunk-seconds".to_string(),
            "0".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.chunk_seconds, None);
        assert_eq!(parsed.chunk_overlap_seconds, 0.0);
    }
}
