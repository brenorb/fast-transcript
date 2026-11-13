"""Verify that the benchmark refuses to reuse a run after a manifest changes."""
import json
import sys
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
from unittest.mock import patch

sys.path.insert(0, str(Path.cwd() / "scripts"))
import benchmark_asr as module


with TemporaryDirectory() as temporary:
    root = Path(temporary).resolve()
    fixture = root / "fixture"
    fixture.mkdir()
    out = root / "output"
    out.mkdir()
    manifest = fixture / "one.json"
    manifest.write_text(json.dumps({
        "case": "one",
        "reference_words": 1,
        "utterances": [{"id": "sample", "text": "fixture", "parts": []}],
    }))
    suite = fixture / "suite.json"
    suite.write_text(json.dumps({"protocol": module.PROTOCOL, "manifests": [manifest.name]}))
    worker = fixture / "onnx-worker"
    worker.touch()
    saved = out / "one-onnx-cpu-r1.json"
    saved.write_text("{}")
    (out / "environment.json").write_text(json.dumps({"input_sha256": {"fixture/one.json": "stale"}}))
    original_relative_to = Path.relative_to

    def rebase(path, parent, *args, **kwargs):
        if parent == module.REPO and path.is_relative_to(fixture):
            return original_relative_to(path, root)
        return original_relative_to(path, parent, *args, **kwargs)

    args = SimpleNamespace(
        suite=suite, cases=None, out_dir=out, resume=True, engines=["onnx"],
        onnx_worker=worker, onnx_model=fixture, repetitions=1,
    )
    with patch.object(Path, "relative_to", rebase), patch.object(module, "summarize") as summarize:
        try:
            module.benchmark(args)
        except AssertionError as error:
            assert "Inputs changed" in str(error), error
        else:
            raise AssertionError("Reused measurements after an input change")
        summarize.assert_not_called()
    assert saved.read_text() == "{}"
print("The benchmark rejected mismatched metadata before reusing a saved result.")
