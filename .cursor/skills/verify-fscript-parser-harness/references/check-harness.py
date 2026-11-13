import subprocess
import sys
import tempfile
from pathlib import Path


source = Path(sys.argv[1] if len(sys.argv) > 1 else "fuzz/fuzz_targets/cli_args.rs").read_text()
adapter = source[source.index("const MAX_ARGS"):source.index("fuzz_target!(")]
checks = r"""
fn main() {
    assert_eq!(bytes_to_args(b"\n \n"), vec!["audio.wav"]);
    assert_eq!(bytes_to_args(b"one\ntwo"), vec!["one", "two"]);
    assert_eq!(bytes_to_args(b"one\ntwo\0three"), vec!["one\ntwo", "three"]);
    assert_eq!(bytes_to_args("x".repeat(65).as_bytes())[0].len(), 64);
    assert_eq!(bytes_to_args(vec!["file"; 17].join("\n").as_bytes()).len(), 16);
    assert_eq!(bytes_to_args(vec![""; 16].join("\n").as_bytes()), vec!["audio.wav"]);
}
"""
with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    script = root / "adapter.rs"
    executable = root / "adapter"
    script.write_text(adapter + checks)
    subprocess.run(["rustc", str(script), "-o", str(executable)], check=True)
    subprocess.run([str(executable)], check=True)
print("Parser harness input normalization passed")
