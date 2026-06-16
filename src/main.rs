use std::env;
fn main() -> anyhow::Result<()> {
    fast_transcript::run_from_args(env::args().skip(1).collect())
}
