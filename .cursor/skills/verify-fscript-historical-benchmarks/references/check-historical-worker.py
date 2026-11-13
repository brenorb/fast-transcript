"""Run the resident Python worker with a local fake model and verify its measured passes."""
import contextlib
import io
import json
import sys
from pathlib import Path
from tempfile import TemporaryDirectory
from types import ModuleType, SimpleNamespace
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path.cwd() / "scripts"))
import benchmark_english_transcription as historical

calls = []


def transcribe(*, audio, timestamps):
    assert timestamps == "segment"
    calls.append(audio.name)
    return {"text": audio.stem}


moondream = ModuleType("moondream")
model = SimpleNamespace(transcribe=transcribe)
context = Mock()
context.__enter__ = Mock(return_value=model)
context.__exit__ = Mock(return_value=False)
moondream.photon = Mock(return_value=context)
torch = ModuleType("torch")
weights = ModuleType("kestrel.models.parakeet_tdt.weights")
weights._PINNED_REVISIONS = {"moondream/parakeet-redux": "fixture"}
modules = {item.__name__: item for item in (moondream, torch, weights)}

with TemporaryDirectory() as temporary:
    manifest = Path(temporary) / "manifest.json"
    manifest.write_text(json.dumps({"utterances": [
        {"id": "one", "audio_path": "one.wav"},
        {"id": "two", "audio_path": "two.wav"},
    ]}))
    args = SimpleNamespace(worker="parakeet-redux", device="cpu", manifest=manifest, repetitions=2)
    stdout = io.StringIO()
    with patch.dict(sys.modules, modules), patch.object(historical, "version", return_value="fixture"):
        with contextlib.redirect_stdout(stdout):
            historical.worker(args)
    report = json.loads(stdout.getvalue())
    assert calls == ["one.wav", "two.wav"] * 3
    assert [(row["repetition"], row["id"]) for row in report["runs"]] == [
        (1, "one"), (1, "two"), (2, "one"), (2, "two"),
    ]
    moondream.photon.assert_called_once()
    context.__exit__.assert_called_once()
print("One resident model completed three passes and reported only the two measured passes.")
