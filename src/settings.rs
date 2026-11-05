//! Persistent named-model preference, kept separate from model caches and CLI parsing.

use crate::errors::Explain;
use crate::model_catalog::find_model;
use anyhow::{bail, Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

#[derive(Deserialize, Serialize)]
struct SavedDefaultModel {
    model: String,
}

pub(crate) struct DefaultModelStore {
    path: PathBuf,
}

impl DefaultModelStore {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn configured() -> Self {
        let path = env::var_os("FSCRIPT_CONFIG_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let directory = ProjectDirs::from("", "", "fast-transcript")
                    .map(|dirs| dirs.config_dir().to_path_buf())
                    .unwrap_or_else(|| {
                        env::var_os("HOME")
                            .map(PathBuf::from)
                            .unwrap_or_else(|| PathBuf::from("."))
                            .join(".config/fast-transcript")
                    });
                directory.join("default-model.json")
            });
        Self::new(path)
    }

    pub(crate) fn load(&self) -> Result<Option<String>> {
        let contents = match fs::read(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).explain(
                || {
                    format!(
                        "could not read the saved default model in {}",
                        self.path.display()
                    )
                },
                "Check that the file is readable, or set FSCRIPT_CONFIG_FILE to another location",
            ),
        };
        let saved: SavedDefaultModel = serde_json::from_slice(&contents)
            .map_err(crate::errors::technical)
            .with_context(|| {
                format!(
                    "the saved default model in {} is damaged; run fscript --reset-default-model, then choose a model again with --set-default-model",
                    self.path.display()
                )
            })?;
        find_model(&saved.model)
            .map(|preset| Some(preset.name.to_string()))
            .with_context(|| {
                format!(
                    "the saved default model {:?} is not available in this version of fscript; run fscript --reset-default-model, or choose one from fscript --list-models",
                    saved.model
                )
            })
    }

    pub(crate) fn save(&self, model: &str) -> Result<String> {
        let model = canonical_model(model)?;
        let parent = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        let failed = || {
            format!(
                "could not save the default model to {}",
                self.path.display()
            )
        };
        fs::create_dir_all(parent).explain(failed, SETTINGS_HINT)?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(parent).explain(failed, SETTINGS_HINT)?;
        let contents = serde_json::to_vec_pretty(&SavedDefaultModel {
            model: model.clone(),
        })?;
        temporary
            .write_all(&contents)
            .and_then(|()| temporary.write_all(b"\n"))
            .and_then(|()| temporary.flush())
            .explain(failed, SETTINGS_HINT)?;
        temporary
            .persist(&self.path)
            .map_err(|error| error.error)
            .explain(failed, SETTINGS_HINT)?;
        Ok(model)
    }

    pub(crate) fn reset(&self) -> Result<()> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).explain(
                || {
                    format!(
                        "could not reset the saved default model in {}",
                        self.path.display()
                    )
                },
                SETTINGS_HINT,
            ),
        }
    }
}

const SETTINGS_HINT: &str =
    "Check that the folder is writable, or set FSCRIPT_CONFIG_FILE to another location";

fn canonical_model(name: &str) -> Result<String> {
    find_model(name)
        .map(|preset| preset.name.to_string())
        .with_context(|| crate::model_catalog::unknown_model_message(name))
}

pub(crate) fn default_model_selection() -> Result<Option<String>> {
    // Preserve the existing explicit environment override ahead of the saved preference.
    if let Ok(url) = env::var("FSCRIPT_MODEL_URL") {
        return Ok(Some(url));
    }
    DefaultModelStore::configured().load()
}

pub(crate) enum SettingsCommand {
    Set(String),
    Get,
    Reset,
}

pub(crate) fn parse_settings_command(args: &[String]) -> Result<Option<SettingsCommand>> {
    let filtered = args
        .iter()
        .filter(|arg| !matches!(arg.as_str(), "--quiet" | "-q"))
        .cloned()
        .collect::<Vec<_>>();
    let args = filtered.as_slice();
    let is_settings_flag = |arg: &str| {
        matches!(
            arg,
            "--set-default-model" | "--get-default-model" | "--reset-default-model"
        ) || arg.starts_with("--set-default-model=")
            || arg.starts_with("--get-default-model=")
            || arg.starts_with("--reset-default-model=")
    };
    if !args
        .iter()
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| is_settings_flag(arg))
    {
        return Ok(None);
    }
    let command = match args {
        [flag, model] if flag == "--set-default-model" && !model.starts_with('-') => SettingsCommand::Set(model.clone()),
        [flag] if flag.starts_with("--set-default-model=") && !flag.ends_with('=') => SettingsCommand::Set(flag.split_once('=').unwrap().1.to_string()),
        [flag] if flag == "--get-default-model" => SettingsCommand::Get,
        [flag] if flag == "--reset-default-model" => SettingsCommand::Reset,
        _ => bail!("default-model settings commands run on their own: fscript --set-default-model NAME, --get-default-model, or --reset-default-model"),
    };
    Ok(Some(command))
}

pub(crate) fn run_settings_command(command: SettingsCommand) -> Result<()> {
    let store = DefaultModelStore::configured();
    match command {
        SettingsCommand::Set(model) => eprintln!("Default model: {}", store.save(&model)?),
        SettingsCommand::Get => crate::delivery::write_stdout(
            &mut std::io::stdout().lock(),
            format_args!(
                "{}\n",
                store
                    .load()?
                    .unwrap_or_else(|| "parakeet-v3-int8".to_string())
            ),
        )?,
        SettingsCommand::Reset => {
            store.reset()?;
            eprintln!("Saved default model reset.");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_model_round_trips_canonical_name_and_rejects_invalid_updates() {
        let root = tempfile::tempdir().unwrap();
        let store = DefaultModelStore::new(root.path().join("nested/config.json"));
        assert_eq!(store.load().unwrap(), None);
        store.save("ultra").unwrap();
        assert_eq!(store.load().unwrap().as_deref(), Some("parakeet-ultra"));
        let before = std::fs::read(&store.path).unwrap();
        assert!(store.save("unknown-model").is_err());
        assert!(store.save("https://example.com/model.tar.gz").is_err());
        assert_eq!(std::fs::read(&store.path).unwrap(), before);
        store.reset().unwrap();
        assert_eq!(store.load().unwrap(), None);
        store.reset().unwrap();
    }

    #[test]
    fn malformed_settings_are_reported_and_reset_can_recover() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.json");
        std::fs::write(&path, "broken json").unwrap();
        let store = DefaultModelStore::new(path);
        assert!(store.load().is_err());
        store.reset().unwrap();
        assert_eq!(store.load().unwrap(), None);
    }

    #[test]
    fn settings_commands_reject_ambiguous_or_missing_arguments() {
        for args in [
            vec!["--set-default-model"],
            vec!["--set-default-model", "--json"],
            vec!["--set-default-model", "ultra", "audio.wav"],
            vec!["audio.wav", "--get-default-model"],
            vec!["--reset-default-model", "--json"],
            vec!["--set-default-model="],
        ] {
            let args = args.into_iter().map(str::to_string).collect::<Vec<_>>();
            assert!(parse_settings_command(&args).is_err(), "{args:?}");
        }
        assert!(
            matches!(parse_settings_command(&["--set-default-model=ultra".into()]).unwrap(), Some(SettingsCommand::Set(name)) if name == "ultra")
        );
        assert!(parse_settings_command(&["audio.wav".into()])
            .unwrap()
            .is_none());
    }
}
