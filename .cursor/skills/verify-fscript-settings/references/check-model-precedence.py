"""Check model selection with a disposable preference and missing media."""
import os
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory


binary = Path(sys.argv[1]).resolve()
with TemporaryDirectory() as directory:
    root = Path(directory)
    config = root / "settings.json"
    config.write_text("invalid JSON")
    environment = {name: value for name, value in os.environ.items() if not name.startswith("FSCRIPT_")}
    environment["FSCRIPT_CONFIG_FILE"] = str(config)
    environment["FSCRIPT_DIARIZATION_BINARY"] = str(root / "absent-helper")
    args = [str(binary), str(root / "missing.wav"), "--text=plain", "--device", "mps"]
    base = subprocess.run(args, env=environment, capture_output=True, text=True, timeout=10)
    assert base.returncode != 0 and "invalid default-model settings" in base.stderr, base
    environment["FSCRIPT_MODEL_URL"] = "https://example.test/model.tar.gz"
    override = subprocess.run(args, env=environment, capture_output=True, text=True, timeout=10)
    assert override.returncode != 0 and "not supported by the selected model" in override.stderr, override
    assert "invalid default-model settings" not in override.stderr, override
    assert config.read_text() == "invalid JSON"
    print("The environment override bypassed malformed saved settings.")
