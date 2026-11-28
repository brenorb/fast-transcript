"""CLI E2E and independent FFmpeg differential checks for media selection.

Build fscript first. FFmpeg, ffprobe, and Python are required. Only the model
worker and remote metadata/download transport are fixtures; actual media probing,
seeking, normalization, CLI routing, and output rendering run unchanged.
"""

import hashlib
import http.server
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import threading
import unittest
import wave


ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get("FSCRIPT_TEST_BINARY", ROOT / "target/release/fscript")).resolve()

WORKER = r'''#!/usr/bin/env python3
import hashlib, json, os, shutil, sys, wave
print(json.dumps(dict(ready=True, device="cpu", model_dir="fixture", load_seconds=0.01)), flush=True)
for line in sys.stdin:
    path = json.loads(line)["audio_path"]
    shutil.copyfile(path, os.environ["RANGE_CAPTURE"])
    with wave.open(path) as audio:
        seconds = audio.getnframes() / audio.getframerate()
        text = hashlib.sha256(audio.readframes(audio.getnframes())).hexdigest()
    print(json.dumps(dict(text=text, segments=[dict(start=0, end=seconds, text=text)])), flush=True)
'''

DOWNLOADER = r'''#!/usr/bin/env python3
import json, os, pathlib, subprocess, sys
args = sys.argv[1:]
if "--version" in args:
    print("fixture")
elif "--dump-single-json" in args:
    print(json.dumps(dict(id="clip", title="Clip", duration=None if os.environ.get("RANGE_NO_DURATION") else 6,
                         subtitles={"en": [{"ext": "vtt"}]} if os.environ.get("RANGE_CAPTIONS") else {})))
elif "--skip-download" in args:
    target = args[args.index("--output") + 1].replace("%(id)s", "clip").replace("%(ext)s", "en.vtt")
    pathlib.Path(target).write_text("WEBVTT\n\n00:00:00.000 --> 00:00:02.000\nbefore\n\n00:00:02.000 --> 00:00:04.000\ninside\n\n00:00:04.000 --> 00:00:06.000\nafter\n")
else:
    pathlib.Path(os.environ["RANGE_DOWNLOAD_ARGS"]).write_text(json.dumps(args))
    target = args[args.index("--output") + 1].replace("%(id)s", "clip").replace("%(ext)s", "wav")
    command = ["ffmpeg", "-v", "error", "-y", "-i", os.environ["RANGE_SOURCE"]]
    if "--download-sections" in args:
        start, end = args[args.index("--download-sections") + 1][1:].split("-")
        expression = "atrim=start=" + start
        if end != "inf":
            expression += ":end=" + end
        command += ["-af", expression + ",asetpts=PTS-STARTPTS"]
    subprocess.run(command + [target], check=True)
    print(target)
'''


def pcm(path):
    with wave.open(str(path)) as audio:
        return audio.readframes(audio.getnframes())


class MediaRangeCLI(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        for program in ("ffmpeg", "ffprobe", "python3"):
            if not shutil.which(program):
                raise RuntimeError(f"{program} is required for media selection E2E tests")
        if not BINARY.is_file():
            raise RuntimeError("Build fscript or set FSCRIPT_TEST_BINARY before running these tests")

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="fscript-range-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / "source.wav"
        with wave.open(str(self.source), "wb") as audio:
            audio.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
            # Distinct sample values throughout: reading the wrong section changes the digest.
            audio.writeframes(b"".join(struct.pack("<h", (i * 31) % 60001 - 30000) for i in range(96000)))
        self.capture = self.root / "seen-by-model.wav"
        self.download_args = self.root / "download-args.json"
        self.env = {key: value for key, value in os.environ.items() if not key.startswith("FSCRIPT_")}
        self.env.update(
            PATH=str(self.root) + os.pathsep + os.environ["PATH"],
            FSCRIPT_PYTHON_BINARY=str(self.root / "worker"),
            FSCRIPT_CONFIG_FILE=str(self.root / "config.json"),
            FSCRIPT_MEETING_CONFIG_FILE=str(self.root / "meeting-config.json"),
            FSCRIPT_CALENDAR_CONFIG_FILE=str(self.root / "calendar-config.json"),
            FSCRIPT_DIARIZATION_BINARY=str(self.root / "missing-helper"),
            RANGE_CAPTURE=str(self.capture), RANGE_DOWNLOAD_ARGS=str(self.download_args), RANGE_SOURCE=str(self.source),
        )
        for name, content in (("worker", WORKER), ("yt-dlp", DOWNLOADER)):
            path = self.root / name
            path.write_text(content)
            path.chmod(0o755)

    def run_cli(self, *args, input=None, success=True, diarize=False):
        result = subprocess.run(
            [str(BINARY), "--model", "ultra", *( ["--diarize", "coreml"] if diarize else ["--no-diarization"]), "--json", "--stdout", *map(str, args)],
            input=input, capture_output=True, env=self.env, cwd=self.root, timeout=30,
        )
        self.assertEqual(result.returncode == 0, success, result.stderr.decode())
        return json.loads(result.stdout) if success else result

    def reference(self, start, end):
        target = self.root / "reference.wav"
        subprocess.run([
            "ffmpeg", "-v", "error", "-y", "-i", str(self.source),
            "-af", f"atrim=start={start}:end={end},asetpts=PTS-STARTPTS",
            "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le", str(target),
        ], check=True, capture_output=True)
        return pcm(target)

    def assert_clip(self, result, start, end):
        expected = self.reference(start, end)
        self.assertEqual(pcm(self.capture), expected)
        self.assertEqual(result["text"], hashlib.sha256(expected).hexdigest())
        self.assertAlmostEqual(result["audio_seconds"], end - start, places=4)
        self.assertAlmostEqual(result["segments"][0]["start_s"], start, places=4)
        self.assertAlmostEqual(result["segments"][0]["end_s"], end, places=4)
        self.assertAlmostEqual(result["chunks"][0]["start_s"], start, places=4)
        self.assertAlmostEqual(result["chunks"][-1]["end_s"], end, places=4)

    def test_local_selection_matches_independent_ffmpeg_filter(self):
        for options, start, end in [
            (["--start", "1.25", "--end", "00:03.75"], 1.25, 3.75),
            (["--last", "00:02"], 4, 6),
            (["--last", "300"], 0, 6),
            (["--end", "2"], 0, 2),
            (["--start", "4", "--end", "100"], 4, 6),
            (["--start", "4"], 4, 6),
        ]:
            with self.subTest(options=options):
                self.assert_clip(self.run_cli(self.source, *options), start, end)

    def test_selected_stdin_and_lossless_compressed_media_match_wav(self):
        selected = ["--start", "1.25", "--end", "3.75"]
        self.assert_clip(self.run_cli("-", *selected, input=self.source.read_bytes()), 1.25, 3.75)
        flac = self.root / "source.flac"
        subprocess.run(["ffmpeg", "-v", "error", "-i", str(self.source), str(flac)], check=True)
        self.assert_clip(self.run_cli(flac, *selected), 1.25, 3.75)

    def test_selected_option_like_filename_keeps_end_of_options_protection(self):
        shutil.copyfile(self.source, self.root / "--help")
        self.assert_clip(self.run_cli("--last", "2", "--", "--help"), 4, 6)

    def test_remote_audio_downloads_only_section_and_does_not_seek_twice(self):
        for options, start, end in [(["--last", "2"], 4, 6), (["--start", "1", "--end", "3"], 1, 3)]:
            with self.subTest(options=options):
                self.assert_clip(self.run_cli("https://example.test/video", *options), start, end)
                args = json.loads(self.download_args.read_text())
                self.assertIn(f"*{start}-{end}", args)
                self.assertIn("--force-keyframes-at-cuts", args)
                self.assertIn("ffmpeg:-xerror", args)

    def test_manual_captions_intersect_interval_without_audio_or_model(self):
        self.env["RANGE_CAPTIONS"] = "1"
        result = self.run_cli("https://example.test/video", "--start", "2.5", "--end", "4")
        self.assertEqual(result["text"], "inside")
        self.assertEqual(result["segments"], [dict(start_s=2.5, end_s=4.0, text="inside")])
        self.assertEqual(result["chunks"][0]["start_s"], 2.5)
        self.assertEqual(result["chunks"][0]["end_s"], 4)
        self.assertFalse(self.capture.exists())
        self.assertFalse(self.download_args.exists())
        result = self.run_cli("https://example.test/video", "--last", "2")
        self.assertEqual(result["text"], "after")

    def test_diarization_receives_only_clip_and_assigns_speakers_before_timestamp_shift(self):
        helper = self.root / "diarizer"
        helper.write_text('''#!/usr/bin/env python3
import json, os, pathlib, shutil, sys, wave
shutil.copyfile(sys.argv[2], os.environ["RANGE_DIARIZATION_CAPTURE"])
with wave.open(sys.argv[2]) as audio:
    duration = audio.getnframes() / audio.getframerate()
pathlib.Path(sys.argv[sys.argv.index("--output") + 1]).write_text(json.dumps(dict(
    speakerCount=1, segments=[dict(startTimeSeconds=0, endTimeSeconds=duration, speakerId="SPEAKER_01")])))
''')
        helper.chmod(0o755)
        self.env["FSCRIPT_DIARIZATION_BINARY"] = str(helper)
        capture = self.root / "seen-by-diarizer.wav"
        self.env["RANGE_DIARIZATION_CAPTURE"] = str(capture)
        result = self.run_cli(self.source, "--last", "2", diarize=True)
        self.assert_clip(result, 4, 6)
        self.assertEqual(pcm(capture), pcm(self.capture))
        self.assertEqual(result["segments"][0]["speaker"], "SPEAKER_01")

    def test_invalid_ranges_fail_before_model_or_download(self):
        for options in (["--start", "6"], ["--end", "0"], ["--last", "0"],
                        ["--last", "2", "--start", "0"], ["--start", "NaN"], ["--start", "1:60"]):
            with self.subTest(options=options):
                self.run_cli(self.source, *options, success=False)
                self.assertFalse(self.capture.exists())
        self.env["RANGE_NO_DURATION"] = "1"
        result = self.run_cli("https://example.test/video", "--last", "2", success=False)
        self.assertIn(b"known, finite media duration", result.stderr)
        self.assertFalse(self.download_args.exists())

    def test_actual_yt_dlp_section_over_http(self):
        # Exercise the real downloader against controlled HTTP media, with no
        # dependence on third-party websites or their authentication policies.
        (self.root / "yt-dlp").unlink()
        if not shutil.which("yt-dlp"):
            raise RuntimeError("yt-dlp is required for the actual HTTP download E2E test")
        data = self.source.read_bytes()
        server_options = {"truncate": False}
        class MediaHandler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass
            def do_GET(self):
                # Seeking requires real HTTP range semantics. SimpleHTTPRequestHandler
                # ignores Range, which can make FFmpeg truncate a seeked stream.
                start, end = 0, len(data) - 1
                requested = self.headers.get("Range")
                if requested:
                    first, last = requested.removeprefix("bytes=").split("-", 1)
                    start = int(first)
                    end = min(int(last), end) if last else end
                self.send_response(206 if requested else 200)
                self.send_header("Content-Type", "audio/wav")
                self.send_header("Accept-Ranges", "bytes")
                self.send_header("Content-Length", str(end - start + 1))
                if requested:
                    self.send_header("Content-Range", f"bytes {start}-{end}/{len(data)}")
                self.end_headers()
                try:
                    # A fixed absolute cutoff must precede the requested end.
                    # Halving each range response can still deliver the entire
                    # selected interval (or recover it through another seek).
                    available_end = min(end + 1, len(data) // 4) if server_options["truncate"] else end + 1
                    body = data[start:available_end]
                    self.wfile.write(body)
                except (BrokenPipeError, ConnectionResetError):
                    pass
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), MediaHandler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            result = self.run_cli(f"http://127.0.0.1:{server.server_port}/source.wav", "--start", "1", "--end", "3")
            self.assert_clip(result, 1, 3)
            # Media unavailable before the selected end must fail, even if the
            # downloader retries with different HTTP byte ranges.
            server_options["truncate"] = True
            self.capture.unlink()
            result = self.run_cli(f"http://127.0.0.1:{server.server_port}/source.wav", "--start", "1", "--end", "3", success=False)
            self.assertIn(b"yt-dlp failed", result.stderr)
            self.assertFalse(self.capture.exists())
        finally:
            server.shutdown()
            server.server_close()
            thread.join()


if __name__ == "__main__":
    unittest.main()
