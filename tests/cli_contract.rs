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
