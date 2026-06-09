from __future__ import annotations

import json
import os
import re
import shutil
import socketserver
import subprocess
import tempfile
import threading
import unittest
from http.server import SimpleHTTPRequestHandler
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RELEASE_BINARY = ROOT / "target" / "release" / "fscript"
HOMEBREW_BINARY = Path("/opt/homebrew/bin/fscript")
PACKAGE_VERSION = next(
    line.split('"')[1]
    for line in (ROOT / "Cargo.toml").read_text(encoding="utf-8").splitlines()
    if line.startswith("version = ")
)
DEFAULT_MODEL_DIR = (
    Path.home()
    / "Library"
    / "Application Support"
    / "fast-transcript"
    / "models"
    / "parakeet-tdt-0.6b-v3-int8"
)


def available_binaries() -> list[tuple[str, Path]]:
    binaries = [("release", RELEASE_BINARY)]
    if HOMEBREW_BINARY.exists():
        binaries.append(("homebrew", HOMEBREW_BINARY))
    return binaries


@unittest.skipUnless(shutil.which("ffmpeg"), "ffmpeg is required for CLI smoke tests")
@unittest.skipUnless(shutil.which("say"), "say is required for CLI smoke tests")
class CliSmokeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        subprocess.run(
            ["cargo", "build", "--release"],
            cwd=ROOT,
            check=True,
        )
        cls.binaries = available_binaries()
        cls.modern_binaries = [
            (label, binary)
            for label, binary in cls.binaries
            if cls.binary_supports_new_diarization_flags(binary)
        ]
        cls.shared_tmpdir = tempfile.TemporaryDirectory(prefix="fscript-cli-smoke-")
        cls.shared_root = Path(cls.shared_tmpdir.name)
        cls.fixture_audio = cls.create_spoken_wav(cls.shared_root)

    @classmethod
    def tearDownClass(cls) -> None:
        cls.shared_tmpdir.cleanup()

    @classmethod
    def binary_supports_new_diarization_flags(cls, binary: Path) -> bool:
        result = subprocess.run(
            [str(binary), "--help"],
            cwd=ROOT,
            text=True,
            capture_output=True,
            check=False,
            timeout=60,
        )
        return "--no-diarization" in result.stdout

    @classmethod
    def create_spoken_wav(cls, directory: Path) -> Path:
        aiff_path = directory / "speech.aiff"
        wav_path = directory / "speech.wav"
        subprocess.run(
            [
                "say",
                "-r",
                "150",
                "-o",
                str(aiff_path),
                "Fast transcript smoke test sentence for command line coverage.",
            ],
            check=True,
            capture_output=True,
            text=True,
        )
        subprocess.run(
            [
                "ffmpeg",
                "-i",
                str(aiff_path),
                "-ar",
                "16000",
                "-ac",
                "1",
                "-y",
                str(wav_path),
            ],
            check=True,
            capture_output=True,
            text=True,
        )
        return wav_path

    def run_cli(
        self,
        binary: Path,
        *args: str,
        env: dict[str, str] | None = None,
        cwd: Path | None = None,
    ) -> subprocess.CompletedProcess[str]:
        cli_env = os.environ.copy()
        if env is not None:
            cli_env.update(env)
        return subprocess.run(
            [str(binary), *args],
            cwd=cwd or ROOT,
            env=cli_env,
            text=True,
            capture_output=True,
            check=False,
            timeout=300,
        )

    def audio_copy(self, directory: Path) -> Path:
        destination = directory / "speech.wav"
        shutil.copyfile(self.fixture_audio, destination)
        return destination

    def write_fake_diarization_helper(
        self,
        directory: Path,
        *,
        speaker_count: int = 2,
    ) -> tuple[Path, Path]:
        args_path = directory / "fake-diarization-args.txt"
        helper_path = directory / "fake-fluidaudiocli"
        helper_path.write_text(
            f"""#!/bin/sh
set -eu

printf '%s\n' "$@" > "{args_path}"

output_path=""
previous=""
for arg in "$@"; do
  if [ "$previous" = "--output" ]; then
    output_path="$arg"
    break
  fi
  previous="$arg"
done

cat > "$output_path" <<'JSON'
{{
  "speakerCount": {speaker_count},
  "segments": [
    {{"startTimeSeconds": 0.0, "endTimeSeconds": 60.0, "speakerId": "S1"}}
  ]
}}
JSON
""",
            encoding="utf-8",
        )
        helper_path.chmod(0o755)
        return helper_path, args_path

    def start_static_http_server(
        self,
        directory: Path,
    ) -> tuple[socketserver.TCPServer, threading.Thread, str]:
        class QuietHandler(SimpleHTTPRequestHandler):
            def __init__(self, *args, **kwargs):
                super().__init__(*args, directory=str(directory), **kwargs)

            def log_message(self, format: str, *args: object) -> None:
                return None

        server = socketserver.TCPServer(("127.0.0.1", 0), QuietHandler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        url = f"http://127.0.0.1:{server.server_address[1]}/speech.wav"
        return server, thread, url

    def assert_timestamped_text(self, text: str) -> None:
        stripped = text.strip()
        self.assertTrue(stripped, "expected timestamped text output")
        self.assertIn(" - ", stripped)

    def assert_plain_text(self, text: str) -> None:
        stripped = text.strip()
        self.assertTrue(stripped, "expected plain text output")
        self.assertNotIn("WEBVTT", stripped)
        self.assertNotIn("-->", stripped)

    def assert_srt(self, text: str) -> None:
        stripped = text.strip()
        self.assertTrue(stripped, "expected SRT output")
        self.assertIn("-->", stripped)

    def assert_vtt(self, text: str) -> None:
        stripped = text.strip()
        self.assertTrue(stripped, "expected VTT output")
        self.assertTrue(stripped.startswith("WEBVTT"))

    def test_help_and_version_aliases(self) -> None:
        for label, binary in self.binaries:
            with self.subTest(binary=label, flag="--help"):
                result = self.run_cli(binary, "--help")
                self.assertEqual(result.returncode, 0, result.stderr)
                if self.binary_supports_new_diarization_flags(binary):
                    self.assertIn("Usage:", result.stdout)
                    self.assertIn("Default:", result.stdout)
                    self.assertIn("Output:", result.stdout)
                    self.assertIn("--speakers[=plain|timestamps]", result.stdout)
                    self.assertIn("Examples:", result.stdout)
                else:
                    self.assertIn("usage: fscript", result.stdout)

            with self.subTest(binary=label, flag="-h"):
                result = self.run_cli(binary, "-h")
                self.assertEqual(result.returncode, 0, result.stderr)
                if self.binary_supports_new_diarization_flags(binary):
                    self.assertIn("Usage:", result.stdout)
                else:
                    self.assertIn("usage: fscript", result.stdout)

            with self.subTest(binary=label, flag="--version"):
                result = self.run_cli(binary, "--version")
                self.assertEqual(result.returncode, 0, result.stderr)
                if label == "release":
                    self.assertIn(f"fscript {PACKAGE_VERSION}", result.stdout)
                else:
                    self.assertRegex(result.stdout, r"^fscript \d+\.\d+\.\d+\n?$")

            with self.subTest(binary=label, flag="-V"):
                result = self.run_cli(binary, "-V")
                self.assertEqual(result.returncode, 0, result.stderr)
                if label == "release":
                    self.assertIn(f"fscript {PACKAGE_VERSION}", result.stdout)
                else:
                    self.assertRegex(result.stdout, r"^fscript \d+\.\d+\.\d+\n?$")

    def test_output_formats_and_output_path_variants(self) -> None:
        for label, binary in self.modern_binaries:
            with tempfile.TemporaryDirectory(prefix=f"fscript-output-{label}-") as tmpdir:
                root = Path(tmpdir)
                audio_path = self.audio_copy(root)
                output_dir = root / "outdir"
                output_dir.mkdir()
                explicit_output = root / "explicit.transcript.txt"
                equals_output = root / "explicit-equals.speakers.txt"
                srt_output = root / "subtitles.srt"
                vtt_output = root / "subtitles.vtt"

                cases = [
                    {
                        "name": "json-stdout-flag",
                        "args": [
                            str(audio_path),
                            "-D",
                            "--json",
                            "--stdout",
                        ],
                        "stdout_mode": "json",
                    },
                    {
                        "name": "text-timestamps-stdout-dash",
                        "args": [
                            str(audio_path),
                            "-",
                            "--no-diarization",
                            "--text=timestamps",
                        ],
                        "stdout_mode": "timestamped",
                    },
                    {
                        "name": "text-default-timestamps-positional-dir",
                        "args": [
                            str(audio_path),
                            str(output_dir),
                            "-D",
                            "--text",
                        ],
                        "file_path": output_dir / "speech.transcript.txt",
                        "validator": self.assert_timestamped_text,
                    },
                    {
                        "name": "text-plain-short-output-flag",
                        "args": [
                            str(audio_path),
                            "--no-diarization",
                            "--text=plain",
                            "-o",
                            str(explicit_output),
                        ],
                        "file_path": explicit_output,
                        "validator": self.assert_plain_text,
                    },
                    {
                        "name": "speakers-timestamps-space-value",
                        "args": [
                            str(audio_path),
                            str(output_dir),
                            "-D",
                            "--speakers",
                            "timestamps",
                        ],
                        "file_path": output_dir / "speech.speakers.txt",
                        "validator": self.assert_timestamped_text,
                    },
                    {
                        "name": "speakers-plain-output-equals",
                        "args": [
                            str(audio_path),
                            "--no-diarization",
                            "--speakers=plain",
                            f"--output={equals_output}",
                        ],
                        "file_path": equals_output,
                        "validator": self.assert_plain_text,
                    },
                    {
                        "name": "srt-clean-positional-file",
                        "args": [
                            str(audio_path),
                            str(srt_output),
                            "-D",
                            "--srt",
                            "--clean",
                        ],
                        "file_path": srt_output,
                        "validator": self.assert_srt,
                    },
                    {
                        "name": "vtt-raw-positional-file",
                        "args": [
                            str(audio_path),
                            str(vtt_output),
                            "--no-diarization",
                            "--vtt",
                            "--raw",
                        ],
                        "file_path": vtt_output,
                        "validator": self.assert_vtt,
                    },
                ]

                for case in cases:
                    with self.subTest(binary=label, case=case["name"]):
                        result = self.run_cli(binary, *case["args"])
                        self.assertEqual(result.returncode, 0, result.stderr)
                        if case.get("stdout_mode") == "json":
                            payload = json.loads(result.stdout)
                            self.assertEqual(payload["input_source"], str(audio_path))
                            self.assertGreater(payload["audio_seconds"], 0.0)
                        elif case.get("stdout_mode") == "timestamped":
                            self.assert_timestamped_text(result.stdout)
                        else:
                            written_path = Path(result.stdout.strip())
                            expected_path = case["file_path"].resolve()
                            self.assertEqual(written_path.resolve(), expected_path)
                            case["validator"](expected_path.read_text(encoding="utf-8"))

    def test_chunk_and_model_override_flags(self) -> None:
        if not DEFAULT_MODEL_DIR.exists():
            self.skipTest(f"missing default model dir at {DEFAULT_MODEL_DIR}")

        for label, binary in self.modern_binaries:
            with tempfile.TemporaryDirectory(prefix=f"fscript-chunk-{label}-") as tmpdir:
                root = Path(tmpdir)
                audio_path = self.audio_copy(root)
                fake_package = root / "unused-model-package.tar.gz"

                with self.subTest(binary=label, case="chunk-overlap-long-flags"):
                    result = self.run_cli(
                        binary,
                        str(audio_path),
                        "-D",
                        "--json",
                        "--stdout",
                        "--chunk",
                        "0.4",
                        "--overlap",
                        "0.1",
                    )
                    self.assertEqual(result.returncode, 0, result.stderr)
                    payload = json.loads(result.stdout)
                    self.assertEqual(payload["chunk_seconds"], 0.4)
                    self.assertEqual(payload["chunk_overlap_seconds"], 0.1)
                    self.assertGreaterEqual(payload["chunk_count"], 2)

                with self.subTest(binary=label, case="chunk-disable-and-model-overrides"):
                    result = self.run_cli(
                        binary,
                        str(audio_path),
                        "--no-diarization",
                        "--json",
                        "--stdout",
                        "--chunk-seconds",
                        "0",
                        "--chunk-overlap-seconds",
                        "0",
                        "--model-dir",
                        str(DEFAULT_MODEL_DIR),
                        "--model-package",
                        str(fake_package),
                        "--model-url",
                        "https://example.invalid/parakeet.tar.gz",
                    )
                    self.assertEqual(result.returncode, 0, result.stderr)
                    payload = json.loads(result.stdout)
                    self.assertIsNone(payload["chunk_seconds"])
                    self.assertEqual(payload["chunk_overlap_seconds"], 0.0)
                    self.assertEqual(payload["model_dir"], str(DEFAULT_MODEL_DIR))

    def test_diarization_backend_aliases_and_options(self) -> None:
        for label, binary in self.modern_binaries:
            with tempfile.TemporaryDirectory(prefix=f"fscript-diarize-{label}-") as tmpdir:
                root = Path(tmpdir)
                audio_path = self.audio_copy(root)
                helper_path, args_path = self.write_fake_diarization_helper(root)
                env = {"FSCRIPT_DIARIZATION_BINARY": str(helper_path)}

                cases = [
                    {
                        "name": "diarize-coreml-long-flag",
                        "args": [
                            str(audio_path),
                            "--diarize",
                            "coreml",
                            "--num-speakers",
                            "2",
                            "--json",
                            "--stdout",
                        ],
                        "expected_args": [
                            "process",
                            "--mode",
                            "offline",
                            "--num-speakers",
                            "2",
                        ],
                        "expected_backend_fragment": "process --mode offline",
                    },
                    {
                        "name": "short-diarize-coreml-value",
                        "args": [
                            str(audio_path),
                            "-d",
                            "coreml",
                            "--json",
                            "--stdout",
                        ],
                        "expected_args": ["process", "--mode", "offline"],
                        "expected_backend_fragment": "process --mode offline",
                    },
                    {
                        "name": "diarize-short-flag",
                        "args": [
                            str(audio_path),
                            "-d",
                            "--json",
                            "--stdout",
                        ],
                        "expected_args": ["process", "--mode", "offline"],
                        "expected_backend_fragment": "process --mode offline",
                    },
                    {
                        "name": "diarize-equals-coreml",
                        "args": [
                            str(audio_path),
                            "--diarize=coreml",
                            "--json",
                            "--stdout",
                        ],
                        "expected_args": ["process", "--mode", "offline"],
                        "expected_backend_fragment": "process --mode offline",
                    },
                    {
                        "name": "diarize-lseend-with-threshold",
                        "args": [
                            str(audio_path),
                            "--diarize",
                            "lseend-dihard3",
                            "--threshold",
                            "0.4",
                            "--json",
                            "--stdout",
                        ],
                        "expected_args": [
                            "lseend",
                            "--variant",
                            "dihard3",
                            "--threshold",
                            "0.4",
                        ],
                        "expected_backend_fragment": "ls-eend-coreml",
                    },
                    {
                        "name": "short-diarize-lseend-default-threshold",
                        "args": [
                            str(audio_path),
                            "-d",
                            "lseend-dihard3",
                            "--json",
                            "--stdout",
                        ],
                        "expected_args": [
                            "lseend",
                            "--variant",
                            "dihard3",
                            "--threshold",
                            "0.3",
                        ],
                        "expected_backend_fragment": "ls-eend-coreml",
                    },
                    {
                        "name": "diarize-space-value-and-short-threshold",
                        "args": [
                            str(audio_path),
                            "--diarize",
                            "lseend-dihard3",
                            "-t",
                            "0.45",
                            "--json",
                            "--stdout",
                        ],
                        "expected_args": [
                            "lseend",
                            "--variant",
                            "dihard3",
                            "--threshold",
                            "0.45",
                        ],
                        "expected_backend_fragment": "ls-eend-coreml",
                    },
                ]

                for case in cases:
                    with self.subTest(binary=label, case=case["name"]):
                        if args_path.exists():
                            args_path.unlink()
                        result = self.run_cli(binary, *case["args"], env=env)
                        self.assertEqual(result.returncode, 0, result.stderr)
                        payload = json.loads(result.stdout)
                        diarization = payload["speaker_diarization"]
                        self.assertIn(
                            case["expected_backend_fragment"],
                            diarization["backend"],
                        )
                        seen_args = args_path.read_text(encoding="utf-8").splitlines()
                        for expected_arg in case["expected_args"]:
                            self.assertIn(expected_arg, seen_args)

    def test_remote_local_aliases(self) -> None:
        if shutil.which("yt-dlp") is None:
            self.skipTest("yt-dlp is required for remote CLI smoke tests")

        for label, binary in self.modern_binaries:
            with tempfile.TemporaryDirectory(prefix=f"fscript-remote-{label}-") as tmpdir:
                root = Path(tmpdir)
                self.audio_copy(root)
                server, thread, remote_url = self.start_static_http_server(root)
                try:
                    for flag in ("-l", "--local", "--prefer-local-for-remote"):
                        with self.subTest(binary=label, flag=flag):
                            result = self.run_cli(
                                binary,
                                remote_url,
                                flag,
                                "-D",
                                "--json",
                                "--stdout",
                            )
                            self.assertEqual(result.returncode, 0, result.stderr)
                            payload = json.loads(result.stdout)
                            self.assertEqual(payload["input_source"], remote_url)
                            self.assertEqual(
                                payload["transcript_source"],
                                "downloaded-audio-local-model",
                            )
                finally:
                    server.shutdown()
                    server.server_close()
                    thread.join(timeout=5)


if __name__ == "__main__":
    unittest.main()
