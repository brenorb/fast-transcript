//! Re-render a completed transcript with the existing batch/stream output renderer.

use anyhow::{Context, Result};
use std::path::Path;

use crate::types::{BenchmarkResult, OutputFormat, SpeakersFormat, SubtitleFormat, TextFormat};

pub(crate) fn run(
    input: &str,
    output: Option<&str>,
    stdout: bool,
    format: &str,
    json: bool,
    copy: bool,
    paste_target: Option<i32>,
) -> Result<()> {
    let format = if format == "markdown" {
        None
    } else {
        Some(match format {
            "plain" => OutputFormat::Text(TextFormat::Plain),
            "compact" => OutputFormat::Text(TextFormat::Compact),
            "speakers" => OutputFormat::Speakers(SpeakersFormat::Timestamped),
            "srt" => OutputFormat::Subtitle(SubtitleFormat::Srt),
            "vtt" => OutputFormat::Subtitle(SubtitleFormat::Vtt),
            "json" => OutputFormat::Json,
            _ => OutputFormat::Text(TextFormat::Timestamped),
        })
    };
    let source = crate::notes_input::read_transcript(input)?;
    let content = if let Some(format) = format {
        let result: BenchmarkResult = serde_json::from_value(
            crate::notes_input::completed_document(&source)?,
        )
        .context("export requires a complete fscript JSON transcript or completed JSONL stream")?;
        crate::output::render_output(&result, format, true)?
    } else {
        if source.trim().is_empty() {
            anyhow::bail!("Markdown input is empty");
        }
        source.strip_suffix('\n').unwrap_or(&source).to_owned()
    };
    if copy || paste_target.is_some() {
        crate::clipboard::deliver(&content, paste_target)?;
    }
    let output_path = if copy
        || paste_target.is_some()
        || stdout
        || output == Some("-")
        || (input == "-" && output.is_none())
    {
        None
    } else {
        let source = Path::new(if input == "-" { "stdin.json" } else { input });
        let stem = source
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("transcript");
        let normalized = source.with_file_name(format!(
            "{}.json",
            stem.strip_suffix(".transcript")
                .filter(|stem| !stem.is_empty())
                .unwrap_or(stem)
        ));
        let default = format
            .map(|format| crate::delivery::default_output_path(&normalized, format))
            .unwrap_or_else(|| source.with_extension("export.md"));
        let destination = crate::delivery::OutputDestination::file(
            output.map(Path::new),
            &default,
            &std::env::current_dir()?,
        );
        let crate::delivery::OutputDestination::File(path) = destination else {
            unreachable!()
        };
        if input != "-" {
            crate::delivery::require_distinct_output(Path::new(input), &path)?;
        }
        Some(crate::delivery::write_output_file(&content, &path)?)
    };
    let text = if json {
        serde_json::to_string_pretty(
            &serde_json::json!({"version":1,"content":content,"output_path":output_path}),
        )?
    } else if let Some(path) = output_path {
        path.display().to_string()
    } else {
        content
    };
    crate::delivery::write_stdout(&mut std::io::stdout().lock(), format_args!("{text}\n"))
}
