"""Exercise the wrapper with the actual stage tree of a renamed local binary."""
import os
import subprocess
import sys
from pathlib import Path
from runpy import run_path
from tempfile import TemporaryDirectory
from unittest.mock import patch


wheel = run_path("scripts/build_pypi_wheel.py")
version = wheel["cargo_version"]()
run = subprocess.run


def inspect_build(command, **kwargs):
    if command[1:3] != ["-m", "build"]:
        return run(command, **kwargs)
    package_root = kwargs["cwd"] / "python"
    binary_root = package_root / "fscript" / "bin"
    expected_name = "fscript.exe" if os.name == "nt" else "fscript"
    assert (binary_root / "renamed-fscript").is_file()
    assert not (binary_root / expected_name).exists()
    wrapper = run(
        [sys.executable, "-m", "fscript", "--version"],
        cwd=kwargs["cwd"],
        env={**os.environ, "PYTHONPATH": str(package_root)},
        capture_output=True,
        text=True,
    )
    assert wrapper.returncode == 1, wrapper
    assert "missing its bundled binary" in wrapper.stderr, wrapper.stderr
    return subprocess.CompletedProcess(command, 0)


with TemporaryDirectory() as directory:
    root = Path(directory)
    binary = root / "renamed-fscript"
    binary.write_text(f"#!/bin/sh\nprintf '%s\\n' 'fscript {version}'\n")
    binary.chmod(0o700)
    with (
        patch.object(sys, "argv", ["build_pypi_wheel.py", "--binary", str(binary), "--out-dir", str(root / "dist")]),
        patch.object(subprocess, "run", side_effect=inspect_build) as invocations,
    ):
        assert wheel["main"]() == 0
        assert invocations.call_count == 2, invocations.call_args_list
print("The build accepted the renamed binary and the staged wrapper could not find it.")
