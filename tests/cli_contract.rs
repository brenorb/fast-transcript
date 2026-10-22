use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    let directory = tempfile::tempdir().unwrap();
    Command::new(env!("CARGO_BIN_EXE_fscript"))
        .args(args)
        .env("FSCRIPT_CONFIG_FILE", directory.path().join("missing.json"))
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn no_arguments_prints_a_successful_concise_introduction() {
    let output = run(&[]);
    assert!(output.status.success(), "{:?}", output);
    let help = stdout(&output);
    assert!(help.contains("transcrib"));
    assert!(help.contains("Usage:"));
    assert!(help.contains("fscript lecture.mp3"));
    assert!(help.contains("--help"));
    assert!(help.lines().count() <= 15);
    assert!(output.stderr.is_empty());
}

#[test]
fn help_aliases_offer_examples_documentation_and_support() {
    for args in [&["help"][..], &["--help"], &["-h"], &["--typo", "--help"]] {
        let output = run(args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        let help = stdout(&output);
        assert!(help.contains("https://github.com/brenorb/fast-transcript#readme"));
        assert!(help.contains("https://github.com/brenorb/fast-transcript/issues"));
        assert!(help.find("Examples:").unwrap() < help.find("Output:").unwrap());
        assert!(help.contains("-h, --help"));
        assert!(help.contains("-V, --version"));
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn mistyped_flags_are_suggested_without_dumping_the_manual() {
    let output = run(&["audio.wav", "--modle", "ultra"]);
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(error.contains("--model"), "{error}");
    assert!(
        error.to_lowercase().contains("similar") || error.contains("Did you mean"),
        "{error}"
    );
    assert!(error.lines().count() < 16, "{error}");
    assert!(output.stdout.is_empty());
}

#[test]
fn mistyped_model_names_suggest_the_closest_model() {
    for args in [
        &["audio.wav", "--model", "parakeet-ultr"][..],
        &["--set-default-model", "parakeet-ultr"],
    ] {
        let output = run(args);
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success());
        assert!(error.contains("Did you mean parakeet-ultra?"), "{error}");
        assert!(error.contains("--list-models"));
    }
}

#[test]
fn option_errors_are_short_and_show_a_recovery_hint() {
    let output = run(&["audio.wav", "--output", "--json"]);
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(error.contains("missing value for --output"), "{error}");
    assert!(error.contains("--help"), "{error}");
    assert!(error.lines().count() < 16, "{error}");
}

#[test]
fn informational_commands_reject_unrelated_options() {
    let output = run(&["--list-models", "--modle"]);
    assert!(!output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty());
}

#[test]
fn end_of_options_keeps_help_named_files_positional() {
    let output = run(&["--", "--help"]);
    assert!(!output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty());
}

#[test]
fn nonfinite_chunk_options_fail_before_processing_audio() {
    for (flag, value) in [
        ("--chunk", "NaN"),
        ("--overlap", "inf"),
        ("--threshold", "NaN"),
    ] {
        let output = run(&["audio.wav", flag, value]);
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success());
        assert!(error.contains("must be finite"), "{error}");
    }
}

#[test]
fn diarization_help_prefers_long_flags_and_explains_legacy_alias() {
    let output = run(&["--help"]);
    let help = stdout(&output);
    assert!(help.contains("  --diarize [coreml|lseend-dihard3]"));
    assert!(help.contains("Legacy alias: -d."));
    assert!(!help.contains("-d, --diarize"));
}

#[test]
fn quiet_settings_commands_preserve_data_and_suppress_confirmations() {
    for args in [
        &["--quiet", "--set-default-model", "ultra"][..],
        &["--reset-default-model", "-q"],
    ] {
        let output = run(args);
        assert!(output.status.success(), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
    let output = run(&["--get-default-model", "--quiet"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(stdout(&output).trim(), "parakeet-v3-int8");
    assert!(output.stderr.is_empty());
}

#[test]
fn settings_confirmations_are_status_messages_on_stderr() {
    let output = run(&["--set-default-model", "ultra"]);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Default model: parakeet-ultra"));
}

#[test]
fn offline_manual_explains_pipelines_configuration_and_error_recovery() {
    for args in [&["help", "manual"][..], &["--manual"]] {
        let output = run(args);
        assert!(output.status.success(), "{output:?}");
        let manual = stdout(&output);
        for topic in [
            "cat lecture.wav | fscript -",
            "FSCRIPT_CONFIG_FILE",
            "FSCRIPT_DIARIZATION_BINARY",
            "--quiet",
            "--list-models",
            "--model",
            "--stdout",
            "--reset-default-model",
            "Troubleshooting",
        ] {
            assert!(manual.contains(topic), "manual must cover {topic}");
        }
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn man_page_is_exportable_without_terminal_escapes() {
    let output = run(&["--man-page"]);
    assert!(output.status.success(), "{output:?}");
    let man = stdout(&output);
    assert!(man.starts_with(".TH FSCRIPT 1"));
    assert!(man.contains(".SH SYNOPSIS"));
    assert!(man.contains(".SH OPTIONS"));
    assert!(man.contains("FSCRIPT_CONFIG_FILE"));
    assert!(!man.contains('\u{1b}'));
    assert!(output.stderr.is_empty());
    let help = stdout(&run(&["--help"]));
    assert!(help.contains("--manual"));
    assert!(help.contains("--man-page"));
}

#[test]
fn both_offline_formats_cover_every_documented_long_option() {
    let help = stdout(&run(&["--help"]));
    let manual = stdout(&run(&["--manual"]));
    let man = stdout(&run(&["--man-page"])).replace("\\-", "-");
    for token in help
        .split_whitespace()
        .filter(|token| token.starts_with("--"))
    {
        let option = token
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .next()
            .unwrap();
        assert!(
            manual.contains(option),
            "offline manual is missing {option}"
        );
        assert!(man.contains(option), "man page is missing {option}");
    }
}
