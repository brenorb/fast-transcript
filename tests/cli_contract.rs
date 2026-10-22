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
