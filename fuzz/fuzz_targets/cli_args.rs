#![no_main]

use libfuzzer_sys::fuzz_target;

const MAX_ARGS: usize = 16;
const MAX_ARG_BYTES: usize = 64;

fn bytes_to_args(data: &[u8]) -> Vec<String> {
    let chunks = if data.contains(&0) {
        data.split(|byte| *byte == 0).collect::<Vec<_>>()
    } else {
        data.split(|byte| *byte == b'\n').collect::<Vec<_>>()
    };

    let mut args = chunks
        .into_iter()
        .take(MAX_ARGS)
        .map(|chunk| {
            let truncated = &chunk[..chunk.len().min(MAX_ARG_BYTES)];
            String::from_utf8_lossy(truncated).trim().to_string()
        })
        .filter(|arg| !arg.is_empty())
        .collect::<Vec<_>>();

    if args.is_empty() || args[0].is_empty() {
        args.insert(0, "audio.wav".to_string());
    }

    args
}

fuzz_target!(|data: &[u8]| {
    let args = bytes_to_args(data);
    fast_transcript::fuzz_parse_args(&args);
});
