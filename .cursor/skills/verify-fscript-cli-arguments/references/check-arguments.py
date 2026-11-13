import subprocess
import sys
import tempfile
from pathlib import Path


binary = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/fscript").resolve()
cases = [
    (["audio.wav", "--json", "--srt"], "output format flags are mutually exclusive"),
    (["audio.wav", "result.txt", "--stdout"], "cannot use both --stdout and an explicit output path"),
    (["audio.wav", "--text", "invalid"], "invalid --text value"),
    (["--", "-h"], None),
]
with tempfile.TemporaryDirectory() as directory:
    for args, expected in cases:
        result = subprocess.run([binary, *args], cwd=directory, capture_output=True, text=True)
        assert result.returncode != 0, args
        if expected is None:
            assert "Usage:" not in result.stdout
            assert "-h" in result.stderr, result.stderr
        else:
            assert expected in result.stderr, (args, result.stderr)
print("CLI dispatch and option conflict checks passed")
