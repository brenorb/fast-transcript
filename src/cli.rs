use anyhow::{bail, Context, Result};
use directories::ProjectDirs;
use std::env;
use std::path::PathBuf;

use crate::diarization::{
    fluidaudio_binary_status, missing_diarization_notice, DiarizationBackend, DiarizationRequest,
    FluidaudioBinaryStatus,
};
use crate::types::{CliArgs, OutputFormat, SpeakersFormat, SubtitleFormat, TextFormat};

const DEFAULT_LSEEND_THRESHOLD: f64 = 0.3;

pub(crate) fn usage() -> String {
    fn option(name: &str, description: &str) -> String {
        format!("  {name:<44} {description}")
    }

    fn display_help_path(path: &std::path::Path) -> String {
        let Some(home) = env::var_os("HOME").map(PathBuf::from) else {
            return path.display().to_string();
        };

        if let Ok(stripped) = path.strip_prefix(&home) {
            if stripped.as_os_str().is_empty() {
                "~".to_string()
            } else {
                format!("~/{}", stripped.display())
            }
        } else {
            path.display().to_string()
        }
    }

    let default_model_dir = default_model_dir();
    let default_model_package = default_model_package();

    [
        "Usage:".to_string(),
        "  fscript <media-or-url> [destination] [options]".to_string(),
        String::new(),
        "Default:".to_string(),
        "  fscript <media-or-url>".to_string(),
        "    Default flags: --speakers --diarize coreml --clean --chunk 120 --overlap 2"
            .to_string(),
        "    Accepts local audio/video files or remote media URLs; video inputs are transcribed from their audio stream."
            .to_string(),
        "    If `fluidaudiocli` is unavailable, warns and falls back to plain transcription."
            .to_string(),
        String::new(),
        "Output:".to_string(),
        option(
            "--speakers[=plain|timestamps]",
            "Speaker-aware transcript. Default output mode.",
        ),
        option(
            "--text[=plain|timestamps]",
            "Transcript text; plain omits timestamps.",
        ),
        option("--json", "Full JSON result with timings and metadata."),
        option("--srt", "Experimental SubRip subtitle output."),
        option("--vtt", "Experimental WebVTT subtitle output."),
        String::new(),
        "Destination:".to_string(),
        option(
            "[destination]",
            "Output file, output directory, or `-` / `--stdout`.",
        ),
        option("-o, --output PATH", "Explicit output path."),
        option("--stdout, -", "Write transcript contents to stdout."),
        option("--raw", "Disable repeated-word cleanup for this run."),
        option("-c, --clean", "Force cleaned output for this run."),
        String::new(),
        "Diarization:".to_string(),
        option(
            "-d, --diarize [coreml|lseend-dihard3]",
            "Enable diarization, optionally choosing the model.",
        ),
        option("-D, --no-diarization", "Disable diarization entirely."),
        option(
            "-n, --num-speakers N",
            "Expected speaker count. CoreML only.",
        ),
        option(
            "-t, --threshold N",
            "Decision threshold. ls-eend-dihard3 only.",
        ),
        option(
            "--backend coreml|lseend-dihard3|none",
            "Legacy alias for older scripts.",
        ),
        String::new(),
        "Remote input:".to_string(),
        option(
            "-l, --local, --prefer-local-for-remote",
            "Always download audio and transcribe locally.",
        ),
        String::new(),
        "Chunking and model overrides:".to_string(),
        option(
            "--chunk N, --chunk-seconds N",
            "Chunk length in seconds. Use 0 to disable.",
        ),
        option(
            "--overlap N, --chunk-overlap-seconds N",
            "Chunk overlap in seconds.",
        ),
        option(
            "--model-dir PATH",
            "Use an existing extracted model directory.",
        ),
        option(
            "--model-package PATH",
            "Override the cached model tarball path.",
        ),
        option("--model-url URL", "Override the model download URL."),
        String::new(),
        "Defaults summary:".to_string(),
        option("Output mode", "--speakers with timestamps."),
        option("Diarization", "coreml when `fluidaudiocli` is available."),
        option(
            "ls-eend threshold",
            &format!("{DEFAULT_LSEEND_THRESHOLD} when selected."),
        ),
        option("Cleaning", "On."),
        option("Chunking", "--chunk 120 with --overlap 2."),
        option("Model dir", &display_help_path(&default_model_dir)),
        option("Model package", &display_help_path(&default_model_package)),
        String::new(),
        "Examples:".to_string(),
        "  fscript lecture.mp3".to_string(),
        "  fscript lecture.mp3 notes/".to_string(),
        "  fscript lecture.mp3 --text=plain".to_string(),
        "  fscript lecture.mp3 --diarize lseend-dihard3".to_string(),
        "  fscript lecture.mp3 -D --json --raw".to_string(),
    ]
    .join("\n")
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

fn parse_diarization_model_value(value: &str) -> Result<DiarizationBackend> {
    DiarizationBackend::from_cli_value(value).with_context(|| {
        format!("invalid diarization mode {value:?}; expected coreml or lseend-dihard3")
    })
}

fn parse_backend_value(value: &str) -> Result<Option<DiarizationBackend>> {
    match value {
        "none" => Ok(None),
        value => parse_diarization_model_value(value).map(Some),
    }
}

fn set_explicit_diarization_backend(
    diarization_backend: &mut Option<DiarizationBackend>,
    diarization_backend_explicit: &mut bool,
    diarization_selection_source: &mut Option<&'static str>,
    source_name: &'static str,
    requested_backend: Option<DiarizationBackend>,
) -> Result<()> {
    if let Some(previous_source) = *diarization_selection_source {
        if *diarization_backend != requested_backend {
            if (*diarization_backend).is_some() != requested_backend.is_some() {
                bail!("cannot combine --diarize with --no-diarization; choose one");
            }
            bail!(
                "cannot combine multiple diarization selections ({previous_source} and {source_name}); choose one"
            );
        }
        return Ok(());
    }

    *diarization_backend_explicit = true;
    *diarization_selection_source = Some(source_name);
    *diarization_backend = requested_backend;
    Ok(())
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
    parse_args_with_diarization_status(raw_args, fluidaudio_binary_status())
}

#[cfg(test)]
fn parse_args_with_diarization_availability(
    raw_args: &[String],
    fluidaudio_available: bool,
) -> Result<CliArgs> {
    let status = if fluidaudio_available {
        FluidaudioBinaryStatus::Available
    } else {
        FluidaudioBinaryStatus::MissingDefaultBinary
    };
    parse_args_with_diarization_status(raw_args, status)
}

fn parse_args_with_diarization_status(
    raw_args: &[String],
    fluidaudio_status: FluidaudioBinaryStatus,
) -> Result<CliArgs> {
    if raw_args.is_empty() {
        bail!("{}", usage());
    }

    let fluidaudio_available = matches!(fluidaudio_status, FluidaudioBinaryStatus::Available);

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
    let mut diarization_backend = None;
    let mut diarization_backend_explicit = false;
    let mut diarization_selection_source = None;
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
                set_explicit_diarization_backend(
                    &mut diarization_backend,
                    &mut diarization_backend_explicit,
                    &mut diarization_selection_source,
                    "--backend",
                    parse_backend_value(value)?,
                )?;
                index += 2;
            }
            flag if flag.starts_with("--backend=") => {
                let value = flag
                    .split_once('=')
                    .map(|(_, value)| value)
                    .with_context(|| format!("missing value for --backend\n{}", usage()))?;
                set_explicit_diarization_backend(
                    &mut diarization_backend,
                    &mut diarization_backend_explicit,
                    &mut diarization_selection_source,
                    "--backend",
                    parse_backend_value(value)?,
                )?;
                index += 1;
            }
            "-d" | "--diarize" => {
                let mut requested_backend = Some(DiarizationBackend::Coreml);
                if let Some(value) = raw_args.get(index + 1) {
                    if value == "none" {
                        bail!("use --no-diarization or -D instead of `--diarize none`");
                    }
                    if let Ok(parsed_backend) = parse_diarization_model_value(value) {
                        requested_backend = Some(parsed_backend);
                        set_explicit_diarization_backend(
                            &mut diarization_backend,
                            &mut diarization_backend_explicit,
                            &mut diarization_selection_source,
                            "--diarize",
                            requested_backend,
                        )?;
                        index += 2;
                        continue;
                    }
                }
                set_explicit_diarization_backend(
                    &mut diarization_backend,
                    &mut diarization_backend_explicit,
                    &mut diarization_selection_source,
                    "--diarize",
                    requested_backend,
                )?;
                index += 1;
            }
            flag if flag.starts_with("--diarize=") => {
                let value = flag
                    .split_once('=')
                    .map(|(_, value)| value)
                    .with_context(|| format!("missing value for --diarize\n{}", usage()))?;
                if value == "none" {
                    bail!("use --no-diarization or -D instead of `--diarize none`");
                }
                set_explicit_diarization_backend(
                    &mut diarization_backend,
                    &mut diarization_backend_explicit,
                    &mut diarization_selection_source,
                    "--diarize",
                    Some(parse_diarization_model_value(value)?),
                )?;
                index += 1;
            }
            "-D" | "--no-diarization" => {
                set_explicit_diarization_backend(
                    &mut diarization_backend,
                    &mut diarization_backend_explicit,
                    &mut diarization_selection_source,
                    "--no-diarization",
                    None,
                )?;
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
            flag if flag.starts_with('-') && flag != "-" => {
                bail!(
                    "unknown argument {flag:?}; if you meant a path starting with `-`, prefix it with `./`\n{}",
                    usage()
                )
            }
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

    let input = input.with_context(|| format!("missing input path\n{}", usage()))?;
    let output_path = if output_to_stdout { None } else { output_path };
    let diarization_notice = if diarization_backend.is_none()
        && !diarization_backend_explicit
        && !fluidaudio_available
    {
        missing_diarization_notice(&fluidaudio_status)
    } else {
        None
    };
    if diarization_backend.is_none() && !diarization_backend_explicit && fluidaudio_available {
        diarization_backend = Some(DiarizationBackend::Coreml);
    }
    if diarization_backend.is_none() && diarization_num_speakers.is_some() {
        bail!("--num-speakers requires diarization; remove --num-speakers or use --diarize");
    }
    if diarization_backend.is_none() && diarization_threshold.is_some() {
        bail!(
            "--threshold requires diarization; remove --threshold or use `--diarize lseend-dihard3`"
        );
    }
    if diarization_num_speakers.is_some()
        && diarization_backend == Some(DiarizationBackend::LseendDihard3)
    {
        bail!(
            "--num-speakers is not supported with `--diarize lseend-dihard3`; remove --num-speakers or switch to coreml"
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
            "--threshold only works with `--diarize lseend-dihard3`; remove --threshold or switch diarization modes"
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
        diarization_notice,
        diarization: diarization_backend.map(|backend| DiarizationRequest {
            backend,
            num_speakers: diarization_num_speakers,
            threshold: diarization_threshold,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        default_model_dir, default_model_package, parse_args,
        parse_args_with_diarization_availability, usage, version_string,
    };
    use crate::diarization::{
        missing_diarization_notice, DiarizationBackend, DiarizationRequest, FluidaudioBinaryStatus,
    };
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

    fn lcg_next(state: &mut u64) -> u64 {
        *state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *state
    }

    fn choose_slice<'a>(state: &mut u64, options: &'a [&'a [&'a str]]) -> &'a [&'a str] {
        let index = (lcg_next(state) as usize) % options.len();
        options[index]
    }

    #[test]
    fn parse_args_defaults_to_easy_mode() {
        let args = vec!["audio.mp3".to_string()];
        let parsed = parse_args_with_diarization_availability(&args, true).unwrap();
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
        assert_eq!(parsed.diarization_notice, None);
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
    fn usage_groups_flags_into_readable_sections() {
        let help = usage();
        assert!(help.contains("Usage:\n"));
        assert!(help.contains("  fscript <media-or-url> [destination] [options]"));
        assert!(help.contains("Default:\n"));
        assert!(help.contains(
            "Default flags: --speakers --diarize coreml --clean --chunk 120 --overlap 2"
        ));
        assert!(help.contains(
            "Accepts local audio/video files or remote media URLs; video inputs are transcribed from their audio stream."
        ));
        assert!(help.contains("Output:\n"));
        assert!(help.contains("Diarization:\n"));
        assert!(help.contains("Remote input:\n"));
        assert!(help.contains("Defaults summary:\n"));
        assert!(help.contains("Examples:\n"));
        assert!(help.contains("--speakers[=plain|timestamps]"));
        assert!(help.contains("-d, --diarize [coreml|lseend-dihard3]"));
        assert!(help.contains("-D, --no-diarization"));
        assert!(!help.contains("/Users/"));
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
    fn parse_args_rejects_unknown_short_flag_instead_of_treating_it_as_output_path() {
        let args = vec!["audio.wav".to_string(), "-Z".to_string()];
        let err = parse_args(&args).unwrap_err();
        assert!(err.to_string().contains("unknown argument \"-Z\""));
    }

    #[test]
    fn parse_args_distinguishes_short_flags_from_hyphen_prefixed_paths() {
        let stdout = parse_args(&["audio.wav".to_string(), "-".to_string()]).unwrap();
        assert!(stdout.output_to_stdout);
        assert_eq!(stdout.output_path, None);

        let no_diarization = parse_args(&["audio.wav".to_string(), "-D".to_string()]).unwrap();
        assert_eq!(no_diarization.diarization, None);
        assert_eq!(no_diarization.output_path, None);

        let explicit_hyphen_path =
            parse_args(&["audio.wav".to_string(), "./-D".to_string()]).unwrap();
        assert_eq!(
            explicit_hyphen_path.output_path,
            Some(PathBuf::from("./-D"))
        );

        let explicit_unknown_hyphen_path =
            parse_args(&["audio.wav".to_string(), "./-Z".to_string()]).unwrap();
        assert_eq!(
            explicit_unknown_hyphen_path.output_path,
            Some(PathBuf::from("./-Z"))
        );

        let err = parse_args(&["audio.wav".to_string(), "-Z".to_string()]).unwrap_err();
        assert!(err.to_string().contains("unknown argument \"-Z\""));
    }

    #[test]
    fn parse_args_fuzzes_unknown_short_flags_across_valid_contexts() {
        let output_modes: &[&[&str]] =
            &[&[], &["--json"], &["--text=plain"], &["--speakers=plain"]];
        let destinations: &[&[&str]] = &[&[], &["out.txt"], &["--output", "out.txt"], &["-"]];
        let diarization_modes: &[&[&str]] = &[&[], &["-D"], &["--diarize", "coreml"]];
        let chunking_modes: &[&[&str]] = &[&[], &["--chunk", "30", "--overlap", "1"]];

        let mut state = 0x5EED_u64;
        for _ in 0..256 {
            let invalid_flag = format!("-{}", (b'J' + (lcg_next(&mut state) % 10) as u8) as char);
            let mut groups = vec![vec!["audio.wav".to_string()]];
            for token_group in [
                choose_slice(&mut state, output_modes),
                choose_slice(&mut state, destinations),
                choose_slice(&mut state, diarization_modes),
                choose_slice(&mut state, chunking_modes),
            ] {
                if !token_group.is_empty() {
                    groups.push(
                        token_group
                            .iter()
                            .map(|token| (*token).to_string())
                            .collect(),
                    );
                }
            }

            let insert_group = 1 + (lcg_next(&mut state) as usize % groups.len());
            groups.insert(insert_group, vec![invalid_flag.clone()]);

            let args = groups.into_iter().flatten().collect::<Vec<_>>();

            let err = parse_args_with_diarization_availability(&args, true).unwrap_err();
            assert!(
                err.to_string()
                    .contains(&format!("unknown argument {:?}", invalid_flag)),
                "expected unknown short flag rejection for args {:?}, got {:?}",
                args,
                err
            );
        }
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
    fn parse_args_supports_no_diarization_long_flag() {
        let args = vec!["audio.wav".to_string(), "--no-diarization".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.diarization, None);
        assert_eq!(parsed.diarization_notice, None);
    }

    #[test]
    fn parse_args_supports_no_diarization_short_flag() {
        let args = vec!["audio.wav".to_string(), "-D".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.diarization, None);
        assert_eq!(parsed.diarization_notice, None);
    }

    #[test]
    fn parse_args_supports_bare_diarize_flag() {
        let args = vec!["audio.wav".to_string(), "--diarize".to_string()];
        let parsed = parse_args_with_diarization_availability(&args, true).unwrap();
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
    fn parse_args_supports_short_diarize_flag_with_coreml_value() {
        let args = vec![
            "audio.wav".to_string(),
            "-d".to_string(),
            "coreml".to_string(),
        ];
        let parsed = parse_args_with_diarization_availability(&args, true).unwrap();
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
    fn parse_args_supports_diarize_long_flag_with_lseend_value() {
        let args = vec![
            "audio.wav".to_string(),
            "--diarize".to_string(),
            "lseend-dihard3".to_string(),
        ];
        let parsed = parse_args_with_diarization_availability(&args, true).unwrap();
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
    fn parse_args_rejects_diarize_none_value() {
        let args = vec![
            "audio.wav".to_string(),
            "--diarize".to_string(),
            "none".to_string(),
        ];
        let err = parse_args(&args).unwrap_err();
        assert!(err
            .to_string()
            .contains("use --no-diarization or -D instead of `--diarize none`"));
    }

    #[test]
    fn parse_args_rejects_conflicting_diarization_toggles() {
        let args = vec!["audio.wav".to_string(), "-d".to_string(), "-D".to_string()];
        let err = parse_args(&args).unwrap_err();
        assert!(err
            .to_string()
            .contains("cannot combine --diarize with --no-diarization"));
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
    fn parse_args_defaults_to_no_diarization_when_helper_is_missing() {
        let args = vec!["audio.wav".to_string()];
        let parsed = parse_args_with_diarization_availability(&args, false).unwrap();
        assert_eq!(parsed.diarization, None);
        assert_eq!(
            parsed.diarization_notice.as_deref(),
            Some(
                "speaker diarization disabled because `fluidaudiocli` is not available on PATH; this installation may be incomplete. Reinstall the bundled helper or set FSCRIPT_DIARIZATION_BINARY to a working fluidaudiocli binary."
            )
        );
    }

    #[test]
    fn parse_args_uses_override_specific_notice_when_configured_binary_is_missing_on_path() {
        let notice =
            missing_diarization_notice(&FluidaudioBinaryStatus::MissingConfiguredBinaryOnPath {
                binary: "custom-fluidaudiocli".to_string(),
            });
        assert_eq!(
            notice.as_deref(),
            Some(
                "speaker diarization disabled because FSCRIPT_DIARIZATION_BINARY is set to `custom-fluidaudiocli`, but that command is not available on PATH; falling back to plain transcription."
            )
        );
    }

    #[test]
    fn parse_args_uses_override_specific_notice_when_configured_binary_path_is_invalid() {
        let notice =
            missing_diarization_notice(&FluidaudioBinaryStatus::InvalidConfiguredBinaryPath {
                binary: "/definitely/missing/fluidaudiocli".to_string(),
            });
        assert_eq!(
            notice.as_deref(),
            Some(
                "speaker diarization disabled because FSCRIPT_DIARIZATION_BINARY points to `/definitely/missing/fluidaudiocli`, but that path is not an executable file; falling back to plain transcription."
            )
        );
    }

    #[test]
    fn parse_args_keeps_explicit_coreml_request_when_helper_is_missing() {
        let args = vec!["audio.wav".to_string(), "--backend=coreml".to_string()];
        let parsed = parse_args_with_diarization_availability(&args, false).unwrap();
        assert_eq!(
            parsed.diarization,
            Some(DiarizationRequest {
                backend: DiarizationBackend::Coreml,
                num_speakers: None,
                threshold: None,
            })
        );
        assert_eq!(parsed.diarization_notice, None);
    }

    #[test]
    fn parse_args_supports_num_speakers_long_flag() {
        let args = vec![
            "audio.wav".to_string(),
            "--num-speakers".to_string(),
            "2".to_string(),
        ];
        let parsed = parse_args_with_diarization_availability(&args, true).unwrap();
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
        let err = parse_args_with_diarization_availability(&args, true).unwrap_err();
        assert!(err
            .to_string()
            .contains("--threshold only works with `--diarize lseend-dihard3`"));
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
            .contains("--num-speakers is not supported with `--diarize lseend-dihard3`"));
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
