"""Persistent CLI model preferences, isolated from the user's real settings."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get("FSCRIPT_TEST_BINARY", ROOT / "target/release/fscript"))
CONTROL = Path(os.environ.get("FSCRIPT_TEST_BINARY", ROOT / "target/release/fscript"))


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

    def run_control(self, *args):
        return subprocess.run([str(CONTROL), "--quiet", "models", *args], env=self.env,
                              text=True, capture_output=True, timeout=20)

    def control_json(self, *args):
        result = self.run_control(*args, "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")
        return json.loads(result.stdout)

    def test_control_catalog_has_no_setup_side_effect_and_reports_precedence(self):
        listed = self.control_json()
        self.assertEqual(listed["version"], 1)
        self.assertEqual(listed["effective_model"], "parakeet-v3-int8")
        self.assertIsNone(listed["saved_default"])
        self.assertIsNone(listed["environment_override"])
        self.assertFalse(self.settings.exists())
        human = self.run_cli("--list-models").stdout
        for model in listed["models"]:
            self.assertIn(model["name"], human)
            self.assertIn(model["advice"], human)
        self.assertEqual(len(listed["models"]), 4)
        self.run_cli("--set-default-model", "ultra")
        self.env["FSCRIPT_MODEL_URL"] = "https://example.test/override.tar.gz"
        current = self.control_json("get")
        self.assertEqual(current["saved_default"], "parakeet-ultra")
        self.assertEqual(current["environment_override"], self.env["FSCRIPT_MODEL_URL"])
        self.assertEqual(current["effective_model"], self.env["FSCRIPT_MODEL_URL"])
        self.assertEqual(self.run_control("get").stdout.strip(), current["effective_model"])

    def test_control_and_compatibility_flags_have_identical_saved_state_for_all_aliases(self):
        for alias, canonical in [("default", "parakeet-v3-int8"), ("onnx", "parakeet-v3-int8"),
                                 ("redux", "parakeet-redux"), ("ultra", "parakeet-ultra"),
                                 ("phonon", "phonon-2"), *[(name, name) for name in
                                  ("parakeet-v3-int8", "parakeet-redux", "parakeet-ultra", "phonon-2")]]:
            with self.subTest(alias=alias):
                state = self.control_json("set", alias)
                control_bytes = self.settings.read_bytes()
                self.assertEqual(state["saved_default"], canonical)
                self.assertEqual(self.run_cli("--get-default-model").stdout.strip(), canonical)
                self.assertEqual(self.run_cli("--set-default-model", alias).returncode, 0)
                self.assertEqual(self.settings.read_bytes(), control_bytes)
        self.assertEqual(self.run_cli("--reset-default-model").returncode, 0)
        self.assertIsNone(self.control_json("get")["saved_default"])

    def test_control_invalid_updates_preserve_state_and_reset_recovers_corruption(self):
        self.control_json("set", "ultra")
        before = self.settings.read_bytes()
        invalid = self.run_control("set", "ultr")
        self.assertNotEqual(invalid.returncode, 0)
        self.assertIn("Did you mean ultra", invalid.stderr)
        self.assertEqual(self.settings.read_bytes(), before)
        self.settings.write_text("broken")
        self.assertNotEqual(self.run_control("get").returncode, 0)
        reset = self.control_json("reset")
        self.assertIsNone(reset["saved_default"])
        self.assertFalse(self.settings.exists())

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
