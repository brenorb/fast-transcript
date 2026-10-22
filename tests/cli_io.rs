#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Fixture {
    root: tempfile::TempDir,
    audio: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let audio = root.path().join("audio.wav");
        let mut wav = hound::WavWriter::create(
            &audio,
            hound::WavSpec {
                channels: 1,
                sample_rate: 16_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..16_000 {
            wav.write_sample(0_i16).unwrap();
        }
        wav.finalize().unwrap();
        let fixture = Self { root, audio };
        fixture.script("ffprobe", r#"printf '%s\n' '{"streams":[{"codec_type":"audio","sample_rate":"16000","channels":1,"codec_name":"pcm_s16le","bits_per_sample":16}]}'"#);
        fixture.script("python", r#"printf '%s\n' '{"ready":true,"device":"cpu","model_dir":"fixture","load_seconds":0.1}'
while IFS= read -r line; do
  printf '%s\n' '{"text":"Pipe works.","segments":[{"start":0,"end":1,"text":"Pipe works."}]}'
done"#);
        fixture
    }

    fn script(&self, name: &str, script: &str) {
        let path = self.root.path().join(name);
        fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fscript"));
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("FSCRIPT_") {
                command.env_remove(name);
            }
        }
        command
            .args(args)
            .args(["--model", "ultra"])
            .current_dir(self.root.path())
            .env("PATH", self.root.path())
            .env("FSCRIPT_PYTHON_BINARY", self.root.path().join("python"))
            .env("FSCRIPT_CONFIG_FILE", self.root.path().join("config.json"))
            .env(
                "FSCRIPT_DIARIZATION_BINARY",
                self.root.path().join("missing-helper"),
            );
        command
    }

    fn run(&self, args: &[&str], input: &[u8]) -> Output {
        let mut child = self
            .command(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn stdin_media_defaults_to_transcript_stdout_and_cleans_up_spool() {
    let fixture = Fixture::new();
    let output = fixture.run(
        &["-", "--json", "--no-diarization"],
        &fs::read(&fixture.audio).unwrap(),
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["text"], "Pipe works.");
    assert_eq!(result["input_source"], "-");
    assert!(!Path::new(result["audio_path"].as_str().unwrap()).exists());
    assert!(!fixture.root.path().join("stdin.transcript.json").exists());
}

#[test]
fn stdin_media_supports_explicit_files_and_directories() {
    for (destination, expected) in [
        ("out.txt", "out.txt"),
        ("notes/", "notes/stdin.transcript.txt"),
    ] {
        let fixture = Fixture::new();
        let output = fixture.run(
            &["-", destination, "--text=plain"],
            &fs::read(&fixture.audio).unwrap(),
        );
        assert!(output.status.success(), "{}", text(&output.stderr));
        let path = fixture.root.path().join(expected);
        assert_eq!(
            text(&output.stdout).trim(),
            fs::canonicalize(&path).unwrap().to_str().unwrap()
        );
        assert_eq!(fs::read_to_string(path).unwrap(), "Pipe works.\n");
    }
}

#[test]
fn dash_output_flag_writes_contents_instead_of_a_file() {
    let fixture = Fixture::new();
    let output = fixture.run(
        &[
            fixture.audio.to_str().unwrap(),
            "--text=plain",
            "--output",
            "-",
        ],
        &[],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "Pipe works.\n");
    assert!(!fixture.root.path().join("-").exists());
}

#[test]
fn empty_stdin_fails_with_a_pipeline_example() {
    let fixture = Fixture::new();
    let output = fixture.run(&["-", "--text=plain"], &[]);
    assert!(!output.status.success());
    let error = text(&output.stderr);
    assert!(error.contains("stdin is empty"), "{error}");
    assert!(error.contains("fscript -"), "{error}");
    assert!(!error.contains("loading model"));
    assert!(output.stdout.is_empty());
}

#[test]
fn quiet_suppresses_progress_and_worker_chatter_without_changing_transcripts() {
    for flag in ["--quiet", "-q"] {
        let fixture = Fixture::new();
        let normal = fixture.run(
            &[fixture.audio.to_str().unwrap(), "--text=plain", "--stdout"],
            &[],
        );
        assert!(normal.status.success(), "{}", text(&normal.stderr));
        assert!(text(&normal.stderr).contains("loading model"));
        fixture.script(
            "python",
            r#"printf '%s\n' 'SDK debug chatter' >&2
printf '%s\n' '{"ready":true,"device":"cpu","model_dir":"fixture","load_seconds":0.1}'
while IFS= read -r line; do
  printf '%s\n' '{"text":"Pipe works.","segments":[{"start":0,"end":1,"text":"Pipe works."}]}'
done"#,
        );
        let quiet = fixture.run(
            &[
                fixture.audio.to_str().unwrap(),
                "--text=plain",
                "--stdout",
                flag,
            ],
            &[],
        );
        assert!(quiet.status.success(), "{}", text(&quiet.stderr));
        assert_eq!(quiet.stdout, normal.stdout);
        assert!(quiet.stderr.is_empty(), "{}", text(&quiet.stderr));
    }
}

#[test]
fn quiet_file_output_still_prints_the_result_path() {
    let fixture = Fixture::new();
    let output = fixture.run(
        &[
            fixture.audio.to_str().unwrap(),
            "out.txt",
            "--text=plain",
            "--quiet",
        ],
        &[],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout).trim(),
        fs::canonicalize(fixture.root.path().join("out.txt"))
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert!(output.stderr.is_empty(), "{}", text(&output.stderr));
}

#[test]
fn quiet_preserves_warnings_and_worker_failure_details() {
    let fixture = Fixture::new();
    let output = fixture.run(
        &[
            fixture.audio.to_str().unwrap(),
            "--json",
            "--stdout",
            "--quiet",
        ],
        &[],
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stderr).contains("speaker diarization disabled"));
    assert!(!text(&output.stderr).contains("loading model"));
    fixture.script(
        "python",
        "printf '%s\\n' 'backend failed: unavailable device' >&2\nexit 1",
    );
    let output = fixture.run(
        &[fixture.audio.to_str().unwrap(), "--text=plain", "--quiet"],
        &[],
    );
    assert!(!output.status.success());
    assert!(
        text(&output.stderr).contains("backend failed: unavailable device"),
        "{}",
        text(&output.stderr)
    );
    assert!(output.stdout.is_empty());
}
