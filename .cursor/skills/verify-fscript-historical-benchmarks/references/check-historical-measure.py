"""Exercise the historical runner's output gates and one recorded measurement."""
import sys
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
from unittest.mock import patch

sys.path.insert(0, str(Path.cwd() / "scripts"))
import benchmark_transcription_engines as engines

with TemporaryDirectory() as temporary:
    root = Path(temporary)
    args = SimpleNamespace(out_dir=root, repetitions=1)
    for backend, truncated in (("cpu", False), ("mlx", True), ("mlx", False)):
        payload = {"engine": backend, "truncated": truncated, "text": "one three"}
        with patch.object(engines, "run", return_value=(payload, {"wall_clock_s": 1})):
            try:
                result = engines.measure(
                    ["unused"], "test", 1, "one two", "fixture", "phonon-2", "mlx", 1, args,
                )
            except RuntimeError as error:
                assert (backend, truncated) != ("mlx", False)
                assert "Unexpected Phonon backend or truncated output" in str(error)
                assert not list(root.iterdir())
            else:
                assert (backend, truncated) == ("mlx", False), "An invalid worker output was saved"
                assert result["wer_vs_reference"] == 0.5
                assert (root / "test_phonon-2_mlx.json").exists()
print("The historical runner rejected wrong-backend and truncated output before writing.")
