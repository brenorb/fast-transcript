"""Passive call inspection E2E: no model, media tools, capture, or authorization."""
import json
import os
from pathlib import Path
import selectors
import signal
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get('FSCRIPT_TEST_BINARY', ROOT / 'target/release/fscript')).resolve()


class CallsCLI(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not BINARY.is_file():
            raise RuntimeError('Build fscript first, or set FSCRIPT_TEST_BINARY to a compiled binary')

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='fscript-calls-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.trace = self.root / 'trace.json'
        config = self.root / 'invalid-settings.json'
        config.write_text('{broken')
        self.env = dict(os.environ, PATH='/missing/tools', FSCRIPT_CONFIG_FILE=str(config),
                        FSCRIPT_PYTHON_BINARY='/missing/worker', FSCRIPT_CALLS_TEST_TRACE=str(self.trace))

    def frame(self, at, inputs=True, outputs=True, complete=True):
        return {'at_ms': at, 'complete': complete, 'clients': [
            {'pid': 12, 'bundle_id': 'US.ZOOM.XOS.helper', 'input': inputs, 'output': outputs}]}

    def write_trace(self, frames, hold=False):
        self.trace.write_text(json.dumps({'frames': frames, 'hold': hold}))

    def cli(self, *args, success=True):
        result = subprocess.run([str(BINARY), 'calls', *args], env=self.env, cwd=self.root,
                                text=True, capture_output=True, timeout=5)
        self.assertEqual(result.returncode == 0, success, result.stderr)
        return result

    def test_snapshot_is_a_candidate_and_does_not_initialize_transcription(self):
        self.write_trace([self.frame(0)])
        before = set(self.root.iterdir())
        plain = self.cli().stdout
        self.assertIn('candidates: Zoom', plain)
        snapshot = json.loads(self.cli('--json', '--quiet').stdout)
        self.assertEqual(snapshot['version'], 1)
        self.assertEqual(snapshot['candidates'], ['Zoom'])
        self.assertTrue(snapshot['snapshot']['complete'])
        self.assertEqual(set(self.root.iterdir()), before)

    def test_watch_streams_one_start_and_end_with_monotonic_envelopes(self):
        self.write_trace([self.frame(t, t <= 6_000, t <= 6_000) for t in range(0, 30_000, 2_000)])
        result = self.cli('--watch', '--json')
        events = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual([e['sequence'] for e in events], list(range(len(events))))
        self.assertTrue(all(e['version'] == 1 for e in events))
        transitions = [(e['type'], e['data']['app'], e['data']['observed_ms'])
                       for e in events if e['type'] in ('started', 'ended')]
        self.assertEqual(transitions, [('started', 'Zoom', 6_000), ('ended', 'Zoom', 28_000)])
        self.assertEqual(events[-1]['type'], 'completed')
        self.assertEqual(result.stderr, '')

    def test_other_apps_cannot_supply_a_calls_output(self):
        frame = self.frame(0, outputs=False)
        frame['clients'].append({'pid': 20, 'bundle_id': 'com.microsoft.teams', 'input': False, 'output': True})
        self.write_trace([frame])
        self.assertEqual(json.loads(self.cli('--json').stdout)['candidates'], [])

    def test_zoom_meeting_helper_corroborates_launcher_but_capture_helpers_do_not(self):
        frames = []
        for at in range(0, 56_000, 2_000):
            output = ('us.zoom.CptHost' if at < 10_000 else
                      'us.zoom.ZMScreenshot' if at < 20_000 else 'us.zoom.ZoomMeeting')
            clients = [
                {'pid': 12, 'bundle_id': 'us.zoom.xos', 'input': at < 30_000, 'output': False},
                {'pid': 20, 'bundle_id': output, 'input': False, 'output': True},
            ]
            if at == 28_000:
                # Mic ownership can move to the meeting process without a new call.
                clients[0].update(input=False, output=True)
                clients[1].update(input=True, output=False)
            frames.append({'at_ms': at, 'complete': True, 'clients': clients})
        self.write_trace(frames)
        events = [json.loads(line) for line in self.cli('--watch', '--json').stdout.splitlines()]
        transitions = [(e['type'], e['data']['app'], e['data']['observed_ms'])
                       for e in events if e['type'] in ('started', 'ended')]
        self.assertEqual(transitions, [('started', 'Zoom', 26_000), ('ended', 'Zoom', 50_000)])
        snapshots = {e['data']['observed_ms']: e['data'] for e in events if e['type'] == 'snapshot'}
        self.assertTrue(all(not s['candidates'] for at, s in snapshots.items() if at < 20_000))
        self.assertEqual(snapshots[28_000]['active'], ['Zoom'])
        self.assertEqual({c['bundle_id'] for c in snapshots[20_000]['snapshot']['clients']},
                         {'us.zoom.xos', 'us.zoom.ZoomMeeting'})
        self.assertEqual(events[-1]['type'], 'completed')

    def test_unknown_intervals_and_sleep_do_not_confirm_a_call(self):
        self.write_trace([self.frame(0), self.frame(4_000), self.frame(5_000, complete=False),
                          self.frame(6_000), self.frame(60_000)])
        events = [json.loads(line) for line in self.cli('--watch', '--json').stdout.splitlines()]
        self.assertFalse(any(e['type'] in ('started', 'ended') for e in events))

    def test_pipe_delivers_before_eof_and_ctrl_c_cancels(self):
        self.write_trace([self.frame(0)], hold=True)
        process = subprocess.Popen([str(BINARY), 'calls', '--watch', '--json'], env=self.env,
                                   cwd=self.root, stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=0)
        self.addCleanup(lambda: process.poll() is None and process.kill())
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            self.assertTrue(selector.select(3), 'first snapshot was buffered until EOF')
        self.assertEqual(json.loads(process.stdout.readline())['type'], 'snapshot')
        self.assertIsNone(process.poll())
        process.send_signal(signal.SIGINT)
        stdout, stderr = process.communicate(timeout=3)
        self.assertEqual(process.returncode, 130, stderr)
        self.assertNotIn(b'completed', stdout)

    def test_duration_is_unattended_and_invalid_values_fail_before_reading(self):
        self.write_trace([self.frame(0)], hold=True)
        began = time.monotonic()
        events = [json.loads(line) for line in self.cli('--watch', '--duration', '.08', '--json').stdout.splitlines()]
        self.assertLess(time.monotonic() - began, 2)
        self.assertEqual(events[-1]['type'], 'completed')
        self.env['FSCRIPT_CALLS_TEST_TRACE'] = '/missing/trace'
        for value in ['0', '-1', 'NaN', 'inf', '1e300', 'not-a-number']:
            result = self.cli('--watch', '--duration=' + value, success=False)
            self.assertIn('duration', result.stderr)
            self.assertNotIn('cannot read call test trace', result.stderr)
        self.cli('--duration', '1', success=False)

    def test_backwards_time_and_invalid_traces_do_not_publish_completion(self):
        self.write_trace([self.frame(2_000), self.frame(1_000)])
        result = self.cli('--watch', '--json', success=False)
        self.assertIn('backwards', result.stderr)
        self.assertNotIn('completed', result.stdout)
        self.write_trace([])
        self.assertIn('no frames', self.cli('--json', success=False).stderr)


if __name__ == '__main__':
    unittest.main()
