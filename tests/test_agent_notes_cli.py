"""Process E2E and differential contracts for external notes; no paid agents in CI."""

import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get("FSCRIPTCTL_TEST_BINARY", ROOT / "target/release/fscriptctl")).resolve()
NOTES = "# Summary\nShip version 2.\n\n## Key points\n- Tests first.\n\n## Decisions\n- Ship Friday.\n\n## Action items\n- Ben: tests by Thursday.\n\n## Open questions\n- Support owner."

AGENT = r'''#!/usr/bin/env python3
import json, os, pathlib, subprocess, sys, time
args = sys.argv[1:]
prompt = sys.stdin.read()
mode = os.environ.get("NOTES_FIXTURE_MODE", "ok")
pids = [os.getpid(), os.getppid()]
if mode == "hang":
    child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
    pids.append(child.pid)
record = dict(provider=pathlib.Path(sys.argv[0]).name, args=args, prompt=prompt,
              cwd=os.getcwd(), scratch=os.environ.get("TMPDIR"), pids=pids)
pathlib.Path(os.environ["NOTES_FIXTURE_LOG"]).write_text(json.dumps(record))
if mode == "hang":
    time.sleep(60)
if mode == "fail":
    print("fixture authentication failed", file=sys.stderr)
    sys.exit(3)
response = os.environ["NOTES_FIXTURE_RESPONSE"]
payload = b"\xff" if mode == "invalid" else (b"" if mode == "empty" else response.encode())
if "--output-last-message" in args:
    pathlib.Path(args[args.index("--output-last-message") + 1]).write_bytes(payload)
    print("This stdout log is not the Codex final message")
else:
    sys.stdout.buffer.write(payload)
'''


class AgentNotesCLI(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not BINARY.is_file():
            raise RuntimeError("Build fscriptctl or set FSCRIPTCTL_TEST_BINARY")

    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="fscript-notes-e2e-")
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.source = self.root / "meeting.txt"
        self.source.write_text("Alice: Ship Friday.\nBen: I will finish tests Thursday.\n", encoding="utf-8")
        self.log = self.root / "agent.json"
        self.config = self.root / "config" / "agents.json"
        self.env = {name: value for name, value in os.environ.items() if not name.startswith("FSCRIPT_")}
        self.env.update(FSCRIPT_AGENT_PATH=str(self.bin), FSCRIPT_NOTES_CONFIG_FILE=str(self.config),
                        NOTES_FIXTURE_LOG=str(self.log), NOTES_FIXTURE_RESPONSE=NOTES)
        for provider in ("codex", "claude", "omp"):
            self.agent(provider)

    def agent(self, name):
        path = self.bin / name
        path.write_text(AGENT)
        path.chmod(0o755)
        return path

    def command(self, *args, json_output=False):
        return [str(BINARY), "--quiet", *(["--json"] if json_output else []), *map(str, args)]

    def run_cli(self, *args, json_output=False, input=None, success=True):
        result = subprocess.run(self.command(*args, json_output=json_output), input=input, capture_output=True,
                                cwd=self.root, env=self.env, timeout=15)
        self.assertEqual(result.returncode == 0, success, result.stderr.decode())
        return json.loads(result.stdout) if json_output and success else result

    def test_help_discovery_and_missing_commands_do_not_start_agents(self):
        for args in [(), ("--help",), ("notes", "--help"), ("agents",), ("agents", "add", "--help"), ("--manual",), ("--man-page",), ("--version",)]:
            with self.subTest(args=args):
                self.assertTrue(self.run_cli(*args).stdout)
        self.assertFalse(self.log.exists())
        self.assertFalse(self.config.exists())
        self.run_cli("nots", str(self.source), success=False)
        self.assertFalse(self.log.exists())
        result = self.run_cli("agents", "list", json_output=True)
        self.assertEqual(result["default_agent"], "codex")
        self.assertEqual([agent["name"] for agent in result["agents"]], ["codex", "claude", "omp"])
        self.assertFalse(self.log.exists())

    def test_default_file_output_and_all_builtin_adapters_have_identical_notes(self):
        default = self.run_cli("notes", self.source)
        path = self.source.with_suffix(".notes.md")
        self.assertEqual(default.stdout.decode().strip(), str(path.resolve()))
        self.assertEqual(path.read_text().strip(), NOTES)
        prompts = []
        for provider in ("codex", "claude", "omp"):
            result = self.run_cli("notes", self.source, "--agent", provider, "--model", "selected-model", "--stdout", json_output=True)
            self.assertEqual(result["notes"], NOTES)
            self.assertEqual(result["agent"], provider)
            self.assertIsNone(result["output_path"])
            record = json.loads(self.log.read_text())
            prompts.append(record["prompt"])
            self.assertIn("selected-model", record["args"])
            self.assertTrue(Path(record["cwd"]).name.startswith("fscript-notes-"))
            self.assertNotIn(self.source.read_text(), " ".join(record["args"]))
            self.assertFalse(Path(record["scratch"]).exists())
            self.assertFalse(Path(record["cwd"]).exists())
        self.assertEqual(prompts[0], prompts[1])
        self.assertEqual(prompts[1], prompts[2])

    def test_stdin_plain_json_and_completed_jsonl_are_differentially_equivalent(self):
        text = "Task: finish tests Thursday. 🦀 漢 é"
        document = {"text": text}
        stream = "\n".join(json.dumps(dict(version=1, job_id="job", sequence=index, type=kind, data=data))
                           for index, (kind, data) in enumerate([("started", {}), ("update", {"text": "partial"}), ("completed", {"result": document})]))
        prompts = []
        for content in (text, json.dumps(document), stream):
            result = self.run_cli("notes", "-", json_output=True, input=content.encode())
            self.assertEqual(result["notes"], NOTES)
            self.assertIsNone(result["output_path"])
            prompts.append(json.loads(self.log.read_text())["prompt"])
        self.assertEqual(prompts[0], prompts[1])
        self.assertEqual(prompts[1], prompts[2])
        self.assertFalse((self.root / "stdin.notes.md").exists())

    def test_timed_input_preserves_original_source_positions_and_speakers(self):
        document = dict(text="raw", segments=[dict(start_s=300.125, end_s=302.0, text="Ship Friday.", speaker="S1")])
        self.run_cli("notes", "-", input=json.dumps(document).encode())
        prompt = json.loads(self.log.read_text())["prompt"]
        quoted = json.loads(prompt.split("Transcript data (JSON):\n", 1)[1])
        self.assertEqual(quoted["transcript"], "[00:05:00.125] SPEAKER_01: Ship Friday.")

    def test_settings_custom_literal_argv_and_default_selection(self):
        self.run_cli("agents", "set", "omp")
        self.assertEqual(self.run_cli("notes", self.source, "--stdout", json_output=True)["agent"], "omp")
        program = self.agent("runner with spaces")
        arguments = ["$(touch NEVER)", "; exit 2", "--json", "a b", ""]
        self.run_cli("agents", "add", "local", "--", program, *arguments, json_output=True)
        self.run_cli("agents", "set", "local")
        result = self.run_cli("notes", self.source, "--stdout", json_output=True)
        self.assertEqual(result["agent"], "local")
        self.assertEqual(json.loads(self.log.read_text())["args"], arguments)
        self.assertFalse((self.root / "NEVER").exists())
        self.run_cli("agents", "remove", "local")
        self.assertEqual(self.run_cli("agents", "list", json_output=True)["default_agent"], "codex")
        before = self.config.read_bytes()
        self.run_cli("agents", "add", "codex", "--", program, success=False)
        self.assertEqual(self.config.read_bytes(), before)

    def test_saved_missing_agent_does_not_silently_send_to_another_provider(self):
        (self.bin / "codex").unlink()
        self.assertEqual(self.run_cli("agents", "list", json_output=True)["default_agent"], "claude")
        self.run_cli("agents", "set", "codex")
        self.run_cli("notes", self.source, success=False)
        self.assertFalse(self.log.exists())
        result = self.run_cli("notes", self.source, "--agent", "auto", "--stdout", json_output=True)
        self.assertEqual(result["agent"], "claude")

    def test_invalid_inputs_and_incomplete_streams_do_not_start_an_agent(self):
        for content in [b"", b"\xff", b'{broken', b'{"version":1,"type":"update","data":{}}',
                        b'{"version":1,"job_id":"j","sequence":0,"type":"started","data":{}}\n']:
            with self.subTest(content=content):
                self.run_cli("notes", "-", input=content, success=False)
                self.assertFalse(self.log.exists())
        for args in [("--timeout", "NaN"), ("--timeout", "0"), ("--agent", "unknown")]:
            self.run_cli("notes", self.source, *args, success=False)
            self.assertFalse(self.log.exists())
        self.run_cli("notes", self.source, "--output", self.source, success=False)
        self.assertFalse(self.log.exists())

    def test_failure_empty_invalid_output_and_timeout_preserve_previous_notes(self):
        destination = self.root / "notes.md"
        for mode in ("fail", "empty", "invalid", "hang"):
            with self.subTest(mode=mode):
                self.env["NOTES_FIXTURE_MODE"] = mode
                destination.write_text("previous notes")
                result = self.run_cli("notes", self.source, destination, "--timeout", "1", success=False)
                self.assertEqual(destination.read_text(), "previous notes")
                if mode == "fail":
                    self.assertIn(b"fixture authentication failed", result.stderr)
                if mode == "hang":
                    self.assertIn(b"timed out", result.stderr)
                    self.assert_processes_stopped(json.loads(self.log.read_text()))

    def test_malformed_settings_can_be_reset_without_touching_models(self):
        self.config.parent.mkdir()
        self.config.write_text("broken")
        self.run_cli("agents", "list", success=False)
        self.run_cli("--help")
        self.run_cli("agents", "reset")
        self.assertFalse(self.config.exists())
        self.assertEqual(self.run_cli("agents", "list", json_output=True)["default_agent"], "codex")

    def test_directory_destination_and_option_like_source(self):
        result = self.run_cli("notes", self.source, "notes/", json_output=True)
        self.assertEqual(Path(result["output_path"]), (self.root / "notes" / "meeting.notes.md").resolve())
        (self.root / "--help").write_text("A transcript")
        result = self.run_cli("notes", "--stdout", "--", "--help", json_output=True)
        self.assertEqual(result["notes"], NOTES)

    def assert_processes_stopped(self, record):
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            alive = []
            for pid in record["pids"]:
                status = subprocess.run(["ps", "-p", str(pid), "-o", "stat="], text=True, capture_output=True).stdout.strip()
                if status and not status.startswith("Z"):
                    alive.append((pid, status))
            if not alive:
                self.assertFalse(Path(record["scratch"]).exists())
                return
            time.sleep(0.05)
        self.fail(f"notes processes remain active: {alive}")

    @unittest.skipUnless(os.name == "posix", "Unix process-group cancellation")
    def test_cancellation_stops_agent_descendants_and_removes_scratch(self):
        self.env["NOTES_FIXTURE_MODE"] = "hang"
        process = subprocess.Popen(self.command("notes", self.source), cwd=self.root, env=self.env,
                                   stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.addCleanup(self.stop, process)
        deadline = time.monotonic() + 5
        record = None
        while time.monotonic() < deadline:
            try:
                record = json.loads(self.log.read_text())
                break
            except (FileNotFoundError, json.JSONDecodeError):
                time.sleep(0.01)
        self.assertIsNotNone(record)
        process.send_signal(signal.SIGTERM)
        self.assertEqual(process.wait(timeout=3), 130)
        self.assert_processes_stopped(record)
        self.assertFalse(self.source.with_suffix(".notes.md").exists())

    @staticmethod
    def stop(process):
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=3)
        for stream in (process.stdout, process.stderr):
            if stream is not None:
                stream.close()


if __name__ == "__main__":
    unittest.main()
