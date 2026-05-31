use anyhow::{bail, Context, Result};
use directories::ProjectDirs;
use std::env;
use std::path::PathBuf;

use crate::diarization::{DiarizationBackend, DiarizationRequest};
use crate::types::{CliArgs, OutputFormat, SpeakersFormat, SubtitleFormat, TextFormat};

const DEFAULT_LSEEND_THRESHOLD: f64 = 0.3;

pub(crate) fn usage() -> String {
    format!(
        "usage: fscript <audio-or-url> [output-path | -o PATH | - | --stdout] [--speakers[=plain] | --text[=plain] | --json | --srt | --vtt] [--backend coreml|lseend-dihard3|none] [-n N | --num-speakers N] [-t N | --threshold N] [-l | --local] [--chunk N] [--overlap N] [--model-dir PATH] [--model-package PATH] [--model-url URL]\n\
aliases:\n\
  -o, --output <path>\n\
  --speakers[=plain]\n\
  --text[=plain]\n\
  --json\n\
  --raw\n\
  -l, --local\n\
  --backend <coreml|lseend-dihard3|none>\n\
  --srt\n\
  --vtt\n\
  -n, --num-speakers <count>\n\
  -t, --threshold <value>\n\
  --chunk <seconds>\n\
  --overlap <seconds>\n\
defaults:\n\
  --speakers timestamps\n\
  --text timestamps\n\
  --backend coreml\n\
  --backend=lseend-dihard3 => --threshold 0.3\n\
  clean output on\n\
  --model-dir {}\n\
  --model-package {}\n\
  --chunk 120\n\
  --overlap 2",
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

fn parse_backend_value(value: &str) -> Result<Option<DiarizationBackend>> {
    match value {
        "none" => Ok(None),
        value => DiarizationBackend::from_cli_value(value)
            .map(Some)
            .with_context(|| {
                format!(
                    "invalid --backend value {value:?}; expected coreml, lseend-dihard3, or none"
                )
            }),
    }
}

fn ensure_single_output_format(
    selected_output_flag: &mut Option<&'static str>,
    flag_name: &'static str,
) -> Result<()> {
    if let Some(previous) = *selected_output_flag {
        if previous != flag_name {
            bail!(
                "output format flags are mutually exclusive; received both {previous} and {flag_name}"
            );
        }
    } else {
        *selected_output_flag = Some(flag_name);
    }
    Ok(())
}

fn ensure_single_output_path_source(
    selected_output_path_source: &mut Option<&'static str>,
    source_name: &'static str,
) -> Result<()> {
    if let Some(previous) = *selected_output_path_source {
        bail!(
            "cannot use both {previous} and {source_name} for output path; choose one and remove the other"
        );
    }
    *selected_output_path_source = Some(source_name);
    Ok(())
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
    let mut output_path_source = None;
    let mut output_format = OutputFormat::Speakers(SpeakersFormat::Timestamped);
    let mut selected_output_flag = None;
    let mut clean_output = true;
    let mut clean_flag = None;
    let mut force_local_for_remote = false;
    let mut chunk_seconds_override = None;
    let mut chunk_overlap_seconds_override = None;
    let mut diarization_backend = Some(DiarizationBackend::Coreml);
    let mut diarization_num_speakers = None;
    let mut diarization_threshold = None;
    let mut index = 0usize;

    while index < raw_args.len() {
        match raw_args[index].as_str() {
            "--output" | "-o" => {
                let value = raw_args
                    .get(index + 1)
                    .with_context(|| format!("missing value for --output\n{}", usage()))?;
                if output_to_stdout {
                    bail!("cannot use both --output and --stdout; remove one");
                }
                ensure_single_output_path_source(&mut output_path_source, "--output")?;
                output_path = Some(PathBuf::from(value));
                index += 2;
            }
            flag if flag.starts_with("--output=") => {
                let value = flag
                    .split_once('=')
                    .map(|(_, value)| value)
                    .filter(|value| !value.is_empty())
                    .with_context(|| format!("missing value for --output\n{}", usage()))?;
                if output_to_stdout {
                    bail!("cannot use both --output and --stdout; remove one");
                }
                ensure_single_output_path_source(&mut output_path_source, "--output")?;
                output_path = Some(PathBuf::from(value));
                index += 1;
            }
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
                if output_path.is_some() {
                    bail!("cannot use both --stdout and an explicit output path; remove one");
                }
                output_to_stdout = true;
                index += 1;
            }
            "--json" => {
                ensure_single_output_format(&mut selected_output_flag, "--json")?;
                output_format = OutputFormat::Json;
                index += 1;
            }
            "--raw" => {
                if clean_flag == Some("--clean") {
                    bail!("cannot use both --raw and --clean; remove one");
                }
                clean_flag = Some("--raw");
                clean_output = false;
                index += 1;
            }
            "-c" | "--clean" => {
                if clean_flag == Some("--raw") {
                    bail!("cannot use both --raw and --clean; remove one");
                }
                clean_flag = Some("--clean");
                clean_output = true;
                index += 1;
            }
            "--speakers" => {
                ensure_single_output_format(&mut selected_output_flag, "--speakers")?;
                output_format = OutputFormat::Speakers(SpeakersFormat::Timestamped);
                if let Some(value) = raw_args.get(index + 1) {
                    if let Some(speakers_format) = SpeakersFormat::from_cli_value(value) {
                        output_format = OutputFormat::Speakers(speakers_format);
                        index += 2;
                        continue;
                    }
                }
                index += 1;
            }
            flag if flag.starts_with("--speakers=") => {
                ensure_single_output_format(&mut selected_output_flag, "--speakers")?;
                let value = flag
                    .split_once('=')
                    .map(|(_, value)| value)
                    .with_context(|| format!("missing value for --speakers\n{}", usage()))?;
                let speakers_format = SpeakersFormat::from_cli_value(value).with_context(|| {
                    format!("invalid --speakers value {value:?}; expected plain or timestamps")
                })?;
                output_format = OutputFormat::Speakers(speakers_format);
                index += 1;
            }
            "--text" => {
                ensure_single_output_format(&mut selected_output_flag, "--text")?;
                output_format = OutputFormat::Text(TextFormat::Timestamped);
                if let Some(value) = raw_args.get(index + 1) {
                    if let Some(text_format) = TextFormat::from_cli_value(value) {
                        output_format = OutputFormat::Text(text_format);
                        index += 2;
                        continue;
                    }
                }
                index += 1;
            }
            flag if flag.starts_with("--text=") => {
                ensure_single_output_format(&mut selected_output_flag, "--text")?;
                let value = flag
                    .split_once('=')
                    .map(|(_, value)| value)
                    .with_context(|| format!("missing value for --text\n{}", usage()))?;
                let text_format = TextFormat::from_cli_value(value).with_context(|| {
                    format!("invalid --text value {value:?}; expected plain or timestamps")
                })?;
                output_format = OutputFormat::Text(text_format);
                index += 1;
            }
            "--srt" => {
                ensure_single_output_format(&mut selected_output_flag, "--srt")?;
                output_format = OutputFormat::Subtitle(SubtitleFormat::Srt);
                index += 1;
            }
            "--vtt" => {
                ensure_single_output_format(&mut selected_output_flag, "--vtt")?;
                output_format = OutputFormat::Subtitle(SubtitleFormat::Vtt);
                index += 1;
            }
            "--local" | "-l" | "--prefer-local-for-remote" => {
                force_local_for_remote = true;
                index += 1;
            }
            "--backend" => {
                let value = raw_args
                    .get(index + 1)
                    .with_context(|| format!("missing value for --backend\n{}", usage()))?;
                diarization_backend = parse_backend_value(value)?;
                index += 2;
            }
            flag if flag.starts_with("--backend=") => {
                let value = flag
                    .split_once('=')
                    .map(|(_, value)| value)
                    .with_context(|| format!("missing value for --backend\n{}", usage()))?;
                diarization_backend = parse_backend_value(value)?;
                index += 1;
            }
            "-d" | "--diarize" => {
                if let Some(value) = raw_args.get(index + 1) {
                    if let Ok(parsed_backend) = parse_backend_value(value) {
                        diarization_backend = parsed_backend;
                        index += 2;
                        continue;
                    }
                }
                if diarization_backend.is_none() {
                    diarization_backend = Some(DiarizationBackend::Coreml);
                }
                index += 1;
            }
            flag if flag.starts_with("--diarize=") => {
                let value = flag
                    .split_once('=')
                    .map(|(_, value)| value)
                    .with_context(|| format!("missing value for --diarize\n{}", usage()))?;
                diarization_backend = parse_backend_value(value)?;
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
            "--chunk" | "--chunk-seconds" => {
                let value = raw_args
                    .get(index + 1)
                    .with_context(|| format!("missing value for --chunk\n{}", usage()))?;
                let parsed = value
                    .parse::<f64>()
                    .with_context(|| format!("invalid --chunk value {value:?}\n{}", usage()))?;
                if parsed < 0.0 {
                    bail!("--chunk must be >= 0");
                }
                chunk_seconds_override = Some(parsed);
                index += 2;
            }
            "--overlap" | "--chunk-overlap-seconds" => {
                let value = raw_args
                    .get(index + 1)
                    .with_context(|| format!("missing value for --overlap\n{}", usage()))?;
                let parsed = value
                    .parse::<f64>()
                    .with_context(|| format!("invalid --overlap value {value:?}\n{}", usage()))?;
                if parsed < 0.0 {
                    bail!("--overlap must be >= 0");
                }
                chunk_overlap_seconds_override = Some(parsed);
                index += 2;
            }
            flag if flag.starts_with("--") => bail!("unknown argument {flag:?}\n{}", usage()),
            value => {
                if input.is_none() {
                    input = Some(value.to_string());
                } else if !output_to_stdout && output_path.is_none() {
                    if value == "-" {
                        if output_path.is_some() {
                            bail!(
                                "cannot use both --stdout and an explicit output path; remove one"
                            );
                        }
                        output_to_stdout = true;
                    } else {
                        ensure_single_output_path_source(
                            &mut output_path_source,
                            "positional output path",
                        )?;
                        output_path = Some(PathBuf::from(value));
                    }
                } else if output_path_source == Some("--output") {
                    bail!(
                        "cannot use both positional output path and --output for output path; choose one and remove the other"
                    );
                } else {
                    bail!("unexpected positional argument {value:?}\n{}", usage());
                }
                index += 1;
            }
        }
    }

    let input = input.with_context(|| format!("missing audio path\n{}", usage()))?;
    let output_path = if output_to_stdout { None } else { output_path };
    if diarization_backend.is_none() && diarization_num_speakers.is_some() {
        bail!(
            "--num-speakers requires diarization; remove --num-speakers or choose --backend=coreml"
        );
    }
    if diarization_backend.is_none() && diarization_threshold.is_some() {
        bail!(
            "--threshold requires diarization; remove --threshold or choose --backend=lseend-dihard3"
        );
    }
    if diarization_num_speakers.is_some()
        && diarization_backend == Some(DiarizationBackend::LseendDihard3)
    {
        bail!(
            "--num-speakers is not supported with --backend=lseend-dihard3; remove --num-speakers or switch to --backend=coreml"
        );
    }
    if diarization_backend == Some(DiarizationBackend::LseendDihard3)
        && diarization_threshold.is_none()
    {
        diarization_threshold = Some(DEFAULT_LSEEND_THRESHOLD);
    }
    if diarization_threshold.is_some()
        && diarization_backend != Some(DiarizationBackend::LseendDihard3)
    {
        bail!(
            "--threshold only works with --backend=lseend-dihard3; remove --threshold or switch backends"
        );
    }

    let requested_chunk_seconds = chunk_seconds_override.unwrap_or(crate::DEFAULT_CHUNK_SECONDS);
    let (chunk_seconds, chunk_overlap_seconds) = if requested_chunk_seconds == 0.0 {
        let overlap = chunk_overlap_seconds_override.unwrap_or(0.0);
        if overlap > 0.0 {
            bail!("--overlap requires chunking to stay enabled; remove --overlap or set --chunk to a positive value");
        }
        (None, 0.0)
    } else {
        let overlap =
            chunk_overlap_seconds_override.unwrap_or(crate::DEFAULT_CHUNK_OVERLAP_SECONDS);
        if overlap >= requested_chunk_seconds {
            bail!("--overlap must be smaller than --chunk");
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
        force_local_for_remote,
        chunk_seconds,
        chunk_overlap_seconds,
        diarization: diarization_backend.map(|backend| DiarizationRequest {
            backend,
            num_speakers: diarization_num_speakers,
            threshold: diarization_threshold,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::{default_model_dir, default_model_package, parse_args, version_string};
    use crate::diarization::{DiarizationBackend, DiarizationRequest};
    use crate::types::{OutputFormat, SpeakersFormat, SubtitleFormat, TextFormat};
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
        assert_eq!(
            parsed.output_format,
            OutputFormat::Speakers(SpeakersFormat::Timestamped)
        );
        assert!(parsed.clean_output);
        assert_eq!(parsed.chunk_seconds, Some(120.0));
        assert_eq!(parsed.chunk_overlap_seconds, 2.0);
        assert_eq!(
            parsed.diarization,
            Some(DiarizationRequest {
                backend: DiarizationBackend::Coreml,
                num_speakers: None,
                threshold: None,
            })
        );
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
        let args = vec!["audio.mp3".to_string(), "--clean".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(parsed.clean_output);
    }

    #[test]
    fn parse_args_supports_raw_flag() {
        let args = vec!["audio.mp3".to_string(), "--raw".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert!(!parsed.clean_output);
    }

    #[test]
    fn parse_args_rejects_raw_and_clean_together() {
        let args = vec![
            "audio.mp3".to_string(),
            "--raw".to_string(),
            "--clean".to_string(),
        ];
        let err = parse_args(&args).unwrap_err();
        assert!(err
            .to_string()
            .contains("cannot use both --raw and --clean"));
    }

    #[test]
    fn parse_args_accepts_optional_output_and_chunk_seconds() {
        let args = vec![
            "audio.wav".to_string(),
            "out.json".to_string(),
            "--model-dir".to_string(),
            "custom-model".to_string(),
            "--chunk".to_string(),
            "60".to_string(),
            "--overlap".to_string(),
            "1.5".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.output_path, Some(PathBuf::from("out.json")));
        assert_eq!(parsed.model_dir, PathBuf::from("custom-model"));
        assert_eq!(parsed.chunk_seconds, Some(60.0));
        assert_eq!(parsed.chunk_overlap_seconds, 1.5);
    }

    #[test]
    fn parse_args_supports_output_flag() {
        let args = vec![
            "audio.wav".to_string(),
            "-o".to_string(),
            "out.json".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.output_path, Some(PathBuf::from("out.json")));
    }

    #[test]
    fn parse_args_supports_output_flag_equals_syntax() {
        let args = vec!["audio.wav".to_string(), "--output=out.json".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.output_path, Some(PathBuf::from("out.json")));
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
    fn parse_args_rejects_output_path_with_stdout_flag() {
        let args = vec![
            "audio.wav".to_string(),
            "--output".to_string(),
            "out.json".to_string(),
            "--stdout".to_string(),
        ];
        let err = parse_args(&args).unwrap_err();
        assert!(err
            .to_string()
            .contains("cannot use both --stdout and an explicit output path"));
    }

    #[test]
    fn parse_args_rejects_positional_output_path_with_output_flag() {
        let args = vec![
            "audio.wav".to_string(),
            "one.txt".to_string(),
            "--output".to_string(),
            "two.txt".to_string(),
        ];
        let err = parse_args(&args).unwrap_err();
        assert!(err
            .to_string()
            .contains("cannot use both positional output path and --output"));
    }

    #[test]
    fn parse_args_supports_speakers_output_defaulting_to_timestamps() {
        let args = vec!["audio.wav".to_string(), "--speakers".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.output_format,
            OutputFormat::Speakers(SpeakersFormat::Timestamped)
        );
    }

    #[test]
    fn parse_args_supports_plain_speakers_output() {
        let args = vec!["audio.wav".to_string(), "--speakers=plain".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.output_format,
            OutputFormat::Speakers(SpeakersFormat::Plain)
        );
    }

    #[test]
    fn parse_args_rejects_removed_script_flag() {
        let args = vec!["audio.wav".to_string(), "--script".to_string()];
        let err = parse_args(&args).unwrap_err();
        assert!(err.to_string().contains("unknown argument"));
    }

    #[test]
    fn parse_args_supports_text_output_defaulting_to_timestamps() {
        let args = vec!["audio.wav".to_string(), "--text".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.output_format,
            OutputFormat::Text(TextFormat::Timestamped)
        );
    }

    #[test]
    fn parse_args_supports_plain_text_output() {
        let args = vec!["audio.wav".to_string(), "--text=plain".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.output_format, OutputFormat::Text(TextFormat::Plain));
    }

    #[test]
    fn parse_args_supports_timestamped_text_output_alias() {
        let args = vec!["audio.wav".to_string(), "--text=timestamps".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.output_format,
            OutputFormat::Text(TextFormat::Timestamped)
        );
    }

    #[test]
    fn parse_args_supports_text_equals_syntax() {
        let args = vec!["audio.wav".to_string(), "--text=timestamps".to_string()];
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
    fn parse_args_supports_explicit_json_output() {
        let args = vec!["audio.wav".to_string(), "--json".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.output_format, OutputFormat::Json);
    }

    #[test]
    fn parse_args_rejects_multiple_output_modes() {
        let args = vec![
            "audio.wav".to_string(),
            "--text".to_string(),
            "--json".to_string(),
        ];
        let err = parse_args(&args).unwrap_err();
        assert!(err
            .to_string()
            .contains("output format flags are mutually exclusive"));
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
    fn parse_args_supports_local_short_flag() {
        let args = vec![
            "https://www.youtube.com/watch?v=QSdh8Gj0mEg".to_string(),
            "-l".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert!(parsed.force_local_for_remote);
    }

    #[test]
    fn parse_args_supports_backend_none() {
        let args = vec!["audio.wav".to_string(), "--backend=none".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.diarization, None);
    }

    #[test]
    fn parse_args_supports_backend_lseend() {
        let args = vec![
            "audio.wav".to_string(),
            "--backend".to_string(),
            "lseend-dihard3".to_string(),
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
    fn parse_args_supports_num_speakers_long_flag() {
        let args = vec![
            "audio.wav".to_string(),
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
    fn parse_args_rejects_num_speakers_with_backend_none() {
        let args = vec![
            "audio.wav".to_string(),
            "--backend=none".to_string(),
            "--num-speakers".to_string(),
            "2".to_string(),
        ];
        let err = parse_args(&args).unwrap_err();
        assert!(err
            .to_string()
            .contains("--num-speakers requires diarization"));
    }

    #[test]
    fn parse_args_supports_threshold_short_flag_with_lseend() {
        let args = vec![
            "audio.wav".to_string(),
            "--backend=lseend-dihard3".to_string(),
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
    fn parse_args_allows_overriding_default_lseend_threshold() {
        let args = vec![
            "audio.wav".to_string(),
            "--backend=lseend-dihard3".to_string(),
            "--threshold".to_string(),
            "0.45".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.diarization,
            Some(DiarizationRequest {
                backend: DiarizationBackend::LseendDihard3,
                num_speakers: None,
                threshold: Some(0.45),
            })
        );
    }

    #[test]
    fn parse_args_rejects_threshold_without_lseend_backend() {
        let args = vec![
            "audio.wav".to_string(),
            "--threshold".to_string(),
            "0.3".to_string(),
        ];
        let err = parse_args(&args).unwrap_err();
        assert!(err
            .to_string()
            .contains("--threshold only works with --backend=lseend-dihard3"));
    }

    #[test]
    fn parse_args_rejects_num_speakers_with_lseend_backend() {
        let args = vec![
            "audio.wav".to_string(),
            "--backend=lseend-dihard3".to_string(),
            "--num-speakers".to_string(),
            "2".to_string(),
        ];
        let err = parse_args(&args).unwrap_err();
        assert!(err
            .to_string()
            .contains("--num-speakers is not supported with --backend=lseend-dihard3"));
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
            "--chunk".to_string(),
            "0".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.chunk_seconds, None);
        assert_eq!(parsed.chunk_overlap_seconds, 0.0);
    }
}
