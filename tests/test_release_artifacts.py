import stat
import subprocess
import tempfile
import unittest
import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RELEASE_BINARY = ROOT / "target" / "release" / "fscript"
DIST_DIR = ROOT / "dist-pypi"
PACKAGE_VERSION = next(
    line.split('"')[1]
    for line in (ROOT / "Cargo.toml").read_text(encoding="utf-8").splitlines()
    if line.startswith("version = ")
)


def run_cli(binary: Path, *args: str) -> str:
    completed = subprocess.run(
        [str(binary), *args],
        cwd=ROOT,
        check=True,
        text=True,
        capture_output=True,
    )
    return completed.stdout


class ReleaseArtifactTests(unittest.TestCase):
    def test_release_binary_reports_package_version(self) -> None:
        self.assertTrue(RELEASE_BINARY.exists(), f"missing release binary: {RELEASE_BINARY}")
        self.assertEqual(
            run_cli(RELEASE_BINARY, "--version").strip(),
            f"fscript {PACKAGE_VERSION}",
        )

    def test_wheel_binary_matches_release_binary(self) -> None:
        if not DIST_DIR.exists():
            self.skipTest(
                f"wheel artifact check requires a built wheel directory at {DIST_DIR}"
            )
        wheels = sorted(DIST_DIR.glob(f"fscript-{PACKAGE_VERSION}-*.whl"))
        if not wheels:
            self.skipTest(
                "wheel artifact check requires a built wheel; run "
                "`python scripts/build_pypi_wheel.py --out-dir dist-pypi` first"
            )
        self.assertEqual(
            len(wheels),
            1,
            f"expected exactly one wheel for {PACKAGE_VERSION}, found {wheels}",
        )
        wheel = wheels[0]

        release_version = run_cli(RELEASE_BINARY, "--version")
        release_help = run_cli(RELEASE_BINARY, "--help")

        with tempfile.TemporaryDirectory(prefix="fscript-wheel-check-") as tmpdir:
            extracted = Path(tmpdir) / "fscript"
            with zipfile.ZipFile(wheel) as archive:
                binary_members = [
                    name for name in archive.namelist() if name.endswith("/bin/fscript")
                ]
                self.assertEqual(
                    len(binary_members),
                    1,
                    f"expected exactly one bundled binary in {wheel.name}, found {binary_members}",
                )
                extracted.write_bytes(archive.read(binary_members[0]))

            extracted.chmod(extracted.stat().st_mode | stat.S_IXUSR)

            self.assertEqual(run_cli(extracted, "--version"), release_version)
            self.assertEqual(run_cli(extracted, "--help"), release_help)


if __name__ == "__main__":
    unittest.main()
