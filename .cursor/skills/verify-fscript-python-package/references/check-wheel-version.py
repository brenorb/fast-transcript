"""Check wheel staging is rejected for a local wrong-version binary."""
from pathlib import Path
from runpy import run_path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
from unittest.mock import patch


def unexpected_staging(*_args):
    raise AssertionError("staged wrong-version binary")


wheel = run_path("scripts/build_pypi_wheel.py")
main = wheel["main"]
with TemporaryDirectory() as directory:
    root = Path(directory)
    binary = root / "fscript"
    binary.write_text("#!/bin/sh\nprintf '%s\\n' 'fscript 0.0.0'\n")
    binary.chmod(0o700)
    destination = root / "dist"
    with (
        patch.dict(main.__globals__, {"parse_args": lambda: SimpleNamespace(binary=binary, out_dir=destination)}),
        patch.dict(main.__globals__, {"copy_stage_tree": unexpected_staging}),
    ):
        try:
            main()
        except SystemExit as error:
            assert "does not match Cargo.toml" in str(error), error
        else:
            raise AssertionError("version mismatch was accepted")
    assert not destination.exists()
print("The wrong-version binary was rejected before staging.")
