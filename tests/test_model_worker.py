"""Exercise the embedded worker protocol without model downloads or SDK installs."""

import json
import subprocess
import sys
import unittest
from pathlib import Path


WORKER = Path(__file__).resolve().parents[1] / "src" / "python_worker.py"
ULTRA_REVISION = "510e6f5a1c4619f39c72b083c091476935734e65"
PHONON_SHA256 = "98125795b6dda72f5c6eee9ba33d19815df65dcb18b50a357bf9f73c9935309e"

BOOTSTRAP = r'''
import importlib.metadata
import os
import runpy
import sys
import types

versions = {"moondream": "2.4.1", "kestrel": "0.8.1", "torch": "2.14.0", "fermion-research": "0.2.3"}
importlib.metadata.version = lambda name: versions[name]

def module(name, **values):
    value = types.ModuleType(name)
    value.__dict__.update(values)
    sys.modules[name] = value
    return value

mode = sys.argv[2]
class Model:
    def __init__(self):
        print("SDK load diagnostic")
        self.calls = 0

    def transcribe(self, *, audio, timestamps):
        assert timestamps == "segment"
        self.calls += 1
        print("SDK inference diagnostic")
        return {"text": f"call {self.calls}", "segments": [{"start": 0.0, "end": 1.0, "text": f"call {self.calls}"}]}

    def transcribe_detailed(self, audio):
        return types.SimpleNamespace(text="speech", segments=[{"start": 0.0, "end": 1.0, "text": "speech"}], truncated=mode == "truncated")

def photon(repo, *, device, model_path):
    assert repo == "moondream/parakeet-ultra"
    assert device == "mps"
    assert model_path == "/cached/pinned-ultra"
    if mode == "load-error":
        raise RuntimeError("could not load the selected model")
    return Model()

def snapshot_download(*, repo_id, revision, allow_patterns):
    assert repo_id == "moondream/parakeet-ultra"
    assert revision == "510e6f5a1c4619f39c72b083c091476935734e65"
    assert "model.safetensors" in allow_patterns
    print("SDK download diagnostic")
    return "/cached/pinned-ultra"

module("moondream", photon=photon)
module("huggingface_hub", snapshot_download=snapshot_download)
module("torch", cuda=types.SimpleNamespace(is_available=lambda: False),
       backends=types.SimpleNamespace(mps=types.SimpleNamespace(is_available=lambda: True)),
       mps=types.SimpleNamespace(synchronize=lambda: None))

pin = {"sha256": "bad-pin" if mode == "wrong-pin" else "98125795b6dda72f5c6eee9ba33d19815df65dcb18b50a357bf9f73c9935309e", "backend": "phonon2-five-value"}
def resolve_device(what):
    # An explicit CLI device replaces the inherited SDK device environment.
    assert os.environ["FERMION_DEVICE"] == "cpu"
    return "cpu"
backends = types.SimpleNamespace(resolve=resolve_device, require_engine_for=lambda *args: None,
                                load=lambda *args, **kwargs: Model())
fetch = types.SimpleNamespace(ensure=lambda *args: "/cached/phonon")
module("fermion")
module("fermion._speech", backends=backends, fetch=fetch)
module("fermion.transcribe", _resolve=lambda repo: (repo, "five-value", pin, None))

worker, _, repository, revision, device, local_directory = sys.argv[1:]
sys.argv = [worker, repository, revision, device, local_directory]
runpy.run_path(worker, run_name="__main__")
'''


class ModelWorkerTests(unittest.TestCase):
    def run_worker(self, mode="normal", *, phonon=False):
        repository = "FermionResearch/Phonon-2" if phonon else "moondream/parakeet-ultra"
        revision = PHONON_SHA256 if phonon else ULTRA_REVISION
        return subprocess.run(
            [sys.executable, "-c", BOOTSTRAP, str(WORKER), mode, repository,
             revision, "cpu" if phonon else "auto", ""],
            input='{"audio_path":"first.wav"}\n{"audio_path":"second.wav"}\n',
            text=True, capture_output=True, timeout=20,
        )

    def test_photon_loads_pinned_weights_once_and_preserves_stdout_protocol(self):
        result = self.run_worker()
        self.assertEqual(result.returncode, 0, result.stderr)
        messages = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual(len(messages), 3)
        self.assertTrue(messages[0]["ready"])
        self.assertEqual(messages[0]["device"], "mps")
        self.assertEqual([message["text"] for message in messages[1:]], ["call 1", "call 2"])
        self.assertEqual(messages[1]["segments"][0]["end"], 1.0)
        self.assertEqual(result.stderr.count("SDK load diagnostic"), 1)
        self.assertNotIn("diagnostic", result.stdout)

    def test_startup_failure_returns_error_before_any_transcript(self):
        result = self.run_worker("load-error")
        self.assertNotEqual(result.returncode, 0)
        messages = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual(messages, [{"error": "could not load the selected model"}])

    def test_phonon_truncation_is_an_error_instead_of_successful_partial_output(self):
        result = self.run_worker("truncated", phonon=True)
        messages = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertTrue(messages[0]["ready"])
        self.assertEqual(messages[0]["device"], "cpu")
        self.assertIn("truncated", messages[1]["error"])
        self.assertNotIn("text", messages[1])

    def test_phonon_refuses_a_changed_artifact_pin(self):
        result = self.run_worker("wrong-pin", phonon=True)
        self.assertNotEqual(result.returncode, 0)
        messages = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertIn("benchmark pin", messages[0]["error"])


if __name__ == "__main__":
    unittest.main()
