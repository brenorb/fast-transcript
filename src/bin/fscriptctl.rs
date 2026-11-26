fn main() -> std::process::ExitCode {
    fast_transcript::cli_exit(fast_transcript::run_control_from_args(
        std::env::args().skip(1).collect(),
    ))
}
