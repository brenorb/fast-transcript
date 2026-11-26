"""Recording E2E with deterministic capture transport, real WAV/FFmpeg, and fixture ASR."""
import json
import os
from pathlib import Path
import signal
import subprocess
import time
import unittest
import wave

from tests import test_streaming_cli as streaming_fixture
BINARY = streaming_fixture.BINARY


class RecordingCLI(unittest.TestCase):
    # Reuse fixture and byte-safe event readers, not the streaming test cases.
    setUpClass = classmethod(streaming_fixture.StreamingCLI.setUpClass.__func__)
    spawn = streaming_fixture.StreamingCLI.spawn
    event = streaming_fixture.StreamingCLI.event
    stop = staticmethod(streaming_fixture.StreamingCLI.stop)
    def command(self, *args, json_output=True):
        return [str(BINARY), 'record', '--model', 'ultra', '--quiet',
                *(['--json'] if json_output else []), *map(str, args)]

    def setUp(self):
        streaming_fixture.StreamingCLI.setUp(self)
        self.wav = self.root / 'capture.wav'
        # One second of the existing independent PCM fixture.
        with wave.open(str(self.fixture.source), 'rb') as source:
            with wave.open(str(self.wav), 'wb') as destination:
                destination.setparams(source.getparams())
                destination.writeframes(source.readframes(source.getframerate()))
        self.env['FSCRIPT_CAPTURE_TEST_WAV'] = str(self.wav)

    def request(self, process, action):
        process.stdin.write((json.dumps({'action': action}) + '\n').encode())
        process.stdin.flush()

    def test_duration_matches_batch_and_only_loads_one_model(self):
        process = self.spawn('--duration', '1.3')
        stdout, stderr = process.communicate(timeout=10)
        self.assertEqual(process.returncode, 0, stderr.decode())
        events = [json.loads(line) for line in stdout.splitlines()]
        self.assertEqual([event['type'] for event in events], ['ready', 'recording', 'processing', 'completed'])
        self.assertEqual([event['sequence'] for event in events], list(range(4)))
        final = events[-1]['data']['result']
        batch = self.fixture.run_cli(self.wav)
        for key in ['text', 'segments', 'audio_seconds', 'chunks', 'chunk_seconds', 'chunk_overlap_seconds']:
            if key == 'chunks':
                for actual, expected in zip(final[key], batch[key]):
                    for field in ['text', 'index', 'start_s', 'end_s', 'audio_seconds']:
                        self.assertEqual(actual[field], expected[field])
            else:
                self.assertEqual(final[key], batch[key], key)
        self.assertFalse(events[-1]['data']['model_reused'])

    def test_control_reuses_model_cancel_has_no_transcript_and_output_is_exportable(self):
        directory = self.root / 'saved'
        process = self.spawn('--control', '--output-dir', directory, stdin=subprocess.PIPE)
        self.assertEqual(self.event(process)['type'], 'ready')
        self.request(process, 'start')
        first_id = self.event(process)['session_id']
        self.request(process, 'cancel')
        self.assertEqual(self.event(process)['type'], 'cancelled')
        self.assertFalse(directory.exists())
        ids = {first_id}
        for index in range(2):
            self.request(process, 'start')
            event = self.event(process)
            self.assertEqual(event['type'], 'recording')
            self.assertNotIn(event['session_id'], ids)
            ids.add(event['session_id'])
            time.sleep(1.2)
            self.request(process, 'stop')
            self.assertEqual(self.event(process)['type'], 'processing')
            event = self.event(process)
            self.assertEqual(event['type'], 'completed', event)
            self.assertEqual(event['data']['model_reused'], index > 0)
            if index: self.assertEqual(event['data']['result']['load_seconds'], 0)
            saved = Path(event['data']['output_path'])
            self.assertEqual(json.loads(saved.read_text()), event['data']['result'])
            exported = subprocess.run([str(BINARY), 'export', str(saved), '--format', 'plain', '--stdout'],
                env=self.env, capture_output=True, text=True, timeout=5)
            self.assertEqual(exported.returncode, 0, exported.stderr)
            self.assertEqual(exported.stdout.strip(), event['data']['result']['text'])
        self.request(process, 'shutdown')
        stdout, stderr = process.communicate(timeout=5)
        self.assertEqual(process.returncode, 0, stderr.decode())
        self.assertEqual(stdout, b'')
        self.assertEqual(len(self.worker_log.read_text().splitlines()), 1)
        self.assertEqual(len(list(directory.glob('*/transcript.json'))), 2)

    def test_sigint_finishes_but_sigterm_cancels_and_cleans_worker(self):
        for signal_value in [signal.SIGINT, signal.SIGTERM]:
            process = self.spawn('--duration', '60')
            self.assertEqual(self.event(process)['type'], 'ready')
            self.assertEqual(self.event(process)['type'], 'recording')
            time.sleep(.15)
            process.send_signal(signal_value)
            stdout, stderr = process.communicate(timeout=8)
            self.assertEqual(process.returncode, 0 if signal_value == signal.SIGINT else 130, stderr.decode())
            events = [json.loads(line) for line in stdout.splitlines()]
            self.assertEqual(any(event['type'] == 'completed' for event in events), signal_value == signal.SIGINT)
            for pid in self.worker_log.read_text().splitlines():
                with self.assertRaises(ProcessLookupError): os.kill(int(pid), 0)

    def test_controller_disconnect_and_invalid_transition_are_explicit(self):
        process = self.spawn('--control', stdin=subprocess.PIPE)
        self.assertEqual(self.event(process)['type'], 'ready')
        self.request(process, 'stop')
        event = self.event(process)
        self.assertEqual(event['type'], 'error')
        self.assertFalse(event['data']['fatal'])
        self.request(process, 'start')
        self.assertEqual(self.event(process)['type'], 'recording')
        process.stdin.close()
        process.stdin = None
        stdout, stderr = process.communicate(timeout=5)
        self.assertNotEqual(process.returncode, 0)
        self.assertIn(b'controller closed before stop', stderr)
        self.assertNotIn(b'"completed"', stdout)

    def test_second_interrupt_cancels_blocked_inference(self):
        self.env['STREAM_HANG_WORKER'] = '1'
        process = self.spawn('--duration', '60')
        self.assertEqual(self.event(process)['type'], 'ready')
        self.assertEqual(self.event(process)['type'], 'recording')
        time.sleep(.1)
        process.send_signal(signal.SIGINT)
        self.assertEqual(self.event(process)['type'], 'processing')
        process.send_signal(signal.SIGINT)
        stdout, stderr = process.communicate(timeout=5)
        self.assertEqual(process.returncode, 130, stderr.decode())
        self.assertNotIn(b'"completed"', stdout)
        for pid in self.worker_log.read_text().splitlines():
            with self.assertRaises(ProcessLookupError): os.kill(int(pid), 0)

    def test_validation_precedes_model_loading(self):
        for args in [[], ['--duration', '0'], ['--duration', 'nan'], ['--duration', '-1'],
                     ['--duration', '1', '--control'], ['--duration', '1', '--hotkey'],
                     ['--hotkey', 'unknown']]:
            result = subprocess.run(self.command(*args), stdin=subprocess.DEVNULL,
                capture_output=True, env=self.env, timeout=5)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(self.worker_log.exists())


if __name__ == '__main__':
    unittest.main()
