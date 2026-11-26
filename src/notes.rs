//! One headless process job for every provider; provider code only constructs argv.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::notes_input::{InputFormat, NotesTranscript};

pub(crate) struct NotesOptions {
    pub(crate) input: String,
    pub(crate) format: InputFormat,
    pub(crate) agent: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) output: Option<PathBuf>,
    pub(crate) stdout: bool,
    pub(crate) timeout: Duration,
}

#[derive(Serialize)]
pub(crate) struct NotesResult {
    version: u32,
    agent: String,
    agent_executable: PathBuf,
    model: Option<String>,
    input_source: String,
    input_format: &'static str,
    notes: String,
    output_path: Option<PathBuf>,
    elapsed_seconds: f64,
}

pub(crate) fn generate(options: NotesOptions) -> Result<NotesResult> {
    let started = Instant::now();
    let agent = crate::agents::select(
        &crate::agents::AgentStore::configured().load()?,
        options.agent.as_deref(),
    )?;
    let input = crate::notes_input::read_transcript(&options.input)?;
    let transcript = NotesTranscript::parse(&input, options.format)?;
    let output = destination(&options)?;
    let scratch = tempfile::Builder::new()
        .prefix("fscript-notes-")
        .tempdir()?;
    let response_path = scratch.path().join("response.md");
    let args = agent.arguments(options.model.as_deref(), &response_path)?;
    let mut prompt = tempfile::NamedTempFile::new_in(scratch.path())?;
    prompt.write_all(transcript.prompt()?.as_bytes())?;
    prompt.flush()?;
    let stdout = tempfile::NamedTempFile::new_in(scratch.path())?;
    let stderr = tempfile::NamedTempFile::new_in(scratch.path())?;
    let mut command = Command::new(&agent.executable);
    command
        .args(args)
        .current_dir(scratch.path())
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .stdin(Stdio::from(prompt.reopen()?))
        .stdout(Stdio::from(stdout.reopen()?))
        .stderr(Stdio::from(stderr.reopen()?));
    eprintln!("generating notes with {}...", agent.name);
    let child = command.spawn().with_context(|| {
        format!(
            "cannot start notes agent {} at {}",
            agent.name,
            agent.executable.display()
        )
    })?;
    let mut process = AgentProcess {
        child,
        finished: false,
    };
    let agent_started = Instant::now();
    let status = loop {
        if let Some(status) = process.child.try_wait()? {
            process.finished = true;
            break status;
        }
        if agent_started.elapsed() >= options.timeout {
            bail!(
                "notes agent {} timed out after {:.2}s; use --timeout SECONDS for a longer run",
                agent.name,
                options.timeout.as_secs_f64()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if !status.success() {
        bail!("notes agent {} failed ({status}); sign in using that agent's own CLI or inspect its configuration{}{}", agent.name,
            crate::diagnostics::diagnostic_tail(Some(&stderr)), crate::diagnostics::diagnostic_tail(Some(&stdout)));
    }
    let response = if agent.custom.is_none() && agent.name == "codex" {
        response_path.as_path()
    } else {
        stdout.path()
    };
    let notes = std::fs::read_to_string(response).with_context(|| {
        format!(
            "agent {} did not produce UTF-8 notes{}",
            agent.name,
            crate::diagnostics::diagnostic_tail(Some(&stderr))
        )
    })?;
    if notes.trim().is_empty() {
        bail!(
            "agent {} returned empty notes{}",
            agent.name,
            crate::diagnostics::diagnostic_tail(Some(&stderr))
        );
    }
    let notes = notes.trim().to_owned();
    let output_path = output
        .map(|path| crate::delivery::write_output_file(&notes, &path))
        .transpose()?;
    Ok(NotesResult {
        version: 1,
        agent: agent.name,
        agent_executable: agent.executable,
        model: options.model,
        input_source: options.input,
        input_format: transcript.format,
        notes,
        output_path,
        elapsed_seconds: started.elapsed().as_secs_f64(),
    })
}

fn destination(options: &NotesOptions) -> Result<Option<PathBuf>> {
    if options.stdout || (options.input == "-" && options.output.is_none()) {
        return Ok(None);
    }
    let input = Path::new(&options.input);
    let default = if options.input == "-" {
        PathBuf::from("stdin.notes.md")
    } else {
        input.with_extension("notes.md")
    };
    let destination = crate::delivery::OutputDestination::file(
        options.output.as_deref(),
        &default,
        &std::env::current_dir()?,
    );
    let crate::delivery::OutputDestination::File(path) = destination else {
        unreachable!()
    };
    if options.input != "-" {
        crate::delivery::require_distinct_output(input, &path)?;
    }
    Ok(Some(path))
}

pub(crate) fn emit(result: &NotesResult, json: bool) -> Result<()> {
    if json {
        crate::delivery::write_stdout(
            &mut io::stdout().lock(),
            format_args!("{}\n", serde_json::to_string_pretty(result)?),
        )
    } else if let Some(path) = &result.output_path {
        eprintln!("saved notes with {}", result.agent);
        crate::delivery::write_stdout(
            &mut io::stdout().lock(),
            format_args!("{}\n", path.display()),
        )
    } else {
        crate::delivery::write_stdout(&mut io::stdout().lock(), format_args!("{}\n", result.notes))
    }
}

struct AgentProcess {
    child: Child,
    finished: bool,
}
impl Drop for AgentProcess {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
