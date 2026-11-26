//! Persistent external-agent selection, discovery, and provider-specific argv only.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(crate) const BUILTINS: [&str; 3] = ["codex", "claude", "omp"];

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CustomCommand {
    pub(crate) program: String,
    #[serde(default)]
    pub(crate) args: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentSettings {
    pub(crate) version: u32,
    pub(crate) default_agent: Option<String>,
    #[serde(default)]
    pub(crate) commands: BTreeMap<String, CustomCommand>,
}
impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            version: 1,
            default_agent: None,
            commands: BTreeMap::new(),
        }
    }
}
impl AgentSettings {
    fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!("unsupported agent settings version; expected 1");
        }
        for (name, command) in &self.commands {
            validate_custom_name(name)?;
            if command.program.trim().is_empty()
                || command.program.contains('\0')
                || command.args.iter().any(|arg| arg.contains('\0'))
            {
                bail!("agent {name:?} has an empty program or invalid NUL character");
            }
        }
        if let Some(name) = &self.default_agent {
            self.require_name(name)?;
        }
        Ok(())
    }

    pub(crate) fn require_name(&self, name: &str) -> Result<()> {
        if !BUILTINS.contains(&name) && !self.commands.contains_key(name) {
            bail!("unknown agent {name:?}; run fscript agents");
        }
        Ok(())
    }
}

pub(crate) struct AgentStore {
    path: PathBuf,
}
impl AgentStore {
    pub(crate) fn configured() -> Self {
        Self {
            path: std::env::var_os("FSCRIPT_NOTES_CONFIG_FILE")
                .map(PathBuf::from)
                .unwrap_or_else(|| crate::settings::config_directory().join("agents.json")),
        }
    }
    pub(crate) fn load(&self) -> Result<AgentSettings> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(AgentSettings::default())
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("cannot read agent settings {}", self.path.display()))
            }
        };
        let settings: AgentSettings = serde_json::from_slice(&bytes).with_context(|| {
            format!(
                "invalid agent settings {}; use fscript agents reset to recover",
                self.path.display()
            )
        })?;
        settings.validate()?;
        Ok(settings)
    }
    pub(crate) fn save(&self, settings: &AgentSettings) -> Result<()> {
        settings.validate()?;
        crate::delivery::write_output_file(&serde_json::to_string_pretty(settings)?, &self.path)?;
        Ok(())
    }
    pub(crate) fn reset(&self) -> Result<()> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).context("cannot reset agent settings"),
        }
    }
}

pub(crate) fn validate_custom_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "auto"
        || BUILTINS.contains(&name)
        || !name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'_'))
    {
        bail!("custom agent names use lowercase letters, digits, - or _ and cannot be auto/codex/claude/omp");
    }
    Ok(())
}

pub(crate) fn search_directories() -> Vec<PathBuf> {
    if let Some(paths) = std::env::var_os("FSCRIPT_AGENT_PATH") {
        return std::env::split_paths(&paths)
            .filter(|path| !path.as_os_str().is_empty())
            .collect();
    }
    let mut dirs = std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .unwrap_or_default();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for path in [
            ".local/bin",
            ".bun/bin",
            ".volta/bin",
            ".npm-global/bin",
            ".cargo/bin",
        ] {
            dirs.push(home.join(path));
        }
        let mut versions = std::fs::read_dir(home.join(".nvm/versions/node"))
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        versions.sort_by_key(|path| node_version(path));
        dirs.extend(versions.into_iter().rev().map(|path| path.join("bin")));
    }
    dirs.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ]);
    dirs
}

fn node_version(path: &Path) -> Vec<u32> {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .trim_start_matches('v')
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

#[derive(Serialize)]
pub(crate) struct AgentInfo {
    pub(crate) name: String,
    pub(crate) kind: &'static str,
    pub(crate) program: String,
    pub(crate) executable: Option<PathBuf>,
    pub(crate) available: bool,
}

pub(crate) fn inventory(settings: &AgentSettings) -> Vec<AgentInfo> {
    let dirs = search_directories();
    BUILTINS
        .into_iter()
        .map(|name| {
            let variable = format!("FSCRIPT_{}_BINARY", name.to_ascii_uppercase());
            let program = std::env::var(variable).unwrap_or_else(|_| name.to_owned());
            let executable = crate::executables::in_directories(&program, &dirs);
            AgentInfo {
                name: name.into(),
                kind: "builtin",
                program,
                available: executable.is_some(),
                executable,
            }
        })
        .chain(settings.commands.iter().map(|(name, command)| {
            let executable = crate::executables::in_directories(&command.program, &dirs);
            AgentInfo {
                name: name.clone(),
                kind: "custom",
                program: command.program.clone(),
                available: executable.is_some(),
                executable,
            }
        }))
        .collect()
}

pub(crate) struct SelectedAgent {
    pub(crate) name: String,
    pub(crate) executable: PathBuf,
    pub(crate) custom: Option<CustomCommand>,
}

pub(crate) fn select(settings: &AgentSettings, requested: Option<&str>) -> Result<SelectedAgent> {
    let entries = inventory(settings);
    let name = requested.filter(|name| *name != "auto").or_else(|| {
        if requested == Some("auto") {
            None
        } else {
            settings.default_agent.as_deref()
        }
    });
    if let Some(name) = name {
        settings.require_name(name)?;
    }
    let entry = match name {
        Some(name) => entries.into_iter().find(|entry| entry.name == name),
        None => entries.into_iter().find(|entry| entry.available && entry.kind == "builtin"),
    }.context("no agent is installed; install/sign in to codex, claude, or omp, or add a command with fscript agents add")?;
    let executable = entry.executable.with_context(|| {
        format!(
            "agent {} is unavailable; check its executable or choose another with --agent",
            entry.name
        )
    })?;
    Ok(SelectedAgent {
        custom: settings.commands.get(&entry.name).cloned(),
        name: entry.name,
        executable,
    })
}

impl SelectedAgent {
    pub(crate) fn arguments(&self, model: Option<&str>, output: &Path) -> Result<Vec<String>> {
        let mut args: Vec<String> = if let Some(custom) = &self.custom {
            if model.is_some() {
                bail!("--model applies to built-in agents; configure model arguments in the custom command");
            }
            return Ok(custom.args.clone());
        } else {
            match self.name.as_str() {
                "codex" => [
                    "exec",
                    "--skip-git-repo-check",
                    "--ephemeral",
                    "--sandbox",
                    "read-only",
                    "--ignore-user-config",
                    "--ignore-rules",
                    "--color",
                    "never",
                    "--disable",
                    "shell_tool",
                    "--disable",
                    "multi_agent",
                    "--disable",
                    "multi_agent_v2",
                    "--disable",
                    "tool_suggest",
                    "-c",
                    "web_search=\"disabled\"",
                    "-c",
                    "project_doc_max_bytes=0",
                ]
                .map(str::to_owned)
                .to_vec(),
                "claude" => [
                    "--print",
                    "--output-format",
                    "text",
                    "--no-session-persistence",
                    "--safe-mode",
                    "--tools",
                    "",
                    "--strict-mcp-config",
                    "--permission-prompts",
                    "none",
                ]
                .map(str::to_owned)
                .to_vec(),
                "omp" => [
                    "--print",
                    "--no-session",
                    "--no-tools",
                    "--no-extensions",
                    "--no-skills",
                    "--no-rules",
                    "--no-context-files",
                    "--no-title",
                    "--no-lsp",
                    "--offline",
                ]
                .map(str::to_owned)
                .to_vec(),
                _ => unreachable!("validated built-in agent"),
            }
        };
        if let Some(model) = model {
            args.extend(["--model".into(), model.into()]);
        }
        if self.name == "codex" {
            args.extend([
                "--output-last-message".into(),
                output.to_string_lossy().into_owned(),
                "-".into(),
            ]);
        }
        Ok(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_round_trip_and_invalid_updates_preserve_previous_data() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentStore {
            path: dir.path().join("nested/agents.json"),
        };
        let mut settings = store.load().unwrap();
        settings.default_agent = Some("claude".into());
        settings.commands.insert(
            "local".into(),
            CustomCommand {
                program: "my-agent".into(),
                args: vec!["--text".into(), "".into()],
            },
        );
        store.save(&settings).unwrap();
        assert_eq!(
            store.load().unwrap().default_agent.as_deref(),
            Some("claude")
        );
        let before = std::fs::read(&store.path).unwrap();
        settings.default_agent = Some("unknown".into());
        assert!(store.save(&settings).is_err());
        assert_eq!(std::fs::read(&store.path).unwrap(), before);
        store.reset().unwrap();
        assert!(store.load().unwrap().default_agent.is_none());
        assert!(node_version(Path::new("v24.22.1")) > node_version(Path::new("v8.9.0")));
    }
    #[test]
    fn custom_arguments_remain_literal_and_builtin_contracts_are_headless() {
        let custom = SelectedAgent {
            name: "custom".into(),
            executable: "runner".into(),
            custom: Some(CustomCommand {
                program: "runner".into(),
                args: vec!["$(touch never)".into(), "--literal".into()],
            }),
        };
        assert_eq!(
            custom.arguments(None, Path::new("ignored")).unwrap(),
            ["$(touch never)", "--literal"]
        );
        assert!(custom
            .arguments(Some("model"), Path::new("ignored"))
            .is_err());
        for name in BUILTINS {
            let agent = SelectedAgent {
                name: name.into(),
                executable: name.into(),
                custom: None,
            };
            let args = agent
                .arguments(Some("selected-model"), Path::new("result.md"))
                .unwrap();
            assert!(args
                .windows(2)
                .any(|pair| pair == ["--model", "selected-model"]));
            assert!(!args.iter().any(|arg| arg.contains("dangerously")));
        }
    }
}
