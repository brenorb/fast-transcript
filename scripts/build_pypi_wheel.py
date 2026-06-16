#!/usr/bin/env python3
import argparse
import os
import shutil
import stat
import subprocess
import sys
import tempfile
from pathlib import Path
import re


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BINARY = REPO_ROOT / "target" / "release" / ("fscript.exe" if os.name == "nt" else "fscript")
STAGE_FILES = ["pyproject.toml", "setup.py", "Cargo.toml", "README.md", "LICENSE"]
STAGE_DIRS = ["python"]


def cargo_version() -> str:
    cargo_toml = (REPO_ROOT / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^version\s*=\s*"([^"]+)"', cargo_toml, re.MULTILINE)
    if match is None:
        raise SystemExit("could not read version from Cargo.toml")
    return match.group(1)


def binary_version(binary_path: Path) -> str:
    completed = subprocess.run(
        [str(binary_path), "--version"],
        check=True,
        text=True,
        capture_output=True,
    )
    version = completed.stdout.strip()
    prefix = "fscript "
    if not version.startswith(prefix):
        raise SystemExit(f"unexpected --version output from {binary_path}: {version!r}")
    return version[len(prefix) :]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, default=DEFAULT_BINARY)
    parser.add_argument("--out-dir", type=Path, default=REPO_ROOT / "dist-pypi")
    return parser.parse_args()


def copy_stage_tree(stage_root: Path, binary_path: Path) -> None:
    for file_name in STAGE_FILES:
        shutil.copy2(REPO_ROOT / file_name, stage_root / file_name)

    for dir_name in STAGE_DIRS:
        shutil.copytree(REPO_ROOT / dir_name, stage_root / dir_name)

    bundled_binary = stage_root / "python" / "fscript" / "bin" / binary_path.name
    bundled_binary.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(binary_path, bundled_binary)
    bundled_binary.chmod(bundled_binary.stat().st_mode | stat.S_IXUSR)


def main() -> int:
    args = parse_args()
    binary_path = args.binary.resolve()
    out_dir = args.out_dir.resolve()

    if not binary_path.exists():
        raise SystemExit(f"missing built binary: {binary_path}")

    expected_version = cargo_version()
    actual_version = binary_version(binary_path)
    if actual_version != expected_version:
        raise SystemExit(
            "built binary version does not match Cargo.toml: "
            f"{actual_version} != {expected_version}. Rebuild the release binary first."
        )

    out_dir.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="fscript-pypi-") as temp_dir:
        stage_root = Path(temp_dir) / "stage"
        stage_root.mkdir()
        copy_stage_tree(stage_root, binary_path)
        subprocess.run(
            [sys.executable, "-m", "build", "--wheel", "--outdir", str(out_dir)],
            cwd=stage_root,
            check=True,
        )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
