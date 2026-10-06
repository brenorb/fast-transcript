"""Persistent CLI model preferences, isolated from the user's real settings."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/release/fscript"


class DefaultModelCliTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="fscript-default-model-")
        self.addCleanup(self.temporary.cleanup)
        self.settings = Path(self.temporary.name) / "settings.json"
        self.env = dict(os.environ, FSCRIPT_CONFIG_FILE=str(self.settings))
        self.env.pop("FSCRIPT_MODEL_URL", None)

    def run_cli(self, *args):
        return subprocess.run([str(BINARY), *args], env=self.env, text=True,
                              capture_output=True, timeout=20)

    def test_default_persists_between_processes_and_can_be_reset(self):
        self.assertEqual(self.run_cli("--get-default-model").stdout.strip(), "parakeet-v3-int8")
        saved = self.run_cli("--set-default-model", "ultra")
        self.assertEqual(saved.returncode, 0, saved.stderr)
        self.assertEqual(json.loads(self.settings.read_text())["model"], "parakeet-ultra")
        self.assertEqual(self.run_cli("--get-default-model").stdout.strip(), "parakeet-ultra")
        reset = self.run_cli("--reset-default-model")
        self.assertEqual(reset.returncode, 0, reset.stderr)
        self.assertEqual(self.run_cli("--get-default-model").stdout.strip(), "parakeet-v3-int8")
        self.assertFalse(self.settings.exists())

    def test_invalid_or_mixed_commands_do_not_change_saved_default(self):
        self.assertEqual(self.run_cli("--set-default-model=ultra").returncode, 0)
        before = self.settings.read_bytes()
        for args in [("--set-default-model",), ("--set-default-model", "unknown"),
                     ("--set-default-model", "onnx", "audio.wav"),
                     ("audio.wav", "--get-default-model"),
                     ("--reset-default-model", "--json")]:
            with self.subTest(args=args):
                result = self.run_cli(*args)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.settings.read_bytes(), before)

    def test_explicit_model_overrides_environment_and_saved_preference(self):
        self.assertEqual(self.run_cli("--set-default-model", "ultra").returncode, 0)
        self.env["FSCRIPT_MODEL_URL"] = "https://example.test/model.tar.gz"
        missing_audio = str(Path(self.temporary.name) / "missing.wav")
        environment = self.run_cli(missing_audio, "--text=plain", "--device", "mps")
        self.assertNotEqual(environment.returncode, 0)
        self.assertIn("not supported by the selected model", environment.stderr)
        explicit = self.run_cli(missing_audio, "--text=plain", "--device", "mps", "--model", "ultra")
        self.assertNotEqual(explicit.returncode, 0)
        self.assertNotIn("not supported by the selected model", explicit.stderr)
        self.assertEqual(self.run_cli("--get-default-model").stdout.strip(), "parakeet-ultra")

    def test_corrupted_settings_can_be_overridden_or_reset(self):
        self.settings.write_text("broken json")
        missing_audio = str(Path(self.temporary.name) / "missing.wav")
        default = self.run_cli(missing_audio, "--text=plain")
        self.assertNotEqual(default.returncode, 0)
        self.assertIn("invalid default-model settings", default.stderr)
        explicit = self.run_cli(missing_audio, "--text=plain", "--model", "onnx")
        self.assertNotEqual(explicit.returncode, 0)
        self.assertNotIn("invalid default-model settings", explicit.stderr)
        self.assertEqual(self.run_cli("--help").returncode, 0)
        self.assertEqual(self.run_cli("--reset-default-model").returncode, 0)


if __name__ == "__main__":
    unittest.main()
