use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::output::collapse_whitespace;
use crate::types::{DownloadedAudio, InputSource, TranscriptSegment};

#[derive(Debug, Deserialize)]
pub(crate) struct YtDlpVideoInfo {
    pub(crate) id: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) subtitles: Option<BTreeMap<String, Vec<YtDlpSubtitleTrack>>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct YtDlpSubtitleTrack {
    pub(crate) ext: Option<String>,
}

#[derive(Debug)]
pub(crate) struct DirectTranscript {
    pub(crate) text: String,
    pub(crate) segments: Vec<TranscriptSegment>,
}

pub(crate) fn infer_input_source(value: &str) -> InputSource {
    if value.starts_with("https://") || value.starts_with("http://") {
        InputSource::RemoteUrl(value.to_string())
    } else {
        InputSource::LocalPath(PathBuf::from(value))
    }
}

pub(crate) fn pick_manual_subtitle_language(info: &YtDlpVideoInfo) -> Option<String> {
    let subtitles = info.subtitles.as_ref()?;
    let preferred = ["pt-BR", "pt", "en", "en-US"];
    for language in preferred {
        if subtitles
            .get(language)
            .is_some_and(|tracks| !tracks.is_empty())
        {
            return Some(language.to_string());
        }
    }
    subtitles
        .iter()
        .find(|(_, tracks)| !tracks.is_empty())
        .map(|(language, _)| language.to_string())
}

fn choose_subtitle_extension(info: &YtDlpVideoInfo, language: &str) -> String {
    let Some(subtitles) = info.subtitles.as_ref() else {
        return "vtt".to_string();
    };
    let Some(tracks) = subtitles.get(language) else {
        return "vtt".to_string();
    };
    if tracks
        .iter()
        .any(|track| track.ext.as_deref() == Some("vtt"))
    {
        "vtt".to_string()
    } else {
        "json3".to_string()
    }
}

fn run_command(command: &mut Command, context: &str) -> Result<std::process::Output> {
    command
        .output()
        .with_context(|| format!("failed to run {context}"))
}

fn yt_dlp_command() -> Command {
    let mut direct = Command::new("yt-dlp");
    direct.arg("--version");
    if direct.output().is_ok_and(|output| output.status.success()) {
        return Command::new("yt-dlp");
    }

    let mut fallback = Command::new("uvx");
    fallback.args(["yt-dlp", "--version"]);
    if fallback
        .output()
        .is_ok_and(|output| output.status.success())
    {
        let mut command = Command::new("uvx");
        command.arg("yt-dlp");
        return command;
    }

    Command::new("yt-dlp")
}

fn parse_subtitle_timestamp(value: &str) -> Option<f64> {
    let normalized = value.trim().replace(',', ".");
    let parts = normalized.split(':').collect::<Vec<_>>();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [minutes, seconds] => (
            0u64,
            minutes.parse::<u64>().ok()?,
            seconds.parse::<f64>().ok()?,
        ),
        [hours, minutes, seconds] => (
            hours.parse::<u64>().ok()?,
            minutes.parse::<u64>().ok()?,
            seconds.parse::<f64>().ok()?,
        ),
        _ => return None,
    };
    Some(hours as f64 * 3600.0 + minutes as f64 * 60.0 + seconds)
}

pub(crate) fn parse_vtt_segments(contents: &str) -> Vec<TranscriptSegment> {
    contents
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split("\n\n")
        .filter_map(|block| {
            let lines = block
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>();
            if lines.is_empty() {
                return None;
            }

            let timing_index = lines.iter().position(|line| line.contains("-->"))?;
            let (start, end) = {
                let timing_line = lines[timing_index];
                let (start, end) = timing_line.split_once("-->")?;
                let end = end.split_whitespace().next()?;
                (
                    parse_subtitle_timestamp(start)?,
                    parse_subtitle_timestamp(end)?,
                )
            };

            let text = collapse_whitespace(&lines[timing_index + 1..].join(" "));
            if text.is_empty() || end <= start {
                return None;
            }

            Some(TranscriptSegment {
                start_s: start,
                end_s: end,
                text,
                speaker: None,
            })
        })
        .collect()
}

pub(crate) fn join_segment_texts(segments: &[TranscriptSegment]) -> String {
    segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Deserialize)]
struct Json3Transcript {
    events: Vec<Json3Event>,
}

#[derive(Debug, Deserialize)]
struct Json3Event {
    #[serde(rename = "tStartMs")]
    t_start_ms: Option<f64>,
    #[serde(rename = "dDurationMs")]
    d_duration_ms: Option<f64>,
    segs: Option<Vec<Json3CaptionSegment>>,
}

#[derive(Debug, Deserialize)]
struct Json3CaptionSegment {
    utf8: Option<String>,
}

pub(crate) fn parse_json3_segments(contents: &str) -> Result<Vec<TranscriptSegment>> {
    let parsed: Json3Transcript =
        serde_json::from_str(contents).context("failed to parse json3 subtitle payload")?;
    Ok(parsed
        .events
        .into_iter()
        .filter_map(|event| {
            let start = event.t_start_ms? / 1000.0;
            let duration = event.d_duration_ms? / 1000.0;
            let end = start + duration;
            let text = collapse_whitespace(
                &event
                    .segs
                    .into_iter()
                    .flatten()
                    .filter_map(|segment| segment.utf8)
                    .filter(|segment| !segment.trim().is_empty() && segment.trim() != "\n")
                    .collect::<String>(),
            );
            if text.is_empty() || end <= start {
                return None;
            }
            Some(TranscriptSegment {
                start_s: start,
                end_s: end,
                text,
                speaker: None,
            })
        })
        .collect())
}

pub(crate) fn load_remote_video_info(url: &str) -> Result<YtDlpVideoInfo> {
    let output = run_command(
        yt_dlp_command().args(["--dump-single-json", "--no-warnings", "--no-playlist", url]),
        "yt-dlp to inspect remote media metadata",
    )
    .with_context(|| "install yt-dlp, or make `uvx yt-dlp` available, to transcribe remote URLs")?;
    if !output.status.success() {
        bail!(
            "yt-dlp failed while inspecting {url}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    serde_json::from_slice(&output.stdout)
        .with_context(|| format!("failed to parse yt-dlp metadata for {url}"))
}

pub(crate) fn download_manual_remote_transcript(
    url: &str,
    info: &YtDlpVideoInfo,
) -> Result<Option<DirectTranscript>> {
    let Some(language) = pick_manual_subtitle_language(info) else {
        return Ok(None);
    };
    let extension = choose_subtitle_extension(info, &language);
    let tempdir = tempfile::tempdir().context("failed to create temp dir for remote subtitles")?;
    let output_template = tempdir.path().join("%(id)s.%(ext)s");
    let output = run_command(
        yt_dlp_command().args([
            "--no-progress",
            "--no-warnings",
            "--skip-download",
            "--write-subs",
            "--sub-langs",
            &language,
            "--sub-format",
            &extension,
            "--no-playlist",
            "--output",
            output_template.to_string_lossy().as_ref(),
            url,
        ]),
        "yt-dlp for manual subtitles",
    )
    .with_context(|| "install yt-dlp, or make `uvx yt-dlp` available, to transcribe remote URLs")?;
    if !output.status.success() {
        bail!(
            "yt-dlp failed while downloading subtitles for {url}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let subtitle_path = fs::read_dir(tempdir.path())
        .with_context(|| format!("failed to inspect {}", tempdir.path().display()))?
        .filter_map(|entry| entry.ok().map(|value| value.path()))
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some(extension.as_str()))
        .with_context(|| {
            format!("yt-dlp reported subtitles for {url} but did not write a .{extension} file")
        })?;

    let (text, segments) = if extension == "vtt" {
        let contents = fs::read_to_string(&subtitle_path)
            .with_context(|| format!("failed to read {}", subtitle_path.display()))?;
        let segments = parse_vtt_segments(&contents);
        (join_segment_texts(&segments), segments)
    } else {
        let contents = fs::read_to_string(&subtitle_path)
            .with_context(|| format!("failed to read {}", subtitle_path.display()))?;
        let segments = parse_json3_segments(&contents)
            .with_context(|| format!("failed to parse {}", subtitle_path.display()))?;
        (join_segment_texts(&segments), segments)
    };

    if text.trim().is_empty() {
        return Ok(None);
    }

    Ok(Some(DirectTranscript { text, segments }))
}

pub(crate) fn download_remote_audio(url: &str) -> Result<DownloadedAudio> {
    let tempdir = tempfile::tempdir().context("failed to create temp dir for remote audio")?;
    let output_template = tempdir.path().join("%(id)s.%(ext)s");
    let output = run_command(
        yt_dlp_command().args([
            "--no-progress",
            "--no-warnings",
            "--no-playlist",
            "-f",
            "bestaudio/best",
            "--output",
            output_template.to_string_lossy().as_ref(),
            "--print",
            "after_move:filepath",
            url,
        ]),
        "yt-dlp to download remote audio",
    )
    .with_context(|| "install yt-dlp, or make `uvx yt-dlp` available, to transcribe remote URLs")?;
    if !output.status.success() {
        bail!(
            "yt-dlp failed while downloading audio for {url}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let audio_path = String::from_utf8_lossy(&output.stdout)
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(|line| PathBuf::from(line.trim()))
        .with_context(|| format!("yt-dlp did not report the downloaded audio path for {url}"))?;
    Ok(DownloadedAudio {
        audio_path,
        _tempdir: tempdir,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        infer_input_source, join_segment_texts, parse_json3_segments, parse_vtt_segments,
        pick_manual_subtitle_language, YtDlpSubtitleTrack, YtDlpVideoInfo,
    };
    use crate::types::{InputSource, TranscriptSegment};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn infer_input_source_detects_remote_urls() {
        assert_eq!(
            infer_input_source("https://www.youtube.com/watch?v=QSdh8Gj0mEg"),
            InputSource::RemoteUrl("https://www.youtube.com/watch?v=QSdh8Gj0mEg".to_string())
        );
        assert_eq!(
            infer_input_source("lecture.mp3"),
            InputSource::LocalPath(PathBuf::from("lecture.mp3"))
        );
    }

    #[test]
    fn pick_manual_subtitle_language_prefers_manual_tracks() {
        let mut subtitles = BTreeMap::new();
        subtitles.insert(
            "en".to_string(),
            vec![YtDlpSubtitleTrack {
                ext: Some("vtt".to_string()),
            }],
        );
        subtitles.insert(
            "pt-BR".to_string(),
            vec![YtDlpSubtitleTrack {
                ext: Some("json3".to_string()),
            }],
        );

        let info = YtDlpVideoInfo {
            id: Some("abc123".to_string()),
            title: Some("Demo".to_string()),
            subtitles: Some(subtitles),
        };

        assert_eq!(
            pick_manual_subtitle_language(&info),
            Some("pt-BR".to_string())
        );
    }

    #[test]
    fn parse_vtt_segments_text_strips_headers_and_timestamps() {
        let segments = parse_vtt_segments(
            "WEBVTT\nKind: captions\nLanguage: en\n\n1\n00:00:00.000 --> 00:00:01.000\nHello world\n\n2\n00:00:01.000 --> 00:00:02.000\nSecond line",
        );
        let text = join_segment_texts(&segments);
        assert_eq!(text, "Hello world Second line");
    }

    #[test]
    fn parse_vtt_segments_extracts_timed_cues() {
        let segments = parse_vtt_segments(
            "WEBVTT\n\nintro\n00:00:00.000 --> 00:00:02.500 align:start position:0%\nHello\nworld\n\n00:00:03.000 --> 00:00:04.250\nSecond line",
        );
        assert_eq!(
            segments,
            vec![
                TranscriptSegment {
                    start_s: 0.0,
                    end_s: 2.5,
                    text: "Hello world".to_string(),
                    speaker: None,
                },
                TranscriptSegment {
                    start_s: 3.0,
                    end_s: 4.25,
                    text: "Second line".to_string(),
                    speaker: None,
                },
            ]
        );
    }

    #[test]
    fn parse_json3_segments_extracts_timed_cues() {
        let segments = parse_json3_segments(
            r#"{
              "events": [
                {
                  "tStartMs": 1250,
                  "dDurationMs": 1750,
                  "segs": [{"utf8": "Hello "}, {"utf8": "world"}]
                },
                {
                  "tStartMs": 4000,
                  "dDurationMs": 500,
                  "segs": [{"utf8": "Second line"}]
                }
              ]
            }"#,
        )
        .unwrap();
        assert_eq!(
            segments,
            vec![
                TranscriptSegment {
                    start_s: 1.25,
                    end_s: 3.0,
                    text: "Hello world".to_string(),
                    speaker: None,
                },
                TranscriptSegment {
                    start_s: 4.0,
                    end_s: 4.5,
                    text: "Second line".to_string(),
                    speaker: None,
                },
            ]
        );
    }
}
