use anyhow::{bail, Context, Result};
use clap::{Arg, ArgAction, ArgMatches, Command};
use std::env;
use std::path::PathBuf;

use crate::diarization::{
    fluidaudio_binary_status, missing_diarization_notice, DiarizationBackend, DiarizationRequest,
    FluidaudioBinaryStatus,
};
use crate::model::{default_model_dir, default_model_package, ModelConfig};
use crate::types::{CliArgs, OutputFormat, SpeakersFormat, SubtitleFormat, TextFormat};

const DEFAULT_LSEEND_THRESHOLD: f64 = 0.3;

pub(crate) fn introduction() -> &'static str {
    "fscript — transcribe local audio/video and remote media URLs.\n\nUsage:\n  fscript <media-or-url> [destination] [options]\n\nExamples:\n  fscript lecture.mp3\n  fscript lecture.mp3 --text=plain --stdout\n\nRun fscript --help for all options and defaults."
}

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
        "Examples:".to_string(),
        "  fscript lecture.mp3".to_string(),
        "  fscript lecture.mp3 notes/".to_string(),
        "  cat lecture.wav | fscript - --text=plain".to_string(),
        "  fscript lecture.mp3 --text=plain --stdout".to_string(),
        "  fscript lecture.mp3 --model parakeet-ultra".to_string(),
        "  fscript lecture.mp3 --diarize lseend-dihard3".to_string(),
        "  fscript lecture.mp3 -D --json --raw".to_string(),
        String::new(),
        "Default:".to_string(),
        "  fscript <media-or-url>".to_string(),
        "    Default flags: --speakers=timestamps --diarize coreml --clean --chunk 120 --overlap 2"
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
            "--text[=plain|compact|timestamps]",
            "Transcript text; text modes never run diarization.",
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
        option("-o, --output PATH", "Explicit output path; - writes to stdout."),
        option("--stdout, -", "Write transcript contents to stdout."),
        option("--raw", "Disable repeated-word cleanup for this run."),
        option("-c, --clean", "Force cleaned output for this run."),
        String::new(),
        "Diarization:".to_string(),
        option(
            "--diarize [coreml|lseend-dihard3]",
            "Enable diarization. Legacy alias: -d.",
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
        "Input:".to_string(),
        option("- (as input)", "Read piped media; defaults to transcript stdout."),
        String::new(),
        "Remote input:".to_string(),
        option(
            "-l, --local, --prefer-local-for-remote",
            "Always download audio and transcribe locally.",
        ),
        String::new(),
        "Chunking and model selection:".to_string(),
        option(
            "--chunk N, --chunk-seconds N",
            "Chunk length in seconds. Use 0 to disable.",
        ),
        option(
            "--overlap N, --chunk-overlap-seconds N",
            "Chunk overlap in seconds.",
        ),
        option(
            "--model NAME|URL",
            "Choose a predefined model name or a custom ONNX bundle URL.",
        ),
        option("--set-default-model NAME", "Save the default model for future runs."),
        option("--get-default-model", "Show the saved default model."),
        option("--reset-default-model", "Clear the saved model preference."),
        option("--list-models", "List models and recommendations from our benchmarks."),
        option("--device auto|cpu|mps|mlx|cuda", "Choose a device; defaults to auto for the selected model."),
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
        option("Model", "Saved preference, otherwise parakeet-v3-int8."),
        option("Model dir", &display_help_path(&default_model_dir)),
        option("Model package", &display_help_path(&default_model_package)),
        String::new(),
        "Help:".to_string(),
        option("-h, --help; help", "Show full help."),
        option("-V, --version", "Show the installed version."),
        String::new(),
        "Documentation: https://github.com/brenorb/fast-transcript#readme".to_string(),
        "Report issues: https://github.com/brenorb/fast-transcript/issues".to_string(),
    ]
    .join("\n")
}

pub(crate) fn version_string() -> String {
    format!("fscript {}", env!("CARGO_PKG_VERSION"))
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

// Optional values accept the existing spaced syntax, while a following input path
// stays positional. Clap handles tokenization, `--`, aliases, and spelling hints.
pub(crate) fn command() -> Command {
    let mut command = Command::new("fscript")
        .disable_help_flag(true)
        .disable_version_flag(true)
        .color(clap::ColorChoice::Never)
        .args_override_self(true)
        .arg(Arg::new("input").value_name("MEDIA_OR_URL"))
        .arg(Arg::new("destination").value_name("DESTINATION"));
    for (name, short) in [
        ("stdout", None),
        ("json", None),
        ("srt", None),
        ("vtt", None),
        ("raw", None),
        ("clean", Some('c')),
        ("no-diarization", Some('D')),
        ("local", Some('l')),
        ("help", Some('h')),
        ("version", Some('V')),
        ("list-models", None),
        ("get-default-model", None),
        ("reset-default-model", None),
    ] {
        let mut arg = Arg::new(name).long(name).action(ArgAction::SetTrue);
        if let Some(short) = short {
            arg = arg.short(short);
        }
        if name == "local" {
            arg = arg.alias("prefer-local-for-remote");
        }
        command = command.arg(arg);
    }
    for (name, short) in [
        ("output", Some('o')),
        ("model", None),
        ("model-url", None),
        ("model-dir", None),
        ("model-package", None),
        ("device", None),
        ("backend", None),
        ("num-speakers", Some('n')),
        ("threshold", Some('t')),
        ("chunk", None),
        ("overlap", None),
        ("set-default-model", None),
    ] {
        let mut arg = Arg::new(name)
            .long(name)
            .action(ArgAction::Append)
            .num_args(1);
        if let Some(short) = short {
            arg = arg.short(short);
        }
        if name == "chunk" {
            arg = arg.alias("chunk-seconds");
        }
        if name == "overlap" {
            arg = arg.alias("chunk-overlap-seconds");
        }
        if ["chunk", "overlap", "threshold"].contains(&name) {
            arg = arg.allow_negative_numbers(true);
        }
        command = command.arg(arg);
    }
    for (name, default) in [
        ("text", "timestamps"),
        ("speakers", "timestamps"),
        ("diarize", "coreml"),
    ] {
        let mut arg = Arg::new(name)
            .long(name)
            .num_args(0..=1)
            .require_equals(true)
            .default_missing_value(default)
            .action(ArgAction::Append);
        if name == "diarize" {
            arg = arg.short('d');
        }
        command = command.arg(arg);
    }
    command
}

fn optional_values(flag: &str) -> Option<(&'static str, &'static [&'static str])> {
    match flag {
        "--text" => Some(("--text", &["plain", "compact", "timestamps"])),
        "--speakers" => Some(("--speakers", &["plain", "timestamps"])),
        "--diarize" | "-d" => Some(("--diarize", &["coreml", "lseend-dihard3", "none"])),
        _ => None,
    }
}

pub(crate) fn parse_command(raw_args: &[String]) -> Result<ArgMatches> {
    let mut normalized = Vec::with_capacity(raw_args.len() + 1);
    normalized.push("fscript".to_string());
    let mut index = 0;
    while index < raw_args.len() {
        let token = &raw_args[index];
        if token == "--" {
            normalized.extend_from_slice(&raw_args[index..]);
            break;
        }
        if let (Some((flag, values)), Some(next)) =
            (optional_values(token), raw_args.get(index + 1))
        {
            if values.contains(&next.as_str()) {
                normalized.push(format!("{flag}={next}"));
                index += 2;
                continue;
            }
        }
        normalized.push(token.clone());
        index += 1;
    }
    let matches = command()
        .try_get_matches_from(normalized)
        .map_err(|error| {
            use clap::error::{ContextKind, ErrorKind};
            let invalid = error
                .get(ContextKind::InvalidArg)
                .map(ToString::to_string)
                .unwrap_or_default();
            let message = match error.kind() {
                ErrorKind::UnknownArgument => format!(
                    "unknown argument {invalid:?}\n{}",
                    error
                        .to_string()
                        .split_once('\n')
                        .map_or("", |(_, detail)| detail)
                ),
                ErrorKind::InvalidValue
                    if error
                        .get(ContextKind::InvalidValue)
                        .is_some_and(|v| v.to_string().is_empty()) =>
                {
                    format!(
                        "missing value for {}; run fscript --help for options",
                        invalid.split_whitespace().next().unwrap_or(&invalid)
                    )
                }
                _ => error.to_string(),
            };
            anyhow::anyhow!(message)
        })?;
    if let Some(input) = matches.get_one::<String>("input") {
        let input_index = raw_args
            .iter()
            .position(|token| token == input)
            .unwrap_or(usize::MAX);
        for (index, pair) in raw_args
            .windows(2)
            .enumerate()
            .take_while(|(_, pair)| pair[0] != "--")
        {
            if let Some((flag, values)) = optional_values(&pair[0]) {
                if index > input_index
                    && !pair[1].starts_with('-')
                    && !values.contains(&pair[1].as_str())
                {
                    bail!("invalid {flag} value {:?}; expected {}. If you meant an output path, pass it before {flag} or use --output.", pair[1], match flag { "--text" => "plain, compact, or timestamps", "--speakers" => "plain or timestamps", _ => "coreml or lseend-dihard3" });
                }
            }
        }
    }
    Ok(matches)
}

fn values(matches: &ArgMatches, name: &str) -> Vec<String> {
    matches
        .get_many::<String>(name)
        .map(|values| values.cloned().collect())
        .unwrap_or_default()
}

fn last_value(matches: &ArgMatches, name: &str) -> Option<String> {
    values(matches, name).pop()
}

fn number(matches: &ArgMatches, name: &str) -> Result<Option<f64>> {
    last_value(matches, name)
        .map(|value| {
            let parsed: f64 = value
                .parse()
                .with_context(|| format!("invalid --{name} value {value:?}; expected a number"))?;
            if !parsed.is_finite() {
                bail!("--{name} must be finite");
            }
            Ok(parsed)
        })
        .transpose()
}

pub(crate) fn parse_args(raw_args: &[String]) -> Result<CliArgs> {
    parse_args_with_default_model(raw_args, || Ok(None))
}

pub(crate) fn parse_args_with_default_model(
    raw_args: &[String],
    default_model: impl FnOnce() -> Result<Option<String>>,
) -> Result<CliArgs> {
    parse_args_with_diarization_status_and_default(
        raw_args,
        fluidaudio_binary_status(),
        default_model,
    )
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

#[cfg(test)]
fn parse_args_with_diarization_status(
    raw_args: &[String],
    fluidaudio_status: FluidaudioBinaryStatus,
) -> Result<CliArgs> {
    parse_args_with_diarization_status_and_default(raw_args, fluidaudio_status, || Ok(None))
}

fn parse_args_with_diarization_status_and_default(
    raw_args: &[String],
    fluidaudio_status: FluidaudioBinaryStatus,
    default_model: impl FnOnce() -> Result<Option<String>>,
) -> Result<CliArgs> {
    let matches = parse_command(raw_args)?;
    let fluidaudio_available = matches!(fluidaudio_status, FluidaudioBinaryStatus::Available);
    let input = matches
        .get_one::<String>("input")
        .cloned()
        .context("missing input path; run fscript --help for examples")?;
    let model_dir_override = last_value(&matches, "model-dir").map(PathBuf::from);
    let model_package_override = last_value(&matches, "model-package").map(PathBuf::from);
    let models = values(&matches, "model");
    let urls = values(&matches, "model-url");
    if models.len() + urls.len() > 1 {
        bail!("choose a single model with --model or --model-url");
    }
    if let Some(url) = urls.last() {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            bail!("--model-url requires an http(s) bundle URL; use --model for predefined names");
        }
    }
    let model_url_override = models.into_iter().chain(urls).next();
    let model_device = last_value(&matches, "device").unwrap_or_else(|| "auto".to_string());
    let explicit_paths = values(&matches, "output");
    let positional_path = matches.get_one::<String>("destination");
    if explicit_paths.len() > 1 {
        bail!("choose a single --output path");
    }
    if positional_path.is_some() && !explicit_paths.is_empty() {
        bail!("cannot use both positional output path and --output for output path; choose one and remove the other");
    }
    let output_to_stdout = matches.get_flag("stdout")
        || positional_path.is_some_and(|path| path == "-")
        || explicit_paths.last().is_some_and(|path| path == "-");
    let output_path = explicit_paths
        .last()
        .or(positional_path)
        .filter(|path| path.as_str() != "-")
        .map(PathBuf::from);
    if output_to_stdout && output_path.is_some() {
        bail!("cannot use both --stdout and an explicit output path; remove one");
    }
    let formats = ["text", "speakers", "json", "srt", "vtt"]
        .into_iter()
        .filter(|name| {
            matches.contains_id(name)
                && match *name {
                    "json" | "srt" | "vtt" => matches.get_flag(name),
                    _ => true,
                }
        })
        .collect::<Vec<_>>();
    if formats.len() > 1 {
        bail!("output format flags are mutually exclusive; choose one");
    }
    let output_format = match formats.first().copied() {
        Some("text") => OutputFormat::Text(
            TextFormat::from_cli_value(&last_value(&matches, "text").unwrap())
                .context("invalid --text value; expected plain, compact, or timestamps")?,
        ),
        Some("speakers") => OutputFormat::Speakers(
            SpeakersFormat::from_cli_value(&last_value(&matches, "speakers").unwrap())
                .context("invalid --speakers value; expected plain or timestamps")?,
        ),
        Some("json") => OutputFormat::Json,
        Some("srt") => OutputFormat::Subtitle(SubtitleFormat::Srt),
        Some("vtt") => OutputFormat::Subtitle(SubtitleFormat::Vtt),
        _ => OutputFormat::Speakers(SpeakersFormat::Timestamped),
    };
    if matches.get_flag("raw") && matches.get_flag("clean") {
        bail!("cannot use both --raw and --clean; remove one");
    }
    let clean_output = !matches.get_flag("raw");
    let force_local_for_remote = matches.get_flag("local");
    let chunk_seconds_override = number(&matches, "chunk")?;
    let chunk_overlap_seconds_override = number(&matches, "overlap")?;
    for (flag, value) in [
        ("chunk", chunk_seconds_override),
        ("overlap", chunk_overlap_seconds_override),
    ] {
        if value.is_some_and(|n| n < 0.0) {
            bail!("--{flag} must be >= 0");
        }
    }
    let mut diarization_backend = None;
    let mut diarization_backend_explicit = false;
    let mut diarization_selection_source = None;
    for backend in values(&matches, "backend") {
        set_explicit_diarization_backend(
            &mut diarization_backend,
            &mut diarization_backend_explicit,
            &mut diarization_selection_source,
            "--backend",
            parse_backend_value(&backend)?,
        )?;
    }
    for backend in values(&matches, "diarize") {
        if backend == "none" {
            bail!("use --no-diarization or -D instead of `--diarize none`");
        }
        set_explicit_diarization_backend(
            &mut diarization_backend,
            &mut diarization_backend_explicit,
            &mut diarization_selection_source,
            "--diarize",
            Some(parse_diarization_model_value(&backend)?),
        )?;
    }
    if matches.get_flag("no-diarization") {
        set_explicit_diarization_backend(
            &mut diarization_backend,
            &mut diarization_backend_explicit,
            &mut diarization_selection_source,
            "--no-diarization",
            None,
        )?;
    }
    let mut diarization_num_speakers = last_value(&matches, "num-speakers")
        .map(|value| {
            let count = value.parse::<usize>().with_context(|| {
                format!("invalid --num-speakers value {value:?}; expected an integer")
            })?;
            if count == 0 {
                bail!("--num-speakers must be >= 1");
            }
            Ok(count)
        })
        .transpose()?;
    let mut diarization_threshold = number(&matches, "threshold")?;
    let diarization_notice = if matches!(output_format, OutputFormat::Text(_)) {
        let ignored_diarization_flags = diarization_backend.is_some()
            || diarization_num_speakers.is_some()
            || diarization_threshold.is_some();
        diarization_backend = None;
        diarization_num_speakers = None;
        diarization_threshold = None;
        ignored_diarization_flags.then_some(
            "--text output ignores diarization flags; continuing without diarization.".to_string(),
        )
    } else {
        let notice = if diarization_backend.is_none()
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
        notice
    };

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
        model: ModelConfig::select(
            match model_url_override {
                Some(selection) => Some(selection),
                None => default_model()?,
            },
            model_dir_override,
            model_package_override,
            model_device,
        )?,
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
    use super::{parse_args, parse_args_with_diarization_availability, usage, version_string};
    use crate::diarization::{
        missing_diarization_notice, DiarizationBackend, DiarizationRequest, FluidaudioBinaryStatus,
    };
    use crate::model::{default_model_dir, default_model_package};
    use crate::types::{OutputFormat, SpeakersFormat, SubtitleFormat, TextFormat};
    use std::path::Path;
    use std::path::PathBuf;

    #[test]
    fn saved_default_is_used_and_explicit_selection_skips_loading_settings() {
        let parsed = super::parse_args_with_default_model(
            &["audio.wav".into(), "--text=plain".into()],
            || Ok(Some("ultra".into())),
        )
        .unwrap();
        assert_eq!(parsed.model.preset.unwrap().name, "parakeet-ultra");
        for flag in ["--model", "--model-url"] {
            let selection = if flag == "--model" {
                "onnx"
            } else {
                crate::DEFAULT_MODEL_URL
            };
            let parsed = super::parse_args_with_default_model(
                &["audio.wav".into(), flag.into(), selection.into()],
                || anyhow::bail!("settings must not be read for explicit selection"),
            )
            .unwrap();
            assert_eq!(parsed.model.url, crate::DEFAULT_MODEL_URL);
        }
    }

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
            &parsed.model.directory,
            &["models", crate::DEFAULT_MODEL_BASENAME]
        ));
        assert!(path_ends_with(
            &parsed.model.package,
            &[crate::DEFAULT_MODEL_PACKAGE_NAME]
        ));
    }

    #[test]
    fn parse_args_selects_model_bundle_with_one_option() {
        let args = vec![
            "audio.mp3".to_string(),
            "--model".to_string(),
            "https://models.example.test/parakeet-fr.tar.gz?download=1".to_string(),
        ];
        let parsed = parse_args_with_diarization_availability(&args, false).unwrap();

        assert_eq!(
            parsed.model.url,
            "https://models.example.test/parakeet-fr.tar.gz?download=1"
        );
        assert_ne!(parsed.model.directory, default_model_dir());
        assert_ne!(parsed.model.package, default_model_package());
    }

    #[test]
    fn selecting_same_model_bundle_uses_stable_cache_paths() {
        let args = vec![
            "audio.mp3".to_string(),
            "--model-url".to_string(),
            "https://models.example.test/parakeet-fr.tar.gz".to_string(),
        ];
        let first = parse_args_with_diarization_availability(&args, false).unwrap();
        let second = parse_args_with_diarization_availability(&args, false).unwrap();

        assert_eq!(first.model.directory, second.model.directory);
        assert_eq!(first.model.package, second.model.package);
    }

    #[test]
    fn parse_args_accepts_benchmarked_model_names() {
        for name in [
            "parakeet-v3-int8",
            "parakeet-redux",
            "parakeet-ultra",
            "phonon-2",
        ] {
            let args = vec!["speech.wav".into(), "--model".into(), name.into()];
            let parsed = parse_args_with_diarization_availability(&args, false).unwrap();
            assert!(
                parsed.model.url.starts_with("https://"),
                "{name} must resolve to its predefined source"
            );
        }
    }

    #[test]
    fn parse_args_unknown_model_name_has_discovery_hint() {
        let args = vec!["speech.wav".into(), "--model".into(), "typo-model".into()];
        let error = parse_args_with_diarization_availability(&args, false).unwrap_err();
        assert!(error.to_string().contains("unknown model"));
        assert!(error.to_string().contains("--list-models"));
    }

    #[test]
    fn parse_args_preserves_named_aliases_and_rejects_incompatible_model_options() {
        let args = vec![
            "speech.wav".into(),
            "--model=ultra".into(),
            "--device=cpu".into(),
        ];
        let parsed = parse_args_with_diarization_availability(&args, false).unwrap();
        assert_eq!(parsed.model.preset.unwrap().name, "parakeet-ultra");
        assert_eq!(parsed.model.device, "cpu");

        for (options, diagnostic) in [
            (
                vec![
                    "--model",
                    "ultra",
                    "--model-url",
                    "https://example.test/model.tar.gz",
                ],
                "single model",
            ),
            (vec!["--model-url", "ultra"], "http(s)"),
            (
                vec!["--model", "ultra", "--model-package", "weights.tar.gz"],
                "only applies to ONNX",
            ),
            (
                vec!["--model", "phonon-2", "--device", "mps"],
                "not supported",
            ),
            (vec!["--model", "onnx", "--device", "mlx"], "not supported"),
        ] {
            let mut args = vec!["speech.wav".to_string()];
            args.extend(options.into_iter().map(str::to_string));
            let error = parse_args_with_diarization_availability(&args, false).unwrap_err();
            assert!(error.to_string().contains(diagnostic), "{error}");
        }
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
            "Default flags: --speakers=timestamps --diarize coreml --clean --chunk 120 --overlap 2"
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
        assert!(help.contains("--diarize [coreml|lseend-dihard3]"));
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
        assert_eq!(parsed.model.directory, PathBuf::from("custom-model"));
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
    fn parse_args_rejects_flag_token_as_output_value() {
        let args = vec![
            "audio.wav".to_string(),
            "--output".to_string(),
            "--json".to_string(),
        ];
        let err = parse_args(&args).unwrap_err();
        assert!(err.to_string().contains("missing value for --output"));
    }

    #[test]
    fn parse_args_rejects_flag_token_as_model_dir_value() {
        let args = vec![
            "audio.wav".to_string(),
            "--model-dir".to_string(),
            "--text".to_string(),
        ];
        let err = parse_args(&args).unwrap_err();
        assert!(err.to_string().contains("missing value for --model-dir"));
    }

    #[test]
    fn parse_args_rejects_flag_token_as_model_package_value() {
        let args = vec![
            "audio.wav".to_string(),
            "--model-package".to_string(),
            "--json".to_string(),
        ];
        let err = parse_args(&args).unwrap_err();
        assert!(err
            .to_string()
            .contains("missing value for --model-package"));
    }

    #[test]
    fn parse_args_rejects_flag_token_as_model_url_value() {
        let args = vec![
            "audio.wav".to_string(),
            "--model-url".to_string(),
            "--json".to_string(),
        ];
        let err = parse_args(&args).unwrap_err();
        assert!(err.to_string().contains("missing value for --model-url"));
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
    fn parse_args_rejects_invalid_spaced_speakers_value_after_input() {
        let args = vec![
            "audio.wav".to_string(),
            "--speakers".to_string(),
            "banana".to_string(),
        ];
        let err = parse_args(&args).unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid --speakers value \"banana\"; expected plain or timestamps"));
        assert!(err.to_string().contains("If you meant an output path"));
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
        let output_modes: &[&[&str]] = &[
            &[],
            &["--json"],
            &["--text=plain"],
            &["--text=compact"],
            &["--speakers=plain"],
        ];
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
    fn parse_args_supports_compact_text_output() {
        let args = vec!["audio.wav".to_string(), "--text=compact".to_string()];
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.output_format,
            OutputFormat::Text(TextFormat::Compact)
        );
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
    fn parse_args_rejects_invalid_spaced_text_value_after_input() {
        let args = vec![
            "audio.wav".to_string(),
            "--text".to_string(),
            "banana".to_string(),
        ];
        let err = parse_args(&args).unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid --text value \"banana\"; expected plain, compact, or timestamps"));
        assert!(err.to_string().contains("If you meant an output path"));
    }

    #[test]
    fn parse_args_text_output_disables_default_diarization() {
        let args = vec!["audio.wav".to_string(), "--text=plain".to_string()];
        let parsed = parse_args_with_diarization_availability(&args, true).unwrap();
        assert_eq!(parsed.diarization, None);
        assert_eq!(parsed.diarization_notice, None);
    }

    #[test]
    fn parse_args_text_output_ignores_explicit_diarization_flags() {
        let args = vec![
            "audio.wav".to_string(),
            "--text=plain".to_string(),
            "--diarize".to_string(),
            "lseend-dihard3".to_string(),
            "--num-speakers".to_string(),
            "2".to_string(),
            "--threshold".to_string(),
            "0.45".to_string(),
        ];
        let parsed = parse_args_with_diarization_availability(&args, true).unwrap();
        assert_eq!(parsed.diarization, None);
        assert_eq!(
            parsed.diarization_notice.as_deref(),
            Some("--text output ignores diarization flags; continuing without diarization.")
        );
    }

    #[test]
    fn parse_args_text_output_skips_missing_helper_notice() {
        let args = vec!["audio.wav".to_string(), "--text".to_string()];
        let parsed = parse_args_with_diarization_availability(&args, false).unwrap();
        assert_eq!(parsed.diarization, None);
        assert_eq!(parsed.diarization_notice, None);
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
    fn parse_args_rejects_invalid_spaced_diarize_value_after_input() {
        let args = vec![
            "audio.wav".to_string(),
            "--diarize".to_string(),
            "banana".to_string(),
        ];
        let err = parse_args(&args).unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid --diarize value \"banana\"; expected coreml or lseend-dihard3"));
        assert!(err.to_string().contains("If you meant an output path"));
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
