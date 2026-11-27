//! Explicit actions under the single fscript executable; all jobs share Rust services.

use anyhow::{bail, Context, Result};
use clap::{Arg, ArgAction, ArgMatches, Command};
use std::path::PathBuf;

/// Known action names are reserved. Use ./notes or -- notes for a literal file.
/// Recognize only flags shared by both interfaces, never skip option values.
pub(crate) fn arguments(raw: &[String]) -> Option<Vec<String>> {
    let position = raw
        .iter()
        .position(|arg| !matches!(arg.as_str(), "--json" | "--quiet" | "-q"))?;
    let command = command();
    let is_action = |name: &str| {
        command
            .get_subcommands()
            .any(|action| action.get_name() == name)
    };
    if is_action(&raw[position]) {
        return Some(raw.to_vec());
    }
    if raw[position] == "help" && raw.get(position + 1).is_some_and(|arg| is_action(arg)) {
        let mut args = raw[..position].to_vec();
        args.extend_from_slice(&raw[position + 1..]);
        args.push("--help".into());
        return Some(args);
    }
    None
}

pub(crate) fn validate_arguments(raw: &[String]) -> std::result::Result<(), clap::Error> {
    matches(raw).map(|_| ())
}

#[cfg(test)]
mod routing_tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).into()).collect()
    }

    #[test]
    fn actions_share_one_entrypoint_without_consuming_media_option_values() {
        for action in ["notes", "export", "models", "agents", "skill", "record"] {
            assert_eq!(arguments(&args(&[action])), Some(args(&[action])));
            assert!(arguments(&args(&["--quiet", "--json", action])).is_some());
            assert!(arguments(&args(&["--", action])).is_none());
            assert!(arguments(&args(&["--model", action, "audio.wav"])).is_none());
            for index in 0..250 {
                assert!(arguments(&[format!("./{action}"), format!("/tmp/{index}")]).is_none());
                assert!(arguments(&[format!("{action}-{index}.wav")]).is_none());
            }
        }
        assert_eq!(
            arguments(&args(&["help", "models", "set"])),
            Some(args(&["models", "set", "--help"]))
        );
    }
}

fn command() -> Command {
    Command::new("fscript").version(env!("CARGO_PKG_VERSION"))
        .about("Transcription jobs and external-agent notes")
        .color(clap::ColorChoice::Never)
        .arg(Arg::new("quiet").long("quiet").short('q').global(true).action(ArgAction::SetTrue).help("Suppress status; keep results and errors"))
        .arg(Arg::new("json").long("json").global(true).action(ArgAction::SetTrue).help("Print machine-readable completion data"))
        .arg(Arg::new("manual").long("manual").global(true).action(ArgAction::SetTrue).help("Read the complete offline control manual"))
        .arg(Arg::new("man-page").long("man-page").action(ArgAction::SetTrue).help("Export the control manual for man(1)"))
        .subcommand(Command::new("skill").about("Print the bundled agent skill")
            .subcommand(Command::new("path").about("Print a local path to the bundled skill")))
        .subcommand(Command::new("record").about("Dictate using the default microphone")
            .after_help("Examples:\n  fscript record\n  fscript record --duration 10\n  fscript record --control --json\n  fscript record --hotkey\n\nCtrl-C finishes bare/duration capture; a second interrupt cancels. In --control/--hotkey mode, Ctrl-C cancels. Control mode reads JSONL start/stop/cancel/shutdown requests on stdin and keeps the model loaded.")
            .arg(Arg::new("duration").long("duration").conflicts_with_all(["control", "hotkey"]).help("Stop automatically after this many seconds of microphone capture"))
            .arg(Arg::new("control").long("control").action(ArgAction::SetTrue).help("Serve explicit JSONL recording requests on stdin with one warm model"))
            .arg(Arg::new("hotkey").long("hotkey").num_args(0..=1).default_missing_value("auto").value_parser(["auto", "ctrl+shift+space", "ctrl+shift+d", "ctrl+shift+period"]).help("Keep a warm global hold shortcut; auto chooses the first available chord"))
            .arg(Arg::new("output-dir").long("output-dir").help("Save each completed JSON transcript in a unique child directory"))
            .arg(Arg::new("model").long("model").help("Override the shared speech-model preference"))
            .arg(Arg::new("device").long("device").default_value("auto").help("Inference device; auto, cpu, mps, mlx, or cuda as supported by the model"))
            .arg(Arg::new("model-dir").long("model-dir").help("Existing speech-model directory")))
        .subcommand(Command::new("export").about("Export a completed transcript without transcribing again")
            .after_help("Examples:\n  fscript export meeting.json\n  fscript export meeting.json --format srt\n  fscript media.wav --stream --json | fscript export - --format plain")
            .arg(Arg::new("input").required(true).help("Complete fscript JSON/JSONL file, or - for stdin"))
            .arg(Arg::new("destination").help("Output file or directory; - writes to stdout"))
            .arg(Arg::new("output").long("output").short('o').conflicts_with("destination").help("Explicit output file or directory"))
            .arg(Arg::new("stdout").long("stdout").action(ArgAction::SetTrue).conflicts_with_all(["output","destination"]).help("Print the export without saving"))
            .arg(Arg::new("copy").long("copy").action(ArgAction::SetTrue).conflicts_with_all(["output", "destination", "paste"]).help("Copy the rendered text instead of saving another file"))
            .arg(Arg::new("paste").long("paste").action(ArgAction::SetTrue).requires("target-pid").conflicts_with_all(["output", "destination"]).help("Copy and paste into an explicit target app; requires macOS Accessibility permission"))
            .arg(Arg::new("target-pid").long("target-pid").requires("paste").value_parser(clap::value_parser!(i32).range(1..)).help("Process ID of the application receiving paste"))
            .arg(Arg::new("format").long("format").default_value("text").value_parser(["text","plain","compact","speakers","srt","vtt","json","markdown"]).help("Completed document format; text includes timestamps")))
        .subcommand(Command::new("models").about("Inspect models and the shared saved preference")
            .subcommand(Command::new("get").about("Show the effective model and preference sources"))
            .subcommand(Command::new("set").about("Save a predefined model or alias").arg(Arg::new("name").required(true)))
            .subcommand(Command::new("reset").about("Clear the saved preference; leave cached models untouched")))
        .subcommand(Command::new("notes").about("Create notes using an installed headless agent")
            .after_help("Examples:\n  fscript notes meeting.txt\n  fscript notes meeting.json --agent claude\n  fscript media.wav --stream --json | fscript notes -\n  fscript notes meeting.txt --stdout --json")
            .arg(Arg::new("input").required(true).help("UTF-8 transcript file, fscript JSON/JSONL, or - for stdin"))
            .arg(Arg::new("destination").help("Notes file or directory; - writes notes to stdout"))
            .arg(Arg::new("output").long("output").short('o').conflicts_with("destination").help("Explicit notes file or directory"))
            .arg(Arg::new("stdout").long("stdout").action(ArgAction::SetTrue).conflicts_with_all(["output", "destination"]).help("Print notes without saving a file"))
            .arg(Arg::new("agent").long("agent").help("codex, claude, omp, auto, or a saved custom command"))
            .arg(Arg::new("model").long("model").help("Override the built-in agent's language model"))
            .arg(Arg::new("input-format").long("input-format").default_value("auto").value_parser(["auto", "text", "json", "jsonl"]).help("Transcript input format"))
            .arg(Arg::new("timeout").long("timeout").default_value("300").help("Agent execution timeout in seconds")))
        .subcommand(Command::new("agents").about("Inspect or choose headless note agents")
            .subcommand(Command::new("set").about("Save the default agent; auto restores automatic detection").arg(Arg::new("name").required(true)))
            .subcommand(Command::new("add").about("Save a custom stdin-to-Markdown command")
                .after_help("Example: fscript agents add local -- /absolute/path/to/agent --non-interactive")
                .arg(Arg::new("name").required(true)).arg(Arg::new("command").required(true).last(true).num_args(1..)))
            .subcommand(Command::new("remove").about("Remove a saved custom command").arg(Arg::new("name").required(true)))
            .subcommand(Command::new("reset").about("Reset note-agent settings; leave models and agent logins untouched")))
}

fn matches(raw: &[String]) -> std::result::Result<ArgMatches, clap::Error> {
    command().try_get_matches_from(std::iter::once("fscript".to_owned()).chain(raw.iter().cloned()))
}

pub(crate) fn needs_supervision(raw: &[String]) -> bool {
    matches(raw).is_ok_and(|matches| {
        (matches!(matches.subcommand_name(), Some("notes" | "record"))
            || matches.subcommand().is_some_and(|(name, values)| {
                name == "export" && (values.get_flag("copy") || values.get_flag("paste"))
            }))
            && !matches.get_flag("manual")
    })
}

pub(crate) fn finish_on_interrupt(raw: &[String]) -> bool {
    matches(raw).is_ok_and(|matches| {
        matches.subcommand().is_some_and(|(name, values)| {
            name == "record"
                && !values.get_flag("control")
                && values.get_one::<String>("hotkey").is_none()
        })
    })
}

pub(crate) fn run(raw: Vec<String>) -> Result<()> {
    let matches = match matches(&raw) {
        Ok(matches) => matches,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayVersion
                    | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            ) =>
        {
            return crate::delivery::write_stdout(
                &mut std::io::stdout().lock(),
                format_args!("{error}"),
            );
        }
        Err(error) => bail!("{}", error.to_string().trim_start_matches("error: ")),
    };
    let _scope = crate::diagnostics::Scope::enter(matches.get_flag("quiet"));
    if matches.get_flag("manual") {
        return print(&crate::cli::manual());
    }
    if matches.get_flag("man-page") {
        return print(include_str!("../docs/fscript.1"));
    }
    let json = matches.get_flag("json");
    match matches.subcommand() {
        Some(("skill", values)) => {
            let content = include_str!("../skills/fast-transcript/SKILL.md");
            let path = if values.subcommand_name() == Some("path") {
                let directory = std::env::var_os("FSCRIPT_SKILL_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        crate::settings::config_directory().join("skills/fast-transcript")
                    });
                let target = directory.join("SKILL.md");
                if std::fs::read_to_string(&target).ok().as_deref() != Some(content) {
                    crate::delivery::write_output_file(
                        content.strip_suffix('\n').unwrap_or(content),
                        &target,
                    )?;
                }
                Some(std::path::absolute(target)?)
            } else {
                None
            };
            if json {
                print(&format!(
                    "{}\n",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"version":1,"name":"fast-transcript","content":content,"path":path})
                    )?
                ))
            } else if let Some(path) = path {
                print(&format!("{}\n", path.display()))
            } else {
                print(content)
            }
        }
        Some(("notes", values)) => {
            let timeout: f64 = values
                .get_one::<String>("timeout")
                .unwrap()
                .parse()
                .context("--timeout requires positive finite seconds")?;
            if !timeout.is_finite() || timeout <= 0.0 {
                bail!("--timeout must be positive finite seconds");
            }
            let destination = values
                .get_one::<String>("output")
                .or(values.get_one::<String>("destination"));
            let stdout = values.get_flag("stdout") || destination.is_some_and(|path| path == "-");
            let format = match values
                .get_one::<String>("input-format")
                .map(String::as_str)
                .unwrap()
            {
                "text" => crate::notes_input::InputFormat::Text,
                "json" => crate::notes_input::InputFormat::Json,
                "jsonl" => crate::notes_input::InputFormat::Jsonl,
                _ => crate::notes_input::InputFormat::Auto,
            };
            let result = crate::notes::generate(crate::notes::NotesOptions {
                input: values.get_one::<String>("input").unwrap().clone(),
                format,
                agent: values.get_one::<String>("agent").cloned(),
                model: values.get_one::<String>("model").cloned(),
                output: destination
                    .filter(|path| path.as_str() != "-")
                    .map(PathBuf::from),
                stdout,
                timeout: std::time::Duration::try_from_secs_f64(timeout)
                    .context("--timeout is outside the supported duration range")?,
            })?;
            crate::notes::emit(&result, json)
        }
        Some(("record", values)) => {
            let duration = values
                .get_one::<String>("duration")
                .map(|value| {
                    let seconds = value
                        .parse::<f64>()
                        .context("--duration requires positive finite seconds")?;
                    if !seconds.is_finite() || seconds <= 0.0 {
                        bail!("--duration requires positive finite seconds");
                    }
                    std::time::Duration::try_from_secs_f64(seconds)
                        .context("--duration is outside the supported range")
                })
                .transpose()?;
            // Reuse the media parser's model selection, environment, preference,
            // text renderer, and chunk defaults instead of inventing another path.
            let mut args = vec!["microphone".into(), "--text=plain".into()];
            for name in ["model", "device", "model-dir"] {
                if let Some(value) = values.get_one::<String>(name) {
                    args.extend([format!("--{name}"), value.clone()]);
                }
            }
            crate::recording::run(crate::recording::Options {
                args: crate::cli::parse_args_with_default_model(
                    &args,
                    crate::settings::default_model_selection,
                )?,
                control: values.get_flag("control"),
                hotkey: values.get_one::<String>("hotkey").cloned(),
                json,
                duration,
                output_directory: values.get_one::<String>("output-dir").map(PathBuf::from),
            })
        }
        Some(("agents", values)) => run_agents(values, json),
        Some(("models", values)) => run_models(values, json),
        Some(("export", values)) => crate::export::run(
            values.get_one::<String>("input").unwrap(),
            values
                .get_one::<String>("output")
                .or(values.get_one::<String>("destination"))
                .map(String::as_str),
            values.get_flag("stdout"),
            values.get_one::<String>("format").unwrap(),
            json,
            values.get_flag("copy"),
            values.get_one::<i32>("target-pid").copied(),
        ),
        _ => print(&crate::cli::usage()),
    }
}

pub(crate) fn usage_summary() -> String {
    let command = command();
    let mut lines = vec!["Actions (run fscript ACTION --help for details):".to_owned()];
    for action in command.get_subcommands() {
        lines.push(format!(
            "  {:<25} {}",
            action.get_name(),
            action
                .get_about()
                .map(ToString::to_string)
                .unwrap_or_default()
        ));
    }
    lines.push("  Use an explicit path, such as ./notes, for files named after actions.".into());
    lines.join("\n")
}

fn run_models(matches: &ArgMatches, json: bool) -> Result<()> {
    let store = crate::settings::DefaultModelStore::configured();
    let (operation, values) = matches.subcommand().unwrap_or(("list", matches));
    match operation {
        "set" => {
            let model = store.save(values.get_one::<String>("name").unwrap())?;
            eprintln!("Default model: {model}");
        }
        "reset" => {
            store.reset()?;
            eprintln!("Saved default model reset.");
        }
        "list" | "get" => {}
        _ => unreachable!("clap validates model commands"),
    }
    let saved = store.load()?;
    let environment = std::env::var("FSCRIPT_MODEL_URL").ok();
    let effective = environment
        .as_deref()
        .or(saved.as_deref())
        .unwrap_or("parakeet-v3-int8");
    if json {
        print(&format!(
            "{}\n",
            serde_json::to_string_pretty(&serde_json::json!({
                "version": 1, "saved_default": saved, "environment_override": environment,
                "effective_model": effective, "models": crate::model_catalog::MODELS,
            }))?
        ))
    } else if operation == "list" {
        print(&format!(
            "{}\n\nEffective model: {effective}\n",
            crate::model_catalog::list_models()
        ))
    } else if operation == "get" {
        print(&format!("{effective}\n"))
    } else {
        Ok(())
    }
}

fn print(text: &str) -> Result<()> {
    crate::delivery::write_stdout(&mut std::io::stdout().lock(), format_args!("{text}"))
}

fn run_agents(matches: &ArgMatches, json: bool) -> Result<()> {
    let store = crate::agents::AgentStore::configured();
    let (operation, values) = matches.subcommand().unwrap_or(("list", matches));
    if operation == "reset" {
        store.reset()?;
        if json {
            return print("{\"version\":1,\"reset\":true}\n");
        }
        eprintln!("Agent settings reset.");
        return Ok(());
    }
    let mut settings = store.load()?;
    match operation {
        "set" => {
            let name = values.get_one::<String>("name").unwrap();
            if name == "auto" {
                settings.default_agent = None;
            } else {
                settings.require_name(name)?;
                settings.default_agent = Some(name.clone());
            }
            store.save(&settings)?;
            eprintln!("Default notes agent: {name}");
        }
        "add" => {
            let name = values.get_one::<String>("name").unwrap();
            crate::agents::validate_custom_name(name)?;
            let mut command = values.get_many::<String>("command").unwrap().cloned();
            let mut program = command.next().unwrap();
            if std::path::Path::new(&program).components().count() > 1 {
                program = std::path::absolute(program)?.to_string_lossy().into_owned();
            }
            settings.commands.insert(
                name.clone(),
                crate::agents::CustomCommand {
                    program,
                    args: command.collect(),
                },
            );
            store.save(&settings)?;
            eprintln!("Saved agent {name}");
        }
        "remove" => {
            let name = values.get_one::<String>("name").unwrap();
            if settings.commands.remove(name).is_none() {
                bail!("no custom agent {name:?}; built-in agents cannot be removed");
            }
            if settings.default_agent.as_ref() == Some(name) {
                settings.default_agent = None;
            }
            store.save(&settings)?;
            eprintln!("Removed agent {name}");
        }
        "list" => {}
        _ => unreachable!("clap validates agent commands"),
    }
    let agents = crate::agents::inventory(&settings);
    let default = settings.default_agent.clone().or_else(|| {
        agents
            .iter()
            .find(|agent| agent.kind == "builtin" && agent.available)
            .map(|agent| agent.name.clone())
    });
    if json {
        return print(&format!(
            "{}\n",
            serde_json::to_string_pretty(
                &serde_json::json!({"version":1,"saved_default":settings.default_agent,"default_agent":default,"agents":agents})
            )?
        ));
    }
    if operation == "list" {
        let mut text = String::from("NAME             AVAILABLE  EXECUTABLE\n");
        for agent in agents {
            let mark = if default.as_ref() == Some(&agent.name) {
                " *"
            } else {
                ""
            };
            text.push_str(&format!(
                "{:<16} {:<10} {}\n",
                format!("{}{mark}", agent.name),
                if agent.available { "yes" } else { "no" },
                agent
                    .executable
                    .map_or(agent.program, |path| path.display().to_string())
            ));
        }
        text.push_str("\n* default agent; availability does not verify login.\n");
        print(&text)?;
    }
    Ok(())
}
