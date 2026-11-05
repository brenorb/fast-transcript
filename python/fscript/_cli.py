import os
import stat
import subprocess
import sys
from importlib.resources import files
from pathlib import Path


def bundled_binary_name() -> str:
    return "fscript.exe" if os.name == "nt" else "fscript"


def bundled_binary_path() -> Path:
    return Path(files("fscript").joinpath("bin", bundled_binary_name()))


def ensure_executable(path: Path) -> None:
    mode = path.stat().st_mode
    if mode & stat.S_IXUSR:
        return
    path.chmod(mode | stat.S_IXUSR)


def main() -> int:
    binary = bundled_binary_path()
    if not binary.exists():
        print(
            "error: this fscript installation is incomplete. Reinstall it with "
            "`uv tool install --reinstall fscript` or `pip install --force-reinstall fscript`",
            file=sys.stderr,
        )
        return 1

    ensure_executable(binary)
    completed = subprocess.run([str(binary), *sys.argv[1:]], check=False)
    return completed.returncode
