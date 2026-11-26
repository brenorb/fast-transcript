//! Explicit control commands; fscript's filename-based interface stays separate.

use anyhow::{bail, Context, Result};
use clap::{Arg, ArgAction, ArgMatches, Command};
use std::path::PathBuf;

fn command() -> Command {
    Command::new("fscriptctl").version(env!("CARGO_PKG_VERSION"))
        .about("Transcription jobs and external-agent notes")
        .color(clap::ColorChoice::Never)
        .arg(Arg::new("quiet").long("quiet").short('q').global(true).action(ArgAction::SetTrue).help("Suppress status; keep results and errors"))
        .arg(Arg::new("json").long("json").global(true).action(ArgAction::SetTrue).help("Print machine-readable completion data"))
        .arg(Arg::new("manual").long("manual").global(true).action(ArgAction::SetTrue).help("Read the complete offline control manual"))
        .arg(Arg::new("man-page").long("man-page").action(ArgAction::SetTrue).help("Export the control manual for man(1)"))
        .subcommand(Command::new("models").about("Inspect models and the shared saved preference")
            .subcommand_required(true).arg_required_else_help(true)
            .subcommand(Command::new("list").about("List the built-in catalog without downloading models"))
            .subcommand(Command::new("get").about("Show the effective model and preference sources"))
            .subcommand(Command::new("set").about("Save a predefined model or alias").arg(Arg::new("name").required(true)))
            .subcommand(Command::new("reset").about("Clear the saved preference; leave cached models untouched")))
        .subcommand(Command::new("notes").about("Create notes using an installed headless agent")
            .after_help("Examples:\n  fscriptctl notes meeting.txt\n  fscriptctl notes meeting.json --agent claude\n  fscript media.wav --stream --json | fscriptctl notes -\n  fscriptctl notes meeting.txt --stdout --json")
            .arg(Arg::new("input").required(true).help("UTF-8 transcript file, fscript JSON/JSONL, or - for stdin"))
            .arg(Arg::new("destination").help("Notes file or directory; - writes notes to stdout"))
            .arg(Arg::new("output").long("output").short('o').conflicts_with("destination").help("Explicit notes file or directory"))
            .arg(Arg::new("stdout").long("stdout").action(ArgAction::SetTrue).conflicts_with_all(["output", "destination"]).help("Print notes without saving a file"))
            .arg(Arg::new("agent").long("agent").help("codex, claude, omp, auto, or a saved custom command"))
            .arg(Arg::new("model").long("model").help("Override the built-in agent's language model"))
            .arg(Arg::new("input-format").long("input-format").default_value("auto").value_parser(["auto", "text", "json", "jsonl"]).help("Transcript input format"))
            .arg(Arg::new("timeout").long("timeout").default_value("300").help("Agent execution timeout in seconds")))
        .subcommand(Command::new("agents").about("Inspect or choose headless note agents")
            .subcommand_required(true).arg_required_else_help(true)
            .subcommand(Command::new("list").about("List agents and the effective default without starting them"))
            .subcommand(Command::new("set").about("Save the default agent; auto restores automatic detection").arg(Arg::new("name").required(true)))
            .subcommand(Command::new("add").about("Save a custom stdin-to-Markdown command")
                .after_help("Example: fscriptctl agents add local -- /absolute/path/to/agent --non-interactive")
                .arg(Arg::new("name").required(true)).arg(Arg::new("command").required(true).last(true).num_args(1..)))
            .subcommand(Command::new("remove").about("Remove a saved custom command").arg(Arg::new("name").required(true)))
            .subcommand(Command::new("reset").about("Reset note-agent settings; leave models and agent logins untouched")))
}

fn matches(raw: &[String]) -> std::result::Result<ArgMatches, clap::Error> {
    command()
        .try_get_matches_from(std::iter::once("fscriptctl".to_owned()).chain(raw.iter().cloned()))
}

pub(crate) fn needs_supervision(raw: &[String]) -> bool {
    matches(raw).is_ok_and(|matches| {
        matches.subcommand_name() == Some("notes") && !matches.get_flag("manual")
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
        return print(include_str!("../docs/control-cli.md"));
    }
    if matches.get_flag("man-page") {
        return print(include_str!("../docs/fscriptctl.1"));
    }
    let json = matches.get_flag("json");
    match matches.subcommand() {
        Some(("notes", values)) => {
            let timeout: f64 = values.get_one::<String>("timeout").unwrap().parse().context("--timeout requires positive finite seconds")?;
            if !timeout.is_finite() || timeout <= 0.0 { bail!("--timeout must be positive finite seconds"); }
            let destination = values.get_one::<String>("output").or(values.get_one::<String>("destination"));
            let stdout = values.get_flag("stdout") || destination.is_some_and(|path| path == "-");
            let format = match values.get_one::<String>("input-format").map(String::as_str).unwrap() {
                "text" => crate::notes_input::InputFormat::Text, "json" => crate::notes_input::InputFormat::Json,
                "jsonl" => crate::notes_input::InputFormat::Jsonl, _ => crate::notes_input::InputFormat::Auto,
            };
            let result = crate::notes::generate(crate::notes::NotesOptions {
                input: values.get_one::<String>("input").unwrap().clone(), format,
                agent: values.get_one::<String>("agent").cloned(), model: values.get_one::<String>("model").cloned(),
                output: destination.filter(|path| path.as_str() != "-").map(PathBuf::from), stdout,
                timeout: std::time::Duration::try_from_secs_f64(timeout).context("--timeout is outside the supported duration range")?,
            })?;
            crate::notes::emit(&result, json)
        }
        Some(("agents", values)) => run_agents(values, json),
        Some(("models", values)) => run_models(values, json),
        _ => print("fscriptctl — transcription controls and external-agent notes\n\nUsage: fscriptctl <command> [options]\n\nExamples:\n  fscriptctl notes meeting.txt\n  fscriptctl agents list\n\nRun fscriptctl --help for all commands.\n"),
    }
}

fn run_models(matches: &ArgMatches, json: bool) -> Result<()> {
    let store = crate::settings::DefaultModelStore::configured();
    let (operation, values) = matches.subcommand().context("choose a models command")?;
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
    let (operation, values) = matches.subcommand().context("choose an agents command")?;
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
