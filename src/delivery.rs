//! Output destination selection and delivery; transcript rendering stays in output.rs.

use crate::types::{InputSource, OutputFormat, SubtitleFormat};
use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub(crate) enum OutputDestination {
    Stdout,
    File(PathBuf),
}

impl OutputDestination {
    pub(crate) fn file(
        requested_path: Option<&Path>,
        default_path: &Path,
        current_dir: &Path,
    ) -> Self {
        Self::File(resolve_output_target_path(
            requested_path.unwrap_or(default_path),
            current_dir,
            default_path,
        ))
    }

    pub(crate) fn write(
        &self,
        contents: &str,
        status_message: &str,
        stdout: &mut impl Write,
        stderr: &mut impl Write,
    ) -> Result<()> {
        match self {
            Self::Stdout => {
                writeln!(stdout, "{contents}").context("failed to write transcript to stdout")
            }
            Self::File(path) => {
                let absolute_path = write_output_file(contents, path)?;
                emit_file_output_completion(stdout, stderr, &absolute_path, status_message)
            }
        }
    }
}

fn transcript_filename(stem: &str, output_format: OutputFormat) -> String {
    let suffix = match output_format {
        OutputFormat::Json => "transcript.json",
        OutputFormat::Speakers(_) => "speakers.txt",
        OutputFormat::Text(_) => "transcript.txt",
        OutputFormat::Subtitle(SubtitleFormat::Srt) => "srt",
        OutputFormat::Subtitle(SubtitleFormat::Vtt) => "vtt",
    };
    format!("{stem}.{suffix}")
}

fn default_output_path(audio_path: &Path, output_format: OutputFormat) -> PathBuf {
    let stem = audio_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("transcript");
    let file_name = transcript_filename(stem, output_format);
    audio_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(file_name)
}

pub(crate) fn default_output_path_for_input(
    source: &InputSource,
    video_title: Option<&str>,
    output_format: OutputFormat,
) -> PathBuf {
    match source {
        InputSource::LocalPath(path) => default_output_path(path, output_format),
        InputSource::RemoteUrl(url) => {
            let stem = video_title
                .filter(|value| !value.trim().is_empty())
                .map(sanitize_file_stem)
                .unwrap_or_else(|| sanitize_file_stem(url));
            PathBuf::from(transcript_filename(&stem, output_format))
        }
    }
}

fn resolve_output_target_path(
    output_path: &Path,
    current_dir: &Path,
    default_output_path: &Path,
) -> PathBuf {
    let candidate = if output_path.is_absolute() {
        output_path.to_path_buf()
    } else {
        current_dir.join(output_path)
    };

    if candidate.is_dir() || has_trailing_path_separator(output_path) {
        let file_name = default_output_path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("transcript.txt"));
        candidate.join(file_name)
    } else {
        candidate
    }
}

fn has_trailing_path_separator(path: &Path) -> bool {
    let value = path.as_os_str().to_string_lossy();
    if value.is_empty() {
        return false;
    }

    #[cfg(windows)]
    {
        value.ends_with('/') || value.ends_with('\\')
    }

    #[cfg(not(windows))]
    {
        value.ends_with(std::path::MAIN_SEPARATOR)
    }
}

fn write_output_file(contents: &str, output_path: &Path) -> Result<PathBuf> {
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
    }
    fs::write(output_path, format!("{contents}\n"))
        .with_context(|| format!("failed to write {}", output_path.display()))?;
    fs::canonicalize(output_path).with_context(|| {
        format!(
            "failed to resolve absolute path for {}",
            output_path.display()
        )
    })
}

fn emit_file_output_completion(
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    output_path: &Path,
    status_message: &str,
) -> Result<()> {
    writeln!(stdout, "{}", output_path.display())
        .context("failed to write final output path to stdout")?;
    writeln!(stderr, "{status_message}").context("failed to write status message to stderr")?;
    Ok(())
}

fn sanitize_file_stem(value: &str) -> String {
    let mut sanitized = String::new();
    let mut last_was_separator = false;
    for ch in value.chars() {
        let normalized = if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-') {
            last_was_separator = false;
            ch
        } else {
            if last_was_separator {
                continue;
            }
            last_was_separator = true;
            '_'
        };
        sanitized.push(normalized);
    }
    let sanitized = sanitized.trim_matches('_').to_string();
    if sanitized.is_empty() {
        "transcript".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SpeakersFormat, TextFormat};
    use std::io::Cursor;
    use tempfile::tempdir;
    #[test]
    fn default_output_path_stays_next_to_source_audio() {
        let path = PathBuf::from("/tmp/folder/audio.file.mp3");
        let output = default_output_path(&path, OutputFormat::Json);
        assert_eq!(
            output,
            PathBuf::from("/tmp/folder/audio.file.transcript.json")
        );
    }

    #[test]
    fn default_speakers_output_path_uses_speakers_extension() {
        let path = PathBuf::from("/tmp/folder/audio.file.mp3");
        let output =
            default_output_path(&path, OutputFormat::Speakers(SpeakersFormat::Timestamped));
        assert_eq!(output, PathBuf::from("/tmp/folder/audio.file.speakers.txt"));
    }

    #[test]
    fn default_text_output_path_uses_transcript_text_extension() {
        let path = PathBuf::from("/tmp/folder/audio.file.mp3");
        let output = default_output_path(&path, OutputFormat::Text(TextFormat::Plain));
        assert_eq!(
            output,
            PathBuf::from("/tmp/folder/audio.file.transcript.txt")
        );
    }

    #[test]
    fn default_srt_output_path_uses_srt_extension() {
        let path = PathBuf::from("/tmp/folder/audio.file.mp3");
        let output = default_output_path(&path, OutputFormat::Subtitle(SubtitleFormat::Srt));
        assert_eq!(output, PathBuf::from("/tmp/folder/audio.file.srt"));
    }

    #[test]
    fn default_vtt_output_path_uses_vtt_extension() {
        let path = PathBuf::from("/tmp/folder/audio.file.mp3");
        let output = default_output_path(&path, OutputFormat::Subtitle(SubtitleFormat::Vtt));
        assert_eq!(output, PathBuf::from("/tmp/folder/audio.file.vtt"));
    }

    #[test]
    fn default_output_path_for_remote_url_uses_video_title() {
        let output = default_output_path_for_input(
            &InputSource::RemoteUrl("https://youtu.be/demo".to_string()),
            Some("TED Talk: Future / Now"),
            OutputFormat::Json,
        );
        assert_eq!(output, PathBuf::from("TED_Talk_Future_Now.transcript.json"));
    }

    #[test]
    fn default_speakers_output_path_for_remote_url_uses_video_title() {
        let output = default_output_path_for_input(
            &InputSource::RemoteUrl("https://youtu.be/demo".to_string()),
            Some("TED Talk: Future / Now"),
            OutputFormat::Speakers(SpeakersFormat::Timestamped),
        );
        assert_eq!(output, PathBuf::from("TED_Talk_Future_Now.speakers.txt"));
    }

    #[test]
    fn default_text_output_path_for_remote_url_uses_video_title() {
        let output = default_output_path_for_input(
            &InputSource::RemoteUrl("https://youtu.be/demo".to_string()),
            Some("TED Talk: Future / Now"),
            OutputFormat::Text(TextFormat::Plain),
        );
        assert_eq!(output, PathBuf::from("TED_Talk_Future_Now.transcript.txt"));
    }

    #[test]
    fn default_srt_output_path_for_remote_url_uses_video_title() {
        let output = default_output_path_for_input(
            &InputSource::RemoteUrl("https://youtu.be/demo".to_string()),
            Some("TED Talk: Future / Now"),
            OutputFormat::Subtitle(SubtitleFormat::Srt),
        );
        assert_eq!(output, PathBuf::from("TED_Talk_Future_Now.srt"));
    }

    #[test]
    fn default_vtt_output_path_for_remote_url_uses_video_title() {
        let output = default_output_path_for_input(
            &InputSource::RemoteUrl("https://youtu.be/demo".to_string()),
            Some("TED Talk: Future / Now"),
            OutputFormat::Subtitle(SubtitleFormat::Vtt),
        );
        assert_eq!(output, PathBuf::from("TED_Talk_Future_Now.vtt"));
    }

    #[test]
    fn resolve_output_target_path_uses_default_filename_inside_existing_directory() {
        let temp = tempdir().unwrap();
        let cwd = temp.path();
        let directory = cwd.join("exports");
        std::fs::create_dir_all(&directory).unwrap();
        let resolved = resolve_output_target_path(
            Path::new("exports"),
            cwd,
            Path::new("/tmp/audio.speakers.txt"),
        );
        assert_eq!(resolved, directory.join("audio.speakers.txt"));
    }

    #[test]
    fn resolve_output_target_path_uses_default_filename_for_missing_directory_with_trailing_slash()
    {
        let temp = tempdir().unwrap();
        let cwd = temp.path();

        let resolved = resolve_output_target_path(
            Path::new("exports/"),
            cwd,
            Path::new("/tmp/audio.speakers.txt"),
        );

        assert_eq!(resolved, cwd.join("exports").join("audio.speakers.txt"));
    }

    #[test]
    fn file_destination_keeps_absolute_paths() {
        let root = tempdir().unwrap();
        let path = root.path().join("out.json");
        let destination =
            OutputDestination::file(Some(&path), Path::new("default.json"), Path::new("/other"));
        match destination {
            OutputDestination::File(resolved) => assert_eq!(resolved, path),
            OutputDestination::Stdout => panic!("expected file destination"),
        }
    }

    #[test]
    fn emit_file_output_completion_sends_path_to_stdout_and_status_to_stderr() {
        let mut stdout = Cursor::new(Vec::new());
        let mut stderr = Cursor::new(Vec::new());
        let path = Path::new("/tmp/example.transcript.json");

        emit_file_output_completion(&mut stdout, &mut stderr, path, "done in 11.11x real-time")
            .unwrap();

        assert_eq!(
            String::from_utf8(stdout.into_inner()).unwrap(),
            "/tmp/example.transcript.json\n"
        );
        assert_eq!(
            String::from_utf8(stderr.into_inner()).unwrap(),
            "done in 11.11x real-time\n"
        );
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::OutputDestination;
    use std::path::Path;

    #[test]
    fn stdout_delivery_writes_only_contents_and_propagates_stream_errors() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        OutputDestination::Stdout
            .write("transcript", "finished", &mut stdout, &mut stderr)
            .unwrap();
        assert_eq!(stdout, b"transcript\n");
        assert!(stderr.is_empty());
        let error = OutputDestination::Stdout
            .write(
                "transcript",
                "finished",
                &mut &mut [0u8; 0][..],
                &mut stderr,
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("stdout"));
    }

    #[test]
    fn file_delivery_resolves_directory_once_and_reports_absolute_path() {
        let root = tempfile::tempdir().unwrap();
        let destination = OutputDestination::file(
            Some(Path::new("exports/")),
            Path::new("input/lecture.transcript.txt"),
            root.path(),
        );
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        destination
            .write("transcript", "finished", &mut stdout, &mut stderr)
            .unwrap();
        let path = root.path().join("exports/lecture.transcript.txt");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "transcript\n");
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            format!("{}\n", std::fs::canonicalize(path).unwrap().display())
        );
        assert_eq!(stderr, b"finished\n");
    }

    #[test]
    fn file_delivery_propagates_write_failure_without_success_output() {
        let root = tempfile::tempdir().unwrap();
        let blocker = root.path().join("blocker");
        std::fs::write(&blocker, "existing file").unwrap();
        let destination = OutputDestination::file(
            Some(Path::new("blocker/out.txt")),
            Path::new("default.txt"),
            root.path(),
        );
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert!(destination
            .write("transcript", "finished", &mut stdout, &mut stderr)
            .is_err());
        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
        assert_eq!(std::fs::read_to_string(blocker).unwrap(), "existing file");
    }
}
