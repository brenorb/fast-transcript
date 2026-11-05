use crate::errors::Explain;
use crate::types::TranscriptSegment;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::env;
use std::io;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::tempdir;

const DEFAULT_FLUIDAUDIO_BINARY: &str = "fluidaudiocli";
const COREML_BACKEND_NAME: &str =
    "FluidInference/speaker-diarization-coreml via fluidaudiocli process --mode offline";
const LSEEND_DIHARD3_BACKEND_NAME: &str =
    "FluidInference/ls-eend-coreml dihard3 via fluidaudiocli lseend";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DiarizationBackend {
    Coreml,
    LseendDihard3,
}

impl DiarizationBackend {
    pub fn from_cli_value(value: &str) -> Option<Self> {
        match value {
            "coreml" => Some(Self::Coreml),
            "lseend-dihard3" => Some(Self::LseendDihard3),
            _ => None,
        }
    }

    pub fn backend_name(self) -> &'static str {
        match self {
            Self::Coreml => COREML_BACKEND_NAME,
            Self::LseendDihard3 => LSEEND_DIHARD3_BACKEND_NAME,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiarizationRequest {
    pub backend: DiarizationBackend,
    pub num_speakers: Option<usize>,
    pub threshold: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpeakerDiarizationMetadata {
    pub backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_num_speakers: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_speaker_count: Option<usize>,
    pub segment_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiarizationSegment {
    pub start_s: f64,
    pub end_s: f64,
    pub speaker: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiarizationResult {
    pub speaker_count: Option<usize>,
    pub segments: Vec<DiarizationSegment>,
}

pub trait SpeakerDiarizer {
    fn diarize(&self, audio_path: &Path, request: &DiarizationRequest)
        -> Result<DiarizationResult>;
}

trait CommandRunner {
    fn run(&self, program: &str, args: &[String]) -> io::Result<Output>;
}

pub(crate) struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&self, program: &str, args: &[String]) -> io::Result<Output> {
        Command::new(program).args(args).output()
    }
}

pub(crate) struct FluidAudioDiarizer<R = SystemCommandRunner> {
    binary: String,
    runner: R,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FluidaudioBinaryStatus {
    Available,
    MissingDefaultBinary,
    MissingConfiguredBinaryOnPath { binary: String },
    InvalidConfiguredBinaryPath { binary: String },
}

impl FluidAudioDiarizer<SystemCommandRunner> {
    pub fn new() -> Self {
        let binary = configured_fluidaudio_binary();
        Self {
            binary,
            runner: SystemCommandRunner,
        }
    }
}

pub(crate) fn configured_fluidaudio_binary() -> String {
    env::var("FSCRIPT_DIARIZATION_BINARY").unwrap_or_else(|_| DEFAULT_FLUIDAUDIO_BINARY.to_string())
}

pub(crate) fn fluidaudio_binary_status() -> FluidaudioBinaryStatus {
    let configured_binary = configured_fluidaudio_binary();
    if binary_is_available(&configured_binary) {
        return FluidaudioBinaryStatus::Available;
    }

    if env::var_os("FSCRIPT_DIARIZATION_BINARY").is_some() {
        let path = Path::new(&configured_binary);
        if path.components().count() > 1 {
            FluidaudioBinaryStatus::InvalidConfiguredBinaryPath {
                binary: configured_binary,
            }
        } else {
            FluidaudioBinaryStatus::MissingConfiguredBinaryOnPath {
                binary: configured_binary,
            }
        }
    } else {
        FluidaudioBinaryStatus::MissingDefaultBinary
    }
}

pub(crate) fn missing_diarization_notice(status: &FluidaudioBinaryStatus) -> Option<String> {
    match status {
        FluidaudioBinaryStatus::Available => None,
        FluidaudioBinaryStatus::MissingDefaultBinary => Some(
            "note: speaker labels are off because fluidaudiocli was not found; writing the transcript without speakers. On macOS, install FluidAudio (or reinstall fast-transcript) to get speaker labels; pass --no-diarization to hide this note.".to_string(),
        ),
        FluidaudioBinaryStatus::MissingConfiguredBinaryOnPath { binary } => Some(
            format!(
                "warning: speaker labels are off because FSCRIPT_DIARIZATION_BINARY is set to {binary}, which is not on PATH; writing the transcript without speakers."
            ),
        ),
        FluidaudioBinaryStatus::InvalidConfiguredBinaryPath { binary } => Some(
            format!(
                "warning: speaker labels are off because FSCRIPT_DIARIZATION_BINARY points to {binary}, which is not a program fscript can run; writing the transcript without speakers."
            ),
        ),
    }
}

fn binary_is_available(binary: &str) -> bool {
    let path = Path::new(binary);
    if path.components().count() > 1 {
        return is_executable_file(path);
    }

    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| is_executable_file(&dir.join(binary))))
        .unwrap_or(false)
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

impl<R> FluidAudioDiarizer<R> {
    #[cfg(test)]
    fn with_runner(binary: impl Into<String>, runner: R) -> Self {
        Self {
            binary: binary.into(),
            runner,
        }
    }
}

impl<R: CommandRunner> SpeakerDiarizer for FluidAudioDiarizer<R> {
    fn diarize(
        &self,
        audio_path: &Path,
        request: &DiarizationRequest,
    ) -> Result<DiarizationResult> {
        // Child parsers must receive a path, even when the filename begins with '-'.
        let audio_path = std::path::absolute(audio_path)
            .with_context(|| format!("cannot open {}", audio_path.display()))?;
        let tempdir = crate::errors::temp(tempdir())?;
        let output_path = tempdir.path().join("diarization.json");
        let mut args = match request.backend {
            DiarizationBackend::Coreml => vec![
                "process".to_string(),
                audio_path.display().to_string(),
                "--mode".to_string(),
                "offline".to_string(),
                "--output".to_string(),
                output_path.display().to_string(),
            ],
            DiarizationBackend::LseendDihard3 => vec![
                "lseend".to_string(),
                audio_path.display().to_string(),
                "--variant".to_string(),
                "dihard3".to_string(),
                "--output".to_string(),
                output_path.display().to_string(),
            ],
        };
        if let Some(threshold) = request.threshold {
            args.push("--threshold".to_string());
            args.push(threshold.to_string());
        }
        if let Some(num_speakers) = request.num_speakers {
            args.push("--num-speakers".to_string());
            args.push(num_speakers.to_string());
        }

        let output = match self.runner.run(&self.binary, &args) {
            Ok(output) => output,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                bail!(
                    "speaker labels need fluidaudiocli, but {} was not found. Install FluidAudio, set FSCRIPT_DIARIZATION_BINARY to its fluidaudiocli path, or rerun with --no-diarization",
                    self.binary
                );
            }
            Err(err) => {
                return Err(err).explain(
                    || format!("could not start the speaker-label tool {}", self.binary),
                    NO_DIARIZATION_HINT,
                );
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if should_skip_diarization_error(&stderr) {
                return Err(anyhow::Error::new(NoSpeech));
            }
            if crate::errors::debug_enabled() {
                std::eprintln!("{}", stderr.trim());
            }
            bail!("speaker detection failed. {NO_DIARIZATION_HINT}");
        }

        let payload = std::fs::read_to_string(&output_path)
            .map_err(crate::errors::technical)
            .context(NO_RESULT)?;
        parse_diarization_output(&payload)
    }
}

const NO_DIARIZATION_HINT: &str =
    "Rerun with --no-diarization to get the transcript without speaker labels";
const NO_RESULT: &str = "speaker detection returned no usable result. Rerun with --no-diarization to get the transcript without speaker labels";

/// The speaker tool found no speech; the transcript continues without labels.
#[derive(Debug)]
struct NoSpeech;

impl std::fmt::Display for NoSpeech {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("no speech detected")
    }
}

impl std::error::Error for NoSpeech {}

pub fn parse_diarization_output(payload: &str) -> Result<DiarizationResult> {
    let parsed: FluidAudioOutput = serde_json::from_str(payload)
        .map_err(crate::errors::technical)
        .context(NO_RESULT)?;
    let mut segments = Vec::with_capacity(parsed.segments.len());
    for segment in parsed.segments {
        let speaker = segment
            .speaker_id
            .as_deref()
            .or(segment.speaker.as_deref())
            .unwrap_or("")
            .trim();
        if speaker.is_empty() {
            continue;
        }
        if segment.end_time_seconds <= segment.start_time_seconds {
            continue;
        }
        segments.push(DiarizationSegment {
            start_s: segment.start_time_seconds,
            end_s: segment.end_time_seconds,
            speaker: speaker.to_string(),
        });
    }
    Ok(DiarizationResult {
        speaker_count: parsed.speaker_count,
        segments,
    })
}

pub fn merge_speakers_into_segments(
    transcript_segments: &[TranscriptSegment],
    diarization_segments: &[DiarizationSegment],
) -> Vec<TranscriptSegment> {
    transcript_segments
        .iter()
        .map(|segment| {
            let mut overlap_by_speaker: BTreeMap<String, f64> = BTreeMap::new();
            for diarization in diarization_segments {
                let overlap = overlap_duration(
                    segment.start_s,
                    segment.end_s,
                    diarization.start_s,
                    diarization.end_s,
                );
                if overlap > 0.0 {
                    *overlap_by_speaker
                        .entry(diarization.speaker.clone())
                        .or_insert(0.0) += overlap;
                }
            }

            let speaker = overlap_by_speaker
                .into_iter()
                .max_by(|left, right| left.1.total_cmp(&right.1))
                .map(|(speaker, _)| speaker);

            let mut merged = segment.clone();
            merged.speaker = speaker;
            merged
        })
        .collect()
}

pub fn maybe_diarize_segments(
    diarizer: &dyn SpeakerDiarizer,
    audio_path: &Path,
    transcript_segments: Vec<TranscriptSegment>,
    request: Option<&DiarizationRequest>,
) -> Result<(Vec<TranscriptSegment>, Option<SpeakerDiarizationMetadata>)> {
    let Some(request) = request else {
        return Ok((transcript_segments, None));
    };

    let diarization = match diarizer.diarize(audio_path, request) {
        Ok(diarization) => diarization,
        Err(err) if err.is::<NoSpeech>() || should_skip_diarization_error(&err.to_string()) => {
            std::eprintln!(
                "warning: no speech found for speaker labels; writing the transcript without speakers"
            );
            return Ok((transcript_segments, None));
        }
        Err(err) => return Err(err),
    };
    let merged_segments = merge_speakers_into_segments(&transcript_segments, &diarization.segments);
    let metadata = SpeakerDiarizationMetadata {
        backend: request.backend.backend_name().to_string(),
        requested_num_speakers: request.num_speakers,
        detected_speaker_count: diarization.speaker_count,
        segment_count: diarization.segments.len(),
    };
    Ok((merged_segments, Some(metadata)))
}

fn should_skip_diarization_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("nospeechdetected") || normalized.contains("no speech detected")
}

fn overlap_duration(start_a: f64, end_a: f64, start_b: f64, end_b: f64) -> f64 {
    (end_a.min(end_b) - start_a.max(start_b)).max(0.0)
}

#[derive(Debug, Deserialize)]
struct FluidAudioOutput {
    #[serde(rename = "speakerCount")]
    speaker_count: Option<usize>,
    #[serde(default)]
    segments: Vec<FluidAudioSegment>,
}

#[derive(Debug, Deserialize)]
struct FluidAudioSegment {
    #[serde(rename = "startTimeSeconds")]
    start_time_seconds: f64,
    #[serde(rename = "endTimeSeconds")]
    end_time_seconds: f64,
    #[serde(rename = "speakerId")]
    speaker_id: Option<String>,
    speaker: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::os::unix::process::ExitStatusExt;
    use std::rc::Rc;

    struct StubDiarizer {
        calls: Cell<usize>,
        last_request: RefCell<Option<DiarizationRequest>>,
        result: RefCell<Option<DiarizationResult>>,
    }

    impl StubDiarizer {
        fn new(result: DiarizationResult) -> Self {
            Self {
                calls: Cell::new(0),
                last_request: RefCell::new(None),
                result: RefCell::new(Some(result)),
            }
        }
    }

    impl SpeakerDiarizer for StubDiarizer {
        fn diarize(
            &self,
            _audio_path: &Path,
            request: &DiarizationRequest,
        ) -> Result<DiarizationResult> {
            self.calls.set(self.calls.get() + 1);
            self.last_request.replace(Some(request.clone()));
            Ok(self
                .result
                .borrow()
                .clone()
                .expect("stub diarizer result should be present"))
        }
    }

    #[derive(Default)]
    struct FakeRunnerState {
        seen_program: RefCell<Option<String>>,
        seen_args: RefCell<Vec<String>>,
    }

    struct FakeRunner {
        output: io::Result<Output>,
        diarization_json: Option<String>,
        state: Rc<FakeRunnerState>,
    }

    impl FakeRunner {
        fn success(diarization_json: &str) -> (Self, Rc<FakeRunnerState>) {
            let state = Rc::new(FakeRunnerState::default());
            let output = Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
            };
            (
                Self {
                    output: Ok(output),
                    diarization_json: Some(diarization_json.to_string()),
                    state: state.clone(),
                },
                state,
            )
        }

        fn not_found() -> (Self, Rc<FakeRunnerState>) {
            let state = Rc::new(FakeRunnerState::default());
            (
                Self {
                    output: Err(io::Error::new(io::ErrorKind::NotFound, "missing")),
                    diarization_json: None,
                    state: state.clone(),
                },
                state,
            )
        }

        fn command_failure(stderr: &str) -> (Self, Rc<FakeRunnerState>) {
            let state = Rc::new(FakeRunnerState::default());
            let output = Output {
                status: std::process::ExitStatus::from_raw(1),
                stdout: Vec::new(),
                stderr: stderr.as_bytes().to_vec(),
            };
            (
                Self {
                    output: Ok(output),
                    diarization_json: None,
                    state: state.clone(),
                },
                state,
            )
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, program: &str, args: &[String]) -> io::Result<Output> {
            self.state.seen_program.replace(Some(program.to_string()));
            self.state.seen_args.replace(args.to_vec());
            if let Some(json) = &self.diarization_json {
                if let Some(output_index) = args.iter().position(|arg| arg == "--output") {
                    if let Some(output_path) = args.get(output_index + 1) {
                        std::fs::write(output_path, json)?;
                    }
                }
            }
            self.output
                .as_ref()
                .map(|output| Output {
                    status: output.status,
                    stdout: output.stdout.clone(),
                    stderr: output.stderr.clone(),
                })
                .map_err(|err| io::Error::new(err.kind(), err.to_string()))
        }
    }

    #[test]
    fn parse_diarization_output_normalizes_segments() {
        let parsed = parse_diarization_output(
            r#"{
                "speakerCount": 2,
                "segments": [
                    {"startTimeSeconds": 0.0, "endTimeSeconds": 1.5, "speakerId": "S1", "embedding": [1]},
                    {"startTimeSeconds": 1.5, "endTimeSeconds": 3.0, "speakerId": "S2"},
                    {"startTimeSeconds": 4.0, "endTimeSeconds": 4.0, "speakerId": "skip-me"}
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(parsed.speaker_count, Some(2));
        assert_eq!(
            parsed.segments,
            vec![
                DiarizationSegment {
                    start_s: 0.0,
                    end_s: 1.5,
                    speaker: "S1".to_string(),
                },
                DiarizationSegment {
                    start_s: 1.5,
                    end_s: 3.0,
                    speaker: "S2".to_string(),
                },
            ]
        );
    }

    #[test]
    fn parse_diarization_output_supports_lseend_speaker_field() {
        let parsed = parse_diarization_output(
            r#"{
                "segments": [
                    {"startTimeSeconds": 0.0, "endTimeSeconds": 1.5, "speaker": "Speaker 0"},
                    {"startTimeSeconds": 1.5, "endTimeSeconds": 3.0, "speaker": "Speaker 1"}
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(parsed.speaker_count, None);
        assert_eq!(parsed.segments.len(), 2);
        assert_eq!(parsed.segments[0].speaker, "Speaker 0");
        assert_eq!(parsed.segments[1].speaker, "Speaker 1");
    }

    #[test]
    fn merge_speakers_into_segments_uses_largest_temporal_overlap() {
        let transcript_segments = vec![
            TranscriptSegment {
                start_s: 0.0,
                end_s: 2.0,
                text: "hello".to_string(),
                speaker: None,
            },
            TranscriptSegment {
                start_s: 2.0,
                end_s: 4.5,
                text: "world".to_string(),
                speaker: None,
            },
        ];
        let diarization_segments = vec![
            DiarizationSegment {
                start_s: 0.0,
                end_s: 0.8,
                speaker: "S1".to_string(),
            },
            DiarizationSegment {
                start_s: 0.8,
                end_s: 2.0,
                speaker: "S2".to_string(),
            },
            DiarizationSegment {
                start_s: 2.0,
                end_s: 4.5,
                speaker: "S1".to_string(),
            },
        ];

        let merged = merge_speakers_into_segments(&transcript_segments, &diarization_segments);

        assert_eq!(merged[0].speaker.as_deref(), Some("S2"));
        assert_eq!(merged[1].speaker.as_deref(), Some("S1"));
    }

    #[test]
    fn maybe_diarize_segments_skips_backend_when_not_requested() {
        let diarizer = StubDiarizer::new(DiarizationResult {
            speaker_count: Some(1),
            segments: vec![],
        });
        let transcript_segments = vec![TranscriptSegment {
            start_s: 0.0,
            end_s: 1.0,
            text: "hello".to_string(),
            speaker: None,
        }];

        let (segments, metadata) = maybe_diarize_segments(
            &diarizer,
            Path::new("/tmp/audio.wav"),
            transcript_segments.clone(),
            None,
        )
        .unwrap();

        assert_eq!(segments, transcript_segments);
        assert!(metadata.is_none());
        assert_eq!(diarizer.calls.get(), 0);
    }

    #[test]
    fn maybe_diarize_segments_invokes_backend_and_attaches_metadata() {
        let diarizer = StubDiarizer::new(DiarizationResult {
            speaker_count: Some(2),
            segments: vec![
                DiarizationSegment {
                    start_s: 0.0,
                    end_s: 1.0,
                    speaker: "S1".to_string(),
                },
                DiarizationSegment {
                    start_s: 1.0,
                    end_s: 2.0,
                    speaker: "S2".to_string(),
                },
            ],
        });
        let transcript_segments = vec![
            TranscriptSegment {
                start_s: 0.0,
                end_s: 1.0,
                text: "bom".to_string(),
                speaker: None,
            },
            TranscriptSegment {
                start_s: 1.0,
                end_s: 2.0,
                text: "dia".to_string(),
                speaker: None,
            },
        ];
        let request = DiarizationRequest {
            backend: DiarizationBackend::Coreml,
            num_speakers: Some(2),
            threshold: None,
        };

        let (segments, metadata) = maybe_diarize_segments(
            &diarizer,
            Path::new("/tmp/audio.wav"),
            transcript_segments,
            Some(&request),
        )
        .unwrap();

        assert_eq!(diarizer.calls.get(), 1);
        assert_eq!(
            diarizer.last_request.borrow().clone(),
            Some(DiarizationRequest {
                backend: DiarizationBackend::Coreml,
                num_speakers: Some(2),
                threshold: None,
            })
        );
        assert_eq!(segments[0].speaker.as_deref(), Some("S1"));
        assert_eq!(segments[1].speaker.as_deref(), Some("S2"));
        assert_eq!(
            metadata,
            Some(SpeakerDiarizationMetadata {
                backend: COREML_BACKEND_NAME.to_string(),
                requested_num_speakers: Some(2),
                detected_speaker_count: Some(2),
                segment_count: 2,
            })
        );
    }

    #[test]
    fn fluidaudio_diarizer_returns_clear_error_when_binary_is_missing() {
        let (runner, _) = FakeRunner::not_found();
        let diarizer = FluidAudioDiarizer::with_runner("fluidaudiocli", runner);
        let err = diarizer
            .diarize(
                Path::new("/tmp/audio.wav"),
                &DiarizationRequest {
                    backend: DiarizationBackend::Coreml,
                    num_speakers: None,
                    threshold: None,
                },
            )
            .unwrap_err();

        assert!(err
            .to_string()
            .contains("speaker labels need fluidaudiocli, but fluidaudiocli was not found"));
        assert!(err.to_string().contains("--no-diarization"));
    }

    #[test]
    fn configured_binary_is_available_via_explicit_path() {
        let tempdir = tempfile::tempdir().unwrap();
        let binary_path = tempdir.path().join("fluidaudiocli");
        std::fs::write(&binary_path, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&binary_path).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&binary_path, permissions).unwrap();
        }

        assert!(binary_is_available(binary_path.to_str().unwrap()));
    }

    #[test]
    fn configured_binary_is_unavailable_when_path_does_not_exist() {
        assert!(!binary_is_available("/definitely/missing/fluidaudiocli"));
    }

    #[test]
    fn missing_diarization_notice_describes_missing_default_binary() {
        let notice = missing_diarization_notice(&FluidaudioBinaryStatus::MissingDefaultBinary);
        assert_eq!(
            notice.as_deref(),
            Some(
                "note: speaker labels are off because fluidaudiocli was not found; writing the transcript without speakers. On macOS, install FluidAudio (or reinstall fast-transcript) to get speaker labels; pass --no-diarization to hide this note."
            )
        );
    }

    #[test]
    fn missing_diarization_notice_describes_missing_configured_binary_on_path() {
        let notice =
            missing_diarization_notice(&FluidaudioBinaryStatus::MissingConfiguredBinaryOnPath {
                binary: "custom-fluidaudiocli".to_string(),
            });
        assert_eq!(
            notice.as_deref(),
            Some(
                "warning: speaker labels are off because FSCRIPT_DIARIZATION_BINARY is set to custom-fluidaudiocli, which is not on PATH; writing the transcript without speakers."
            )
        );
    }

    #[test]
    fn missing_diarization_notice_describes_invalid_configured_binary_path() {
        let notice =
            missing_diarization_notice(&FluidaudioBinaryStatus::InvalidConfiguredBinaryPath {
                binary: "/definitely/missing/fluidaudiocli".to_string(),
            });
        assert_eq!(
            notice.as_deref(),
            Some(
                "warning: speaker labels are off because FSCRIPT_DIARIZATION_BINARY points to /definitely/missing/fluidaudiocli, which is not a program fscript can run; writing the transcript without speakers."
            )
        );
    }

    #[test]
    fn fluidaudio_diarizer_passes_num_speakers_to_backend() {
        let (runner, state) = FakeRunner::success(
            r#"{"speakerCount":2,"segments":[{"startTimeSeconds":0.0,"endTimeSeconds":1.0,"speakerId":"S1"}]}"#,
        );
        let diarizer = FluidAudioDiarizer::with_runner("fluidaudiocli", runner);

        let result = diarizer
            .diarize(
                Path::new("/tmp/audio.wav"),
                &DiarizationRequest {
                    backend: DiarizationBackend::Coreml,
                    num_speakers: Some(2),
                    threshold: None,
                },
            )
            .unwrap();

        let seen_args = state.seen_args.borrow();
        assert_eq!(
            state.seen_program.borrow().as_deref(),
            Some("fluidaudiocli")
        );
        assert_eq!(
            &seen_args[0..5],
            ["process", "/tmp/audio.wav", "--mode", "offline", "--output"]
        );
        assert_eq!(seen_args[6], "--num-speakers");
        assert_eq!(seen_args[7], "2");
        assert_eq!(result.speaker_count, Some(2));
        assert_eq!(result.segments.len(), 1);
    }

    #[test]
    fn fluidaudio_diarizer_can_run_lseend_backend_with_threshold() {
        let (runner, state) = FakeRunner::success(
            r#"{"segments":[{"startTimeSeconds":0.0,"endTimeSeconds":1.0,"speaker":"Speaker 0"}]}"#,
        );
        let diarizer = FluidAudioDiarizer::with_runner("fluidaudiocli", runner);

        let result = diarizer
            .diarize(
                Path::new("/tmp/audio.wav"),
                &DiarizationRequest {
                    backend: DiarizationBackend::LseendDihard3,
                    num_speakers: None,
                    threshold: Some(0.3),
                },
            )
            .unwrap();

        let seen_args = state.seen_args.borrow();
        assert_eq!(
            &seen_args[0..5],
            [
                "lseend",
                "/tmp/audio.wav",
                "--variant",
                "dihard3",
                "--output"
            ]
        );
        assert_eq!(seen_args[6], "--threshold");
        assert_eq!(seen_args[7], "0.3");
        assert_eq!(result.segments[0].speaker, "Speaker 0");
    }

    #[test]
    fn maybe_diarize_segments_skips_no_speech_backend_failures() {
        let (runner, _) = FakeRunner::command_failure(
            "ERROR: Failed to process audio file (offline mode): noSpeechDetected",
        );
        let diarizer = FluidAudioDiarizer::with_runner("fluidaudiocli", runner);
        let transcript_segments = vec![TranscriptSegment {
            start_s: 0.0,
            end_s: 1.0,
            text: String::new(),
            speaker: None,
        }];
        let request = DiarizationRequest {
            backend: DiarizationBackend::Coreml,
            num_speakers: None,
            threshold: None,
        };

        let (segments, metadata) = maybe_diarize_segments(
            &diarizer,
            Path::new("/tmp/audio.wav"),
            transcript_segments.clone(),
            Some(&request),
        )
        .unwrap();

        assert_eq!(segments, transcript_segments);
        assert_eq!(metadata, None);
    }

    #[test]
    fn skip_diarization_error_matches_no_speech_variants() {
        assert!(should_skip_diarization_error("noSpeechDetected"));
        assert!(should_skip_diarization_error(
            "No speech detected while clustering"
        ));
        assert!(!should_skip_diarization_error("model download failed"));
    }
}
