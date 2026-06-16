use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::types::{
    BenchmarkChunk, BenchmarkResult, InputSource, OutputFormat, SpeakersFormat, SubtitleFormat,
    TextFormat, TranscriptSegment,
};

pub(crate) fn default_output_path(audio_path: &Path, output_format: OutputFormat) -> PathBuf {
    let stem = audio_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("transcript");
    let file_name = match output_format {
        OutputFormat::Json => format!("{stem}.transcript.json"),
        OutputFormat::Speakers(_) => format!("{stem}.speakers.txt"),
        OutputFormat::Text(_) => format!("{stem}.transcript.txt"),
        OutputFormat::Subtitle(SubtitleFormat::Srt) => format!("{stem}.srt"),
        OutputFormat::Subtitle(SubtitleFormat::Vtt) => format!("{stem}.vtt"),
    };
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
            match output_format {
                OutputFormat::Json => PathBuf::from(format!("{stem}.transcript.json")),
                OutputFormat::Speakers(_) => PathBuf::from(format!("{stem}.speakers.txt")),
                OutputFormat::Text(_) => PathBuf::from(format!("{stem}.transcript.txt")),
                OutputFormat::Subtitle(SubtitleFormat::Srt) => PathBuf::from(format!("{stem}.srt")),
                OutputFormat::Subtitle(SubtitleFormat::Vtt) => PathBuf::from(format!("{stem}.vtt")),
            }
        }
    }
}

pub(crate) fn resolve_output_target_path(
    output_path: &Path,
    current_dir: &Path,
    default_output_path: &Path,
) -> PathBuf {
    let candidate = if output_path.is_absolute() {
        output_path.to_path_buf()
    } else {
        current_dir.join(output_path)
    };

    if candidate.is_dir() {
        let file_name = default_output_path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("transcript.txt"));
        candidate.join(file_name)
    } else {
        candidate
    }
}

pub(crate) fn resolve_absolute_output_path(
    output_path: &Path,
    current_dir: &Path,
) -> Result<PathBuf> {
    let candidate = if output_path.is_absolute() {
        output_path.to_path_buf()
    } else {
        current_dir.join(output_path)
    };

    fs::canonicalize(&candidate).with_context(|| {
        format!(
            "failed to resolve absolute path for {}",
            candidate.display()
        )
    })
}

pub(crate) fn write_output_file(
    contents: &str,
    output_path: &Path,
    current_dir: &Path,
) -> Result<PathBuf> {
    let candidate = if output_path.is_absolute() {
        output_path.to_path_buf()
    } else {
        current_dir.join(output_path)
    };

    if let Some(parent) = candidate.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
    }
    fs::write(&candidate, format!("{contents}\n"))
        .with_context(|| format!("failed to write {}", candidate.display()))?;

    resolve_absolute_output_path(&candidate, current_dir)
}

pub(crate) fn normalize_speaker_label(label: &str, unknown_label: &str) -> String {
    let cleaned = label.trim();
    if cleaned.is_empty() {
        return unknown_label.to_string();
    }

    let formatted = cleaned
        .strip_prefix("SPEAKER_")
        .or_else(|| cleaned.strip_prefix("S"));
    if let Some(suffix) = formatted {
        if !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()) {
            if let Ok(number) = suffix.parse::<usize>() {
                return format!("SPEAKER_{number:02}");
            }
        }
    }

    cleaned.to_string()
}

pub(crate) fn format_hhmmss(seconds: f64) -> String {
    let total_seconds = if seconds.is_finite() {
        seconds.max(0.0).floor() as u64
    } else {
        0
    };
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn format_subtitle_timestamp(seconds: f64, millis_separator: char) -> String {
    let total_millis = if seconds.is_finite() {
        (seconds.max(0.0) * 1000.0).round() as u64
    } else {
        0
    };
    let hours = total_millis / 3_600_000;
    let minutes = (total_millis % 3_600_000) / 60_000;
    let seconds = (total_millis % 60_000) / 1_000;
    let millis = total_millis % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02}{millis_separator}{millis:03}")
}

pub(crate) fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

const CLEAN_REPEAT_ALLOWLIST: &[&str] = &[
    "a", "ah", "and", "de", "e", "eh", "eu", "i", "o", "the", "to", "uh", "um", "we",
];

const CLEAN_REPEAT_FILLERS: &[&str] = &["ah", "eh", "uh", "um"];

fn split_token_affixes(token: &str) -> (&str, &str, &str) {
    let start = token
        .char_indices()
        .find(|(_, ch)| ch.is_alphanumeric() || *ch == '\'')
        .map(|(index, _)| index)
        .unwrap_or(token.len());
    let end = token
        .char_indices()
        .rev()
        .find(|(_, ch)| ch.is_alphanumeric() || *ch == '\'')
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(start);
    (&token[..start], &token[start..end], &token[end..])
}

pub(crate) fn repeated_word_threshold(normalized: &str) -> Option<usize> {
    if normalized.is_empty() {
        return None;
    }

    if CLEAN_REPEAT_FILLERS.contains(&normalized) || CLEAN_REPEAT_ALLOWLIST.contains(&normalized) {
        return Some(4);
    }

    let char_count = normalized.chars().count();
    if char_count <= 3 {
        return Some(5);
    }

    None
}

pub(crate) fn clean_pathological_repeated_words(text: &str) -> String {
    let tokens = text.split_whitespace().collect::<Vec<_>>();
    if tokens.is_empty() {
        return String::new();
    }

    let mut output = Vec::with_capacity(tokens.len());
    let mut index = 0usize;

    while index < tokens.len() {
        let (_, first_core, _) = split_token_affixes(tokens[index]);
        let normalized = first_core.to_lowercase();
        if normalized.is_empty() {
            output.push(tokens[index].to_string());
            index += 1;
            continue;
        }

        let mut run_end = index + 1;
        while run_end < tokens.len() {
            let (_, candidate_core, _) = split_token_affixes(tokens[run_end]);
            if candidate_core.to_lowercase() != normalized {
                break;
            }
            run_end += 1;
        }

        let run_len = run_end - index;
        let threshold = repeated_word_threshold(&normalized);
        if threshold.is_some_and(|value| run_len >= value) {
            let (first_leading, first_core, _) = split_token_affixes(tokens[index]);
            let (_, last_core, last_trailing) = split_token_affixes(tokens[run_end - 1]);
            output.push(format!("{first_leading}{first_core}..."));
            output.push(format!("{last_core}{last_trailing}"));
        } else {
            output.extend(
                tokens[index..run_end]
                    .iter()
                    .map(|token| (*token).to_string()),
            );
        }

        index = run_end;
    }

    collapse_whitespace(&output.join(" "))
}

fn cleaned_transcript_segment(segment: &TranscriptSegment) -> TranscriptSegment {
    let mut cleaned = segment.clone();
    cleaned.text = clean_pathological_repeated_words(&cleaned.text);
    cleaned
}

fn cleaned_benchmark_chunk(chunk: &BenchmarkChunk) -> BenchmarkChunk {
    let mut cleaned = chunk.clone();
    cleaned.text = clean_pathological_repeated_words(&cleaned.text);
    cleaned
}

fn cleaned_benchmark_result(result: &BenchmarkResult) -> BenchmarkResult {
    let mut cleaned = result.clone();
    cleaned.text = clean_pathological_repeated_words(&cleaned.text);
    cleaned.chunks = cleaned
        .chunks
        .iter()
        .map(cleaned_benchmark_chunk)
        .collect::<Vec<_>>();
    cleaned.segments = cleaned.segments.as_ref().map(|segments| {
        segments
            .iter()
            .map(cleaned_transcript_segment)
            .collect::<Vec<_>>()
    });
    cleaned
}

fn segment_display_text(segment: &TranscriptSegment, include_speaker: bool) -> String {
    let text = segment.text.trim();
    if !include_speaker {
        return text.to_string();
    }

    match segment
        .speaker
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(speaker) => format!("{}: {text}", normalize_speaker_label(speaker, "UNKNOWN")),
        None => text.to_string(),
    }
}

fn render_plain_text(result: &BenchmarkResult) -> String {
    if let Some(segments) = result.segments.as_deref() {
        return segments
            .iter()
            .map(|segment| segment.text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
    }

    result.text.trim().to_string()
}

fn render_compact_text(result: &BenchmarkResult) -> String {
    result.text.trim().replace("\n", " ").trim().to_string()
}

fn normalized_speaker_label(speaker: Option<&str>) -> Option<String> {
    speaker
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| normalize_speaker_label(value, "UNKNOWN"))
}

pub(crate) fn render_speaker_lines(
    segments: &[TranscriptSegment],
    speakers_format: SpeakersFormat,
    merge_consecutive: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current_speaker: Option<String> = None;
    let mut current_start_s = 0.0;
    let mut current_parts: Vec<&str> = Vec::new();

    let flush = |lines: &mut Vec<String>,
                 current_speaker: &mut Option<String>,
                 current_start_s: &mut f64,
                 current_parts: &mut Vec<&str>| {
        let text = current_parts.join(" ").trim().to_string();
        current_parts.clear();
        if text.is_empty() {
            current_speaker.take();
            return;
        }
        let speaker_prefix = current_speaker
            .take()
            .map(|speaker| format!("{speaker}: "))
            .unwrap_or_default();
        let line = match speakers_format {
            SpeakersFormat::Plain => format!("{speaker_prefix}{text}"),
            SpeakersFormat::Timestamped => {
                format!(
                    "{} - {speaker_prefix}{text}",
                    format_hhmmss(*current_start_s)
                )
            }
        };
        lines.push(line);
    };

    for segment in segments {
        let text = segment.text.trim();
        if text.is_empty() {
            continue;
        }

        let speaker = normalized_speaker_label(segment.speaker.as_deref());
        if merge_consecutive && current_speaker == speaker {
            current_parts.push(text);
            continue;
        }

        flush(
            &mut lines,
            &mut current_speaker,
            &mut current_start_s,
            &mut current_parts,
        );
        current_start_s = segment.start_s;
        current_speaker = speaker;
        current_parts.push(text);
    }

    flush(
        &mut lines,
        &mut current_speaker,
        &mut current_start_s,
        &mut current_parts,
    );
    lines
}

fn render_timestamped_text_lines(segments: &[TranscriptSegment]) -> Vec<String> {
    segments
        .iter()
        .filter_map(|segment| {
            let text = segment.text.trim();
            if text.is_empty() {
                None
            } else {
                Some(format!("{} - {text}", format_hhmmss(segment.start_s)))
            }
        })
        .collect()
}

fn render_srt(segments: &[TranscriptSegment]) -> String {
    segments
        .iter()
        .filter(|segment| segment.end_s > segment.start_s && !segment.text.trim().is_empty())
        .enumerate()
        .map(|(index, segment)| {
            format!(
                "{}\n{} --> {}\n{}",
                index + 1,
                format_subtitle_timestamp(segment.start_s, ','),
                format_subtitle_timestamp(segment.end_s, ','),
                segment_display_text(segment, true),
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_vtt(segments: &[TranscriptSegment]) -> String {
    let body = segments
        .iter()
        .filter(|segment| segment.end_s > segment.start_s && !segment.text.trim().is_empty())
        .map(|segment| {
            format!(
                "{} --> {}\n{}",
                format_subtitle_timestamp(segment.start_s, '.'),
                format_subtitle_timestamp(segment.end_s, '.'),
                segment_display_text(segment, true),
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    if body.is_empty() {
        "WEBVTT".to_string()
    } else {
        format!("WEBVTT\n\n{body}")
    }
}

pub(crate) fn render_output(
    result: &BenchmarkResult,
    output_format: OutputFormat,
    clean_output: bool,
) -> Result<String> {
    let cleaned_result;
    let effective_result = if clean_output {
        cleaned_result = cleaned_benchmark_result(result);
        &cleaned_result
    } else {
        result
    };

    match output_format {
        OutputFormat::Json => serde_json::to_string_pretty(effective_result).map_err(Into::into),
        OutputFormat::Speakers(speakers_format) => Ok(render_speaker_lines(
            effective_result.segments.as_deref().unwrap_or(&[]),
            speakers_format,
            true,
        )
        .join("\n")),
        OutputFormat::Text(TextFormat::Plain) => Ok(render_plain_text(effective_result)),
        OutputFormat::Text(TextFormat::Compact) => Ok(render_compact_text(effective_result)),
        OutputFormat::Text(TextFormat::Timestamped) => Ok(render_timestamped_text_lines(
            effective_result.segments.as_deref().unwrap_or(&[]),
        )
        .join("\n")),
        OutputFormat::Subtitle(SubtitleFormat::Srt) => Ok(render_srt(
            effective_result.segments.as_deref().unwrap_or(&[]),
        )),
        OutputFormat::Subtitle(SubtitleFormat::Vtt) => Ok(render_vtt(
            effective_result.segments.as_deref().unwrap_or(&[]),
        )),
    }
}

pub(crate) fn emit_file_output_completion(
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
    use super::{
        clean_pathological_repeated_words, default_output_path, default_output_path_for_input,
        emit_file_output_completion, format_hhmmss, normalize_speaker_label, render_output,
        render_speaker_lines, repeated_word_threshold, resolve_absolute_output_path,
        resolve_output_target_path, write_output_file,
    };
    use crate::types::{
        BenchmarkResult, InputSource, OutputFormat, SpeakersFormat, SubtitleFormat, TextFormat,
        TranscriptSegment,
    };
    use std::io::Cursor;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    fn sample_result(segments: Vec<TranscriptSegment>) -> BenchmarkResult {
        let text = segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        BenchmarkResult {
            input_source: "input".to_string(),
            model_dir: "model".to_string(),
            audio_path: "audio".to_string(),
            prepared_audio_path: "prepared".to_string(),
            used_ffmpeg_normalization: false,
            used_local_model: true,
            transcript_source: "local-audio-local-model".to_string(),
            audio_seconds: 10.0,
            load_seconds: 1.0,
            transcribe_seconds: 1.0,
            total_inside_seconds: 2.0,
            seconds_per_audio_second: 0.2,
            realtime_speedup: 5.0,
            text,
            chunk_seconds: Some(120.0),
            chunk_overlap_seconds: 2.0,
            chunk_count: 1,
            chunks: vec![],
            segments: Some(segments),
            speaker_diarization: None,
        }
    }

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
    fn write_output_file_returns_absolute_path_for_relative_output() {
        let temp = tempdir().unwrap();
        let cwd = temp.path();
        let expected = cwd.join("nested/out.json");

        let output = write_output_file("{\"ok\":true}", Path::new("nested/out.json"), cwd).unwrap();

        assert_eq!(output, std::fs::canonicalize(&expected).unwrap());
        assert_eq!(
            std::fs::read_to_string(expected).unwrap(),
            "{\"ok\":true}\n"
        );
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
    fn resolve_absolute_output_path_keeps_absolute_paths() {
        let temp = tempdir().unwrap();
        let output = temp.path().join("result.json");
        std::fs::write(&output, "{}\n").unwrap();

        assert_eq!(
            resolve_absolute_output_path(&output, Path::new("/tmp/ignored")).unwrap(),
            std::fs::canonicalize(&output).unwrap()
        );
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

    #[test]
    fn normalize_speaker_label_formats_simple_labels() {
        assert_eq!(normalize_speaker_label("S1", "UNKNOWN"), "SPEAKER_01");
        assert_eq!(
            normalize_speaker_label("SPEAKER_2", "UNKNOWN"),
            "SPEAKER_02"
        );
        assert_eq!(normalize_speaker_label("", "UNKNOWN"), "UNKNOWN");
        assert_eq!(normalize_speaker_label("Speaker 0", "UNKNOWN"), "Speaker 0");
    }

    #[test]
    fn format_hhmmss_truncates_to_whole_seconds() {
        assert_eq!(format_hhmmss(0.0), "00:00:00");
        assert_eq!(format_hhmmss(65.9), "00:01:05");
        assert_eq!(format_hhmmss(3_661.2), "01:01:01");
    }

    #[test]
    fn render_speaker_lines_merges_consecutive_turns() {
        let segments = vec![
            TranscriptSegment {
                start_s: 5.0,
                end_s: 6.0,
                text: "Oi.".to_string(),
                speaker: Some("S1".to_string()),
            },
            TranscriptSegment {
                start_s: 6.0,
                end_s: 7.0,
                text: "Tudo bem?".to_string(),
                speaker: Some("S1".to_string()),
            },
            TranscriptSegment {
                start_s: 8.0,
                end_s: 9.0,
                text: "Tudo.".to_string(),
                speaker: Some("S2".to_string()),
            },
        ];

        assert_eq!(
            render_speaker_lines(&segments, SpeakersFormat::Plain, true),
            vec![
                "SPEAKER_01: Oi. Tudo bem?".to_string(),
                "SPEAKER_02: Tudo.".to_string(),
            ]
        );
    }

    #[test]
    fn render_speaker_lines_can_include_timestamps() {
        let segments = vec![
            TranscriptSegment {
                start_s: 65.9,
                end_s: 67.0,
                text: "Primeira.".to_string(),
                speaker: Some("S1".to_string()),
            },
            TranscriptSegment {
                start_s: 68.0,
                end_s: 70.0,
                text: "Segunda.".to_string(),
                speaker: Some("S1".to_string()),
            },
        ];

        assert_eq!(
            render_speaker_lines(&segments, SpeakersFormat::Timestamped, false),
            vec![
                "00:01:05 - SPEAKER_01: Primeira.".to_string(),
                "00:01:08 - SPEAKER_01: Segunda.".to_string(),
            ]
        );
    }

    #[test]
    fn render_speaker_lines_omits_unknown_label_when_diarization_is_absent() {
        let segments = vec![TranscriptSegment {
            start_s: 65.9,
            end_s: 67.0,
            text: "Primeira.".to_string(),
            speaker: None,
        }];

        assert_eq!(
            render_speaker_lines(&segments, SpeakersFormat::Timestamped, false),
            vec!["00:01:05 - Primeira.".to_string()]
        );
    }

    #[test]
    fn repeated_word_threshold_is_conservative() {
        assert_eq!(repeated_word_threshold("we"), Some(4));
        assert_eq!(repeated_word_threshold("uh"), Some(4));
        assert_eq!(repeated_word_threshold("for"), Some(5));
        assert_eq!(repeated_word_threshold("bitcoin"), None);
    }

    #[test]
    fn clean_pathological_repeated_words_collapses_obvious_runs() {
        assert_eq!(
            clean_pathological_repeated_words(
                "So we we we we we we didn't have time and uh uh uh uh to fix it."
            ),
            "So we... we didn't have time and uh... uh to fix it."
        );
    }

    #[test]
    fn clean_pathological_repeated_words_leaves_normal_repetition_alone() {
        assert_eq!(
            clean_pathological_repeated_words("no no no maybe bitcoin bitcoin"),
            "no no no maybe bitcoin bitcoin"
        );
    }

    #[test]
    fn render_output_supports_plain_text() {
        let result = sample_result(vec![
            TranscriptSegment {
                start_s: 0.0,
                end_s: 1.0,
                text: "Primeira frase.".to_string(),
                speaker: None,
            },
            TranscriptSegment {
                start_s: 1.0,
                end_s: 2.0,
                text: "Segunda frase.".to_string(),
                speaker: None,
            },
        ]);

        assert_eq!(
            render_output(&result, OutputFormat::Text(TextFormat::Plain), false).unwrap(),
            "Primeira frase.\nSegunda frase."
        );
    }

    #[test]
    fn render_output_plain_text_falls_back_to_result_text_without_segments() {
        let result = BenchmarkResult {
            input_source: "input.wav".to_string(),
            model_dir: "model".to_string(),
            audio_path: "input.wav".to_string(),
            prepared_audio_path: "input.wav".to_string(),
            used_ffmpeg_normalization: false,
            used_local_model: true,
            transcript_source: "local".to_string(),
            audio_seconds: 1.0,
            load_seconds: 0.1,
            transcribe_seconds: 0.2,
            total_inside_seconds: 0.3,
            seconds_per_audio_second: 0.3,
            realtime_speedup: 3.33,
            text: "Primeira frase.\nSegunda frase.".to_string(),
            chunk_seconds: None,
            chunk_overlap_seconds: 0.0,
            chunk_count: 1,
            chunks: vec![],
            segments: None,
            speaker_diarization: None,
        };

        assert_eq!(
            render_output(&result, OutputFormat::Text(TextFormat::Plain), false).unwrap(),
            "Primeira frase.\nSegunda frase."
        );
    }

    #[test]
    fn render_output_supports_compact_text() {
        let result = sample_result(vec![
            TranscriptSegment {
                start_s: 0.0,
                end_s: 1.0,
                text: "Primeira frase.".to_string(),
                speaker: None,
            },
            TranscriptSegment {
                start_s: 1.0,
                end_s: 2.0,
                text: "Segunda frase.".to_string(),
                speaker: None,
            },
        ]);

        assert_eq!(
            render_output(&result, OutputFormat::Text(TextFormat::Compact), false).unwrap(),
            "Primeira frase. Segunda frase."
        );
    }

    #[test]
    fn render_output_supports_timestamped_text() {
        let result = sample_result(vec![
            TranscriptSegment {
                start_s: 5.0,
                end_s: 6.0,
                text: "Primeira frase.".to_string(),
                speaker: None,
            },
            TranscriptSegment {
                start_s: 65.9,
                end_s: 67.0,
                text: "Segunda frase.".to_string(),
                speaker: None,
            },
        ]);

        assert_eq!(
            render_output(&result, OutputFormat::Text(TextFormat::Timestamped), false).unwrap(),
            "00:00:05 - Primeira frase.\n00:01:05 - Segunda frase."
        );
    }

    #[test]
    fn render_output_supports_srt_subtitles() {
        let result = sample_result(vec![
            TranscriptSegment {
                start_s: 1.25,
                end_s: 3.0,
                text: "Hello world".to_string(),
                speaker: Some("S1".to_string()),
            },
            TranscriptSegment {
                start_s: 4.0,
                end_s: 4.5,
                text: "Second line".to_string(),
                speaker: None,
            },
        ]);

        assert_eq!(
            render_output(&result, OutputFormat::Subtitle(SubtitleFormat::Srt), false).unwrap(),
            "1\n00:00:01,250 --> 00:00:03,000\nSPEAKER_01: Hello world\n\n2\n00:00:04,000 --> 00:00:04,500\nSecond line"
        );
    }

    #[test]
    fn render_output_supports_vtt_subtitles() {
        let result = sample_result(vec![TranscriptSegment {
            start_s: 1.25,
            end_s: 3.0,
            text: "Hello world".to_string(),
            speaker: Some("S1".to_string()),
        }]);

        assert_eq!(
            render_output(&result, OutputFormat::Subtitle(SubtitleFormat::Vtt), false).unwrap(),
            "WEBVTT\n\n00:00:01.250 --> 00:00:03.000\nSPEAKER_01: Hello world"
        );
    }

    #[test]
    fn render_output_can_clean_textual_repetition() {
        let result = sample_result(vec![TranscriptSegment {
            start_s: 1.25,
            end_s: 3.0,
            text: "So we we we we we didn't have time.".to_string(),
            speaker: Some("S1".to_string()),
        }]);

        assert_eq!(
            render_output(&result, OutputFormat::Text(TextFormat::Plain), true).unwrap(),
            "So we... we didn't have time."
        );
        let rendered_json = render_output(&result, OutputFormat::Json, true).unwrap();
        assert!(rendered_json.contains("\"So we... we didn't have time.\""));
    }

    #[test]
    fn render_output_can_clean_speakers_output() {
        let result = sample_result(vec![TranscriptSegment {
            start_s: 65.9,
            end_s: 67.0,
            text: "So we we we we we didn't have time.".to_string(),
            speaker: Some("S1".to_string()),
        }]);

        assert_eq!(
            render_output(
                &result,
                OutputFormat::Speakers(SpeakersFormat::Timestamped),
                true
            )
            .unwrap(),
            "00:01:05 - SPEAKER_01: So we... we didn't have time."
        );
    }

    #[test]
    fn render_output_can_clean_srt_subtitles() {
        let result = sample_result(vec![TranscriptSegment {
            start_s: 1.25,
            end_s: 3.0,
            text: "So we we we we we didn't have time.".to_string(),
            speaker: Some("S1".to_string()),
        }]);

        assert_eq!(
            render_output(&result, OutputFormat::Subtitle(SubtitleFormat::Srt), true).unwrap(),
            "1\n00:00:01,250 --> 00:00:03,000\nSPEAKER_01: So we... we didn't have time."
        );
    }

    #[test]
    fn render_output_can_clean_vtt_subtitles() {
        let result = sample_result(vec![TranscriptSegment {
            start_s: 1.25,
            end_s: 3.0,
            text: "So we we we we we didn't have time.".to_string(),
            speaker: Some("S1".to_string()),
        }]);

        assert_eq!(
            render_output(&result, OutputFormat::Subtitle(SubtitleFormat::Vtt), true).unwrap(),
            "WEBVTT\n\n00:00:01.250 --> 00:00:03.000\nSPEAKER_01: So we... we didn't have time."
        );
    }
}
