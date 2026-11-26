import os
import stat
import subprocess
import sys
from importlib.resources import files
from pathlib import Path


def bundled_binary_name(name: str = "fscript") -> str:
    return name + ".exe" if os.name == "nt" else name


def bundled_binary_path() -> Path:
    return Path(files("fscript").joinpath("bin", bundled_binary_name()))


def bundled_control_binary_path() -> Path:
    return Path(files("fscript").joinpath("bin", bundled_binary_name("fscriptctl")))


def ensure_executable(path: Path) -> None:
    mode = path.stat().st_mode
    if mode & stat.S_IXUSR:
        return
    path.chmod(mode | stat.S_IXUSR)


def run_binary(binary: Path) -> int:
    if not binary.exists():
        print(
            f"fscript wheel is missing its bundled binary: {binary}",
            file=sys.stderr,
        )
        return 1

    ensure_executable(binary)
    completed = subprocess.run([str(binary), *sys.argv[1:]], check=False)
    return completed.returncode


def main() -> int:
    return run_binary(bundled_binary_path())


def control_main() -> int:
    return run_binary(bundled_control_binary_path())
