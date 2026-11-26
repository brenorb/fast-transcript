import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "python"))

from fscript._cli import bundled_binary_name, control_main, main  # noqa: E402


class PyPiWrapperTests(unittest.TestCase):
    def test_bundled_binary_name(self) -> None:
        self.assertIn(bundled_binary_name(), {"fscript", "fscript.exe"})

    def test_main_executes_bundled_binary(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            fake_binary = Path(tmpdir) / "fscript"
            fake_binary.write_text("", encoding="utf-8")

            with (
                mock.patch("fscript._cli.bundled_binary_path", return_value=fake_binary),
                mock.patch("fscript._cli.ensure_executable") as ensure_exec,
                mock.patch("subprocess.run", return_value=subprocess.CompletedProcess(["fscript"], 7)) as run,
                mock.patch.object(sys, "argv", ["fscript", "--help"]),
            ):
                rc = main()

            self.assertEqual(rc, 7)
            ensure_exec.assert_called_once_with(fake_binary)
            run.assert_called_once_with([str(fake_binary), "--help"], check=False)

    def test_main_fails_cleanly_when_binary_is_missing(self) -> None:
        missing = Path("/definitely/missing/fscript")
        with (
            mock.patch("fscript._cli.bundled_binary_path", return_value=missing),
            mock.patch.object(sys, "argv", ["fscript"]),
        ):
            rc = main()
        self.assertEqual(rc, 1)

    def test_control_entrypoint_preserves_argv_and_status(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            binary = Path(tmpdir) / bundled_binary_name("fscriptctl")
            binary.touch()
            with (
                mock.patch("fscript._cli.bundled_control_binary_path", return_value=binary),
                mock.patch("fscript._cli.ensure_executable") as ensure_exec,
                mock.patch("subprocess.run", return_value=subprocess.CompletedProcess([], 130)) as run,
                mock.patch.object(sys, "argv", ["fscriptctl", "notes", "file with spaces.txt", "--json"]),
            ):
                self.assertEqual(control_main(), 130)
            ensure_exec.assert_called_once_with(binary)
            run.assert_called_once_with([str(binary), "notes", "file with spaces.txt", "--json"], check=False)


if __name__ == "__main__":
    unittest.main()
