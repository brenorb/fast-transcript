use anyhow::{Context, Result};
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
    if value == "-" {
        InputSource::Stdin
    } else if value.starts_with("https://") || value.starts_with("http://") {
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

const LINK_HINT: &str = "Check that the link plays in a browser without signing in, or download the file and pass its path to fscript";
const YT_DLP_MISSING: &str = "transcribing web links needs yt-dlp, which is not installed. Install it (for example: brew install yt-dlp), or download the file and pass its path to fscript";

fn run_yt_dlp(command: &mut Command) -> Result<std::process::Output> {
    command.output().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!(YT_DLP_MISSING)
        } else {
            anyhow::Error::new(error).context("could not start yt-dlp to read the link")
        }
    })
}

/// The site's own reason (e.g. "Video unavailable"), without the downloader's
/// prefixes, video ids, or advice about its own command-line flags.
pub(crate) fn download_failure_reason(stderr: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(stderr);
    let line = text
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| line.starts_with("ERROR:"))?;
    let mut reason = line.trim_start_matches("ERROR:").trim();
    if reason.starts_with('[') {
        if let Some(end) = reason.find("] ") {
            reason = &reason[end + 2..];
            if let Some((id, rest)) = reason.split_once(": ") {
                if !id.contains(' ') {
                    reason = rest;
                }
            }
        }
    }
    let reason = reason
        .split(". ")
        .map(str::trim)
        .filter(|sentence| {
            !sentence.contains("--")
                && !sentence.contains("yt-dlp")
                && !sentence.to_ascii_lowercase().starts_with("see ")
        })
        .collect::<Vec<_>>()
        .join(". ");
    let reason = reason.trim().trim_end_matches('.').to_string();
    (!reason.is_empty()).then_some(reason)
}

fn link_failure(action: &str, url: &str, stderr: &[u8]) -> anyhow::Error {
    match download_failure_reason(stderr) {
        Some(reason) => anyhow::anyhow!("could not {action} {url}: {reason}. {LINK_HINT}"),
        None => anyhow::anyhow!("could not {action} {url}. {LINK_HINT}"),
    }
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
    let parsed: Json3Transcript = serde_json::from_str(contents)
        .map_err(crate::errors::technical)
        .context("the published subtitles are in a format fscript cannot read")?;
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
    let output = run_yt_dlp(yt_dlp_command().args([
        "--dump-single-json",
        "--no-warnings",
        "--no-playlist",
        url,
    ]))?;
    if !output.status.success() {
        return Err(link_failure("open", url, &output.stderr));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(crate::errors::technical)
        .with_context(|| {
            format!("could not read the details of {url}. Update yt-dlp and try again, or download the file and pass its path to fscript")
        })
}

pub(crate) fn download_manual_remote_transcript(
    url: &str,
    info: &YtDlpVideoInfo,
) -> Result<Option<DirectTranscript>> {
    let Some(language) = pick_manual_subtitle_language(info) else {
        return Ok(None);
    };
    let extension = choose_subtitle_extension(info, &language);
    let tempdir = crate::errors::temp(tempfile::tempdir())?;
    let output_template = tempdir.path().join("%(id)s.%(ext)s");
    let output = run_yt_dlp(yt_dlp_command().args([
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
    ]))?;
    if !output.status.success() {
        return Err(link_failure(
            "download the published subtitles for",
            url,
            &output.stderr,
        ));
    }

    let missing = || format!("could not find the downloaded subtitles for {url}");
    let subtitle_path = fs::read_dir(tempdir.path())
        .map_err(crate::errors::technical)
        .with_context(missing)?
        .filter_map(|entry| entry.ok().map(|value| value.path()))
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some(extension.as_str()))
        .with_context(missing)?;

    let contents = fs::read_to_string(&subtitle_path)
        .map_err(crate::errors::technical)
        .with_context(missing)?;
    let segments = if extension == "vtt" {
        parse_vtt_segments(&contents)
    } else {
        parse_json3_segments(&contents)?
    };
    let text = join_segment_texts(&segments);

    if text.trim().is_empty() {
        return Ok(None);
    }

    Ok(Some(DirectTranscript { text, segments }))
}

pub(crate) fn download_remote_audio(url: &str) -> Result<DownloadedAudio> {
    let tempdir = crate::errors::temp(tempfile::tempdir())?;
    let output_template = tempdir.path().join("%(id)s.%(ext)s");
    let output = run_yt_dlp(yt_dlp_command().args([
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
    ]))?;
    if !output.status.success() {
        return Err(link_failure("download audio from", url, &output.stderr));
    }
    let audio_path = String::from_utf8_lossy(&output.stdout)
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(|line| PathBuf::from(line.trim()))
        .with_context(|| {
            format!("could not find the downloaded audio for {url}; try again, or download the file and pass its path to fscript")
        })?;
    Ok(DownloadedAudio {
        audio_path,
        _tempdir: tempdir,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn download_failure_reason_keeps_the_sites_reason_only() {
        use super::download_failure_reason;
        assert_eq!(
            download_failure_reason(b"WARNING: x\nERROR: [youtube] dQw4w9WgXcQ: Video unavailable. This video is private\n").as_deref(),
            Some("Video unavailable. This video is private")
        );
        assert_eq!(
            download_failure_reason(b"ERROR: [youtube] abc: Sign in to confirm you are not a bot. Use --cookies-from-browser or --cookies for the authentication. See  https://github.com/yt-dlp/yt-dlp/wiki/FAQ for how to manually pass cookies").as_deref(),
            Some("Sign in to confirm you are not a bot")
        );
        assert_eq!(
            download_failure_reason(b"ERROR: [generic] abc: HTTP Error 403: Forbidden").as_deref(),
            Some("HTTP Error 403: Forbidden")
        );
        assert_eq!(
            download_failure_reason(b"Traceback (most recent call last):"),
            None
        );
    }

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
