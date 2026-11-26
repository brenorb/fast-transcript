"""Streaming CLI E2E: real decoding, pre-EOF results, cancellation, and batch parity."""

import http.server
import json
import os
from pathlib import Path
import selectors
import shutil
import signal
import subprocess
import threading
import time
import unittest

from tests import test_media_range_cli as media_fixture

BINARY = media_fixture.BINARY
WORKER = media_fixture.WORKER


class StreamingCLI(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        media_fixture.MediaRangeCLI.setUpClass()

    def setUp(self):
        self.fixture = media_fixture.MediaRangeCLI()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.root = self.fixture.root
        self.env = self.fixture.env
        self.worker_log = self.root / "worker-starts"
        self.env["STREAM_WORKER_LOG"] = str(self.worker_log)
        worker = WORKER.replace("import hashlib, json, os, shutil, sys, wave", "import hashlib, json, os, shutil, sys, wave, time\nwith open(os.environ['STREAM_WORKER_LOG'], 'a') as log: log.write(str(os.getpid()) + '\\n')")
        worker = worker.replace('    path = json.loads(line)["audio_path"]', '    if os.environ.get("STREAM_HANG_WORKER"): time.sleep(60)\n    path = json.loads(line)["audio_path"]')
        (self.root / "worker").write_text(worker)

    def command(self, *args, json_output=True):
        return [str(BINARY), "--model", "ultra", "--quiet", *(["--json"] if json_output else []), *map(str, args)]

    def spawn(self, *args, json_output=True, stdin=subprocess.DEVNULL):
        process = subprocess.Popen(self.command(*args, json_output=json_output), stdin=stdin,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, cwd=self.root, env=self.env, bufsize=0)
        self.addCleanup(self.stop, process)
        return process

    @staticmethod
    def stop(process):
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=3)
        for stream in (process.stdin, process.stdout, process.stderr):
            if stream is not None:
                stream.close()

    def event(self, process, timeout=5):
        # Read one byte at a time to avoid BufferedReader prefetch hiding lines
        # from selector readiness when proving an update precedes EOF.
        deadline = time.monotonic() + timeout
        line = bytearray()
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            while time.monotonic() < deadline:
                if not selector.select(max(0, deadline - time.monotonic())):
                    break
                byte = process.stdout.read(1)
                if not byte:
                    self.fail(f"stream ended before an event: {process.stderr.read().decode()}")
                if byte == b"\n":
                    return json.loads(line)
                line.extend(byte)
        self.fail(f"no event within {timeout}s; received {line[:200]!r}")

    def through(self, process, kind):
        while True:
            event = self.event(process)
            if event["type"] == kind:
                return event

    def finished(self, process, prefix=(), expected_status=0):
        stdout, stderr = process.communicate(timeout=15)
        self.assertEqual(process.returncode, expected_status, stderr.decode())
        events = [*prefix, *(json.loads(line) for line in stdout.splitlines())]
        self.assertTrue(events)
        self.assertEqual([e["sequence"] for e in events], list(range(len(events))))
        self.assertEqual({e["version"] for e in events}, {1})
        self.assertEqual(len({e["job_id"] for e in events}), 1)
        return events

    def test_stream_and_batch_match_for_identical_chunk_settings_and_export(self):
        options = ["--chunk", "2.5", "--overlap", "0.5", "--raw"]
        batch = self.fixture.run_cli(self.fixture.source, *options)
        self.worker_log.unlink()
        destination = self.root / "result.json"
        process = self.spawn(self.fixture.source, destination, "--stream", *options)
        events = self.finished(process)
        self.assertEqual(events[0]["type"], "started")
        self.assertEqual(events[1]["type"], "ready")
        self.assertEqual(events[-1]["type"], "completed")
        final = events[-1]["data"]["result"]
        for key in ("text", "segments", "audio_seconds", "chunk_count", "chunk_seconds", "chunk_overlap_seconds", "model"):
            self.assertEqual(final[key], batch[key], key)
        for actual, expected in zip(final["chunks"], batch["chunks"]):
            for key in ("index", "start_s", "end_s", "text", "audio_seconds"):
                self.assertEqual(actual[key], expected[key], key)
        self.assertEqual(json.loads(destination.read_text()), final)
        self.assertEqual(events[-1]["data"]["output_path"], str(destination.resolve()))
        self.assertEqual(len(self.worker_log.read_text().splitlines()), 1)
        updates = [e["data"] for e in events if e["type"] == "update"]
        self.assertEqual(len(updates), final["chunk_count"])
        self.assertEqual(updates[-1]["text"], final["text"])
        self.assertEqual(updates[-1]["segments"], final["segments"])

    def test_short_default_opening_and_human_text(self):
        events = self.finished(self.spawn(self.fixture.source, "--stream"))
        updates = [e["data"] for e in events if e["type"] == "update"]
        self.assertEqual(updates[0]["chunk"]["audio_seconds"], 2)
        self.assertEqual(updates[1]["chunk"]["start_s"], 1.5)
        self.assertNotIn("speaker_diarization", events[-1]["data"]["result"])
        process = self.spawn(self.fixture.source, "--stream", "--chunk", "2.5", "--overlap", "0.5", "--raw", json_output=False)
        stdout, stderr = process.communicate(timeout=10)
        self.assertEqual(process.returncode, 0, stderr.decode())
        batch = self.fixture.run_cli(self.fixture.source, "--chunk", "2.5", "--overlap", "0.5", "--raw")
        self.assertEqual(stdout.decode().strip(), batch["text"])

    def test_compressed_media_matches_batch_and_empty_input_fails(self):
        for extension in ("flac", "mp3", "m4a"):
            with self.subTest(format=extension):
                media = self.root / f"compressed.{extension}"
                subprocess.run(["ffmpeg", "-v", "error", "-i", str(self.fixture.source), str(media)], check=True)
                options = ["--chunk", "2", "--overlap", "0.5", "--raw"]
                batch = self.fixture.run_cli(media, *options)
                result = self.finished(self.spawn(media, "--stream", *options))[-1]["data"]["result"]
                for key in ("text", "segments", "audio_seconds", "chunk_count"):
                    self.assertEqual(result[key], batch[key], key)
        process = self.spawn("-", "--stream", stdin=subprocess.PIPE)
        process.stdin.close()
        process.stdin = None
        events = self.finished(process, expected_status=1)
        self.assertEqual(events[-1]["type"], "error")
        self.assertNotIn("completed", [event["type"] for event in events])

    def test_empty_opening_keeps_short_chunks_until_first_text(self):
        worker = (self.root / "worker").read_text().replace("for line in sys.stdin:", "for index, line in enumerate(sys.stdin):")
        worker = worker.replace("    print(json.dumps(dict(text=text,", '    if index < 2: text = ""\n    print(json.dumps(dict(text=text,')
        (self.root / "worker").write_text(worker)
        events = self.finished(self.spawn(self.fixture.source, "--stream"))
        updates = [event["data"] for event in events if event["type"] == "update"]
        self.assertEqual([update["chunk"]["audio_seconds"] for update in updates[:3]], [2, 2, 2])
        self.assertEqual([update["chunk"]["start_s"] for update in updates[:3]], [0, 1.5, 3])
        self.assertFalse(updates[0]["text"])
        self.assertFalse(updates[1]["text"])
        self.assertTrue(updates[2]["text"])

    def test_selection_keeps_source_positions(self):
        process = self.spawn(self.fixture.source, "--stream", "--start", "1.25", "--end", "5.25", "--chunk", "2", "--overlap", "0")
        events = self.finished(process)
        result = events[-1]["data"]["result"]
        self.assertEqual(result["audio_seconds"], 4)
        self.assertEqual(result["chunks"][0]["start_s"], 1.25)
        self.assertEqual(result["chunks"][-1]["end_s"], 5.25)
        updates = [e["data"] for e in events if e["type"] == "update"]
        self.assertEqual(updates[0]["segments"][0]["start_s"], 1.25)
        self.assertEqual(updates[-1]["segments"], result["segments"])

    def test_manual_captions_and_last_stdin(self):
        self.env["RANGE_CAPTIONS"] = "1"
        events = self.finished(self.spawn("https://example.test/video", "--stream", "--last", "2"))
        result = events[-1]["data"]["result"]
        self.assertEqual(result["text"], "after")
        self.assertEqual(result["segments"][0]["start_s"], 4)
        self.assertFalse(self.worker_log.exists())
        process = self.spawn("-", "--stream", "--last", "2", stdin=subprocess.PIPE)
        process.stdin.write(self.fixture.source.read_bytes())
        process.stdin.close()
        process.stdin = None
        result = self.finished(process)[-1]["data"]["result"]
        self.assertEqual(result["audio_seconds"], 2)
        self.assertEqual(result["chunks"][0]["start_s"], 4)

    def test_final_diarization_uses_selected_audio_and_keeps_updates_provisional(self):
        helper = self.root / "diarizer"
        helper.write_text('''#!/usr/bin/env python3
import json, os, pathlib, sys, wave
with wave.open(sys.argv[2]) as audio:
    duration = audio.getnframes() / audio.getframerate()
pathlib.Path(os.environ["STREAM_DIARIZATION_DURATION"]).write_text(str(duration))
pathlib.Path(sys.argv[sys.argv.index("--output") + 1]).write_text(json.dumps(dict(
    speakerCount=1, segments=[dict(startTimeSeconds=0, endTimeSeconds=duration, speakerId="SPEAKER_01")])))
''')
        helper.chmod(0o755)
        duration = self.root / "diarization-duration"
        self.env["FSCRIPT_DIARIZATION_BINARY"] = str(helper)
        self.env["STREAM_DIARIZATION_DURATION"] = str(duration)
        events = self.finished(self.spawn(self.fixture.source, "--stream", "--last", "4", "--diarize", "coreml"))
        self.assertEqual(float(duration.read_text()), 4)
        result = events[-1]["data"]["result"]
        self.assertEqual(result["segments"][0]["start_s"], 2)
        self.assertEqual(result["segments"][0]["speaker"], "SPEAKER_01")
        for update in (e["data"] for e in events if e["type"] == "update"):
            self.assertTrue(update["provisional"])
            self.assertTrue(all("speaker" not in segment for segment in update["segments"]))

    def test_stdin_emits_before_eof(self):
        process = self.spawn("-", "--stream", "--chunk", "1", "--overlap", "0", stdin=subprocess.PIPE)
        content = self.fixture.source.read_bytes()
        split = 44 + 3 * 16000 * 2
        process.stdin.write(content[:split])
        process.stdin.flush()
        prefix = []
        while not prefix or prefix[-1]["type"] != "update":
            prefix.append(self.event(process))
        self.assertIsNone(process.poll(), "stdin was still open with media remaining")
        self.assertTrue(prefix[-1]["data"]["text"])
        process.stdin.write(content[split:])
        process.stdin.close()
        process.stdin = None
        events = self.finished(process, prefix)
        self.assertEqual(events[-1]["data"]["result"]["audio_seconds"], 6)

    def test_remote_emits_before_http_download_finishes(self):
        (self.root / "yt-dlp").unlink()
        if not shutil.which("yt-dlp"):
            raise RuntimeError("yt-dlp is required for streaming HTTP E2E")
        data = self.fixture.source.read_bytes()
        release = threading.Event()
        transferred_all = threading.Event()
        class MediaHandler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass
            def do_GET(self):
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
                    body = data[start:end + 1]
                    self.wfile.write(body[:96044])
                    self.wfile.flush()
                    release.wait(timeout=10)
                    self.wfile.write(body[96044:])
                    self.wfile.flush()
                    transferred_all.set()
                except (BrokenPipeError, ConnectionResetError):
                    pass
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), MediaHandler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        try:
            process = self.spawn(f"http://127.0.0.1:{server.server_port}/source.wav", "--stream", "--local", "--chunk", "1", "--overlap", "0")
            prefix = []
            while not prefix or prefix[-1]["type"] != "update":
                prefix.append(self.event(process, timeout=7))
            self.assertFalse(transferred_all.is_set(), "transcript must arrive while download is incomplete")
            self.assertIsNone(process.poll())
            release.set()
            events = self.finished(process, prefix)
            self.assertEqual(events[-1]["type"], "completed")
            self.assertEqual(events[-1]["data"]["result"]["audio_seconds"], 6)
        finally:
            release.set()
            server.shutdown()
            server.server_close()

    def test_failure_has_terminal_error_and_preserves_existing_output(self):
        destination = self.root / "result.json"
        destination.write_text("original")
        worker = (self.root / "worker").read_text().replace('for line in sys.stdin:', 'for index, line in enumerate(sys.stdin):\n    if index == 1: raise RuntimeError("injected inference failure")')
        (self.root / "worker").write_text(worker)
        process = self.spawn(self.fixture.source, destination, "--stream", "--chunk", "2", "--overlap", "0")
        events = self.finished(process, expected_status=1)
        self.assertEqual(events[-1]["type"], "error")
        self.assertIn("injected inference failure", events[-1]["data"]["message"])
        self.assertNotIn("completed", [e["type"] for e in events])
        self.assertEqual(destination.read_text(), "original")

    def fake_decoder(self):
        path = self.root / "ffmpeg"
        path.write_text('''#!/usr/bin/env python3
import json, os, pathlib, subprocess, sys, time
child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
pathlib.Path(os.environ["STREAM_DECODER_STATE"]).write_text(json.dumps(dict(pids=[os.getpid(), child.pid, os.getppid()], scratch=os.environ["TMPDIR"])))
while True:
    sys.stdout.buffer.write(b"\\0\\0" * 16000)
    sys.stdout.buffer.flush()
    time.sleep(0.1)
''')
        path.chmod(0o755)
        self.decoder_state = self.root / "decoder-state.json"
        self.env["STREAM_DECODER_STATE"] = str(self.decoder_state)

    def assert_stopped(self, pids):
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            active = []
            for pid in pids:
                state = subprocess.run(["ps", "-p", str(pid), "-o", "stat="], capture_output=True, text=True).stdout.strip()
                if state and not state.startswith("Z"):
                    active.append((pid, state))
            if not active:
                return
            time.sleep(0.05)
        self.fail(f"orphaned processes: {active}")

    def read_decoder_state(self):
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            try:
                return json.loads(self.decoder_state.read_text())
            except (FileNotFoundError, json.JSONDecodeError):
                time.sleep(0.01)
        self.fail("decoder did not report its process state")

    @unittest.skipUnless(os.name == "posix", "Unix process-group cancellation")
    def test_interrupt_and_termination_stop_decoder_model_and_descendants(self):
        self.fake_decoder()
        for signum, hang in ((signal.SIGINT, False), (signal.SIGTERM, True), (signal.SIGHUP, True)):
            with self.subTest(signal=signum, blocked_model=hang):
                self.env["STREAM_HANG_WORKER"] = "1" if hang else ""
                self.decoder_state.unlink(missing_ok=True)
                process = self.spawn(self.fixture.source, "--stream", "--chunk", "1", "--overlap", "0")
                self.through(process, "ready" if hang else "update")
                state = self.read_decoder_state()
                pids = [*state["pids"], *map(int, self.worker_log.read_text().splitlines())]
                process.send_signal(signum)
                self.assertEqual(process.wait(timeout=3), 130)
                self.assert_stopped(pids)
                self.assertFalse(Path(state["scratch"]).exists())

    @unittest.skipUnless(os.name == "posix", "Unix process-group cleanup")
    def test_closed_stdout_stops_stream_successfully(self):
        self.fake_decoder()
        process = self.spawn(self.fixture.source, "--stream", "--chunk", "1", "--overlap", "0")
        self.through(process, "update")
        state = self.read_decoder_state()
        process.stdout.close()
        self.assertEqual(process.wait(timeout=3), 0, process.stderr.read().decode())
        self.assert_stopped(state["pids"])
        self.assertFalse(Path(state["scratch"]).exists())


if __name__ == "__main__":
    unittest.main()
