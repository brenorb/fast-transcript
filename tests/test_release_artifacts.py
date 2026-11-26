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
        for name in ("fscript", "fscriptctl"):
            with self.subTest(name=name):
                binary = RELEASE_BINARY.with_name(name)
                self.assertTrue(binary.exists(), f"missing release binary: {binary}")
                self.assertEqual(run_cli(binary, "--version").strip(), f"{name} {PACKAGE_VERSION}")

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

        with tempfile.TemporaryDirectory(prefix="fscript-wheel-check-") as tmpdir:
            with zipfile.ZipFile(wheel) as archive:
                entrypoints = next(name for name in archive.namelist() if name.endswith(".dist-info/entry_points.txt"))
                entrypoints = archive.read(entrypoints).decode("utf-8")
                self.assertIn("fscript = fscript._cli:main", entrypoints)
                self.assertIn("fscriptctl = fscript._cli:control_main", entrypoints)
                for name in ("fscript", "fscriptctl"):
                    with self.subTest(name=name):
                        members = [member for member in archive.namelist() if member.endswith(f"/bin/{name}")]
                        self.assertEqual(len(members), 1, f"expected one bundled {name} in {wheel.name}")
                        extracted = Path(tmpdir) / name
                        extracted.write_bytes(archive.read(members[0]))
                        extracted.chmod(extracted.stat().st_mode | stat.S_IXUSR)
                        for flag in ("--version", "--help", "--manual", "--man-page"):
                            self.assertEqual(run_cli(extracted, flag), run_cli(RELEASE_BINARY.with_name(name), flag))


if __name__ == "__main__":
    unittest.main()
