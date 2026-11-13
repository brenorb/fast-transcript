"""Exercise the validator's protocol gate with a local fixture."""
import json
import subprocess
import sys
from pathlib import Path
from tempfile import TemporaryDirectory

with TemporaryDirectory() as temporary:
    root = Path(temporary)
    suite = root / "suite.json"
    suite.write_text(json.dumps({"protocol": "wrong", "manifests": []}))
    (root / "environment.json").write_text(json.dumps({"input_sha256": {}, "repetitions": 1}))
    (root / "summary.json").write_text(json.dumps({"protocol": "asr-v2", "results": []}))
    result = subprocess.run(
        [sys.executable, "scripts/validate_asr_benchmark.py", "--suite", str(suite), "--runs-dir", str(root)],
        text=True, capture_output=True,
    )
    assert result.returncode != 0, result
    assert 'assert suite["protocol"] == "asr-v2"' in result.stderr, result.stderr
    assert not (root / "validation.json").exists()
    suite.write_text(json.dumps({"protocol": "asr-v2", "manifests": []}))
    result = subprocess.run(result.args, text=True, capture_output=True)
    assert result.returncode == 0, result.stderr
    report = json.loads((root / "validation.json").read_text())
    assert report["process_repetitions_checked"] == 0
    assert report["reference_counts"] == {}
print("The validator rejected a protocol mismatch but accepted an empty suite with the expected protocol.")
