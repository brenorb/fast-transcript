"""Automatic meetings E2E: actual service/storage with explicit call/audio/ASR fixtures."""
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time
import unittest

from tests import test_media_range_cli as media
from tests import test_meeting_cli as meeting


class MeetingWatchCLI(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        meeting.MeetingCLI.setUpClass()

    def setUp(self):
        self.meeting = meeting.MeetingCLI()
        self.meeting.setUp()
        self.addCleanup(self.meeting.doCleanups)
        self.root, self.env = self.meeting.root, self.meeting.env
        self.trace = self.root / 'calls.json'
        self.env['FSCRIPT_CALLS_TEST_TRACE'] = str(self.trace)

    def frame(self, at, apps=('us.zoom.xos',), complete=True):
        return dict(at_ms=at, complete=complete,
                    clients=[dict(pid=i + 100, bundle_id=app, input=True, output=True) for i, app in enumerate(apps)])

    def write_trace(self, frames, interval=40, hold=False):
        self.trace.write_text(json.dumps(dict(frames=frames, interval_ms=interval, hold=hold)))

    def command(self, *args):
        return [str(media.BINARY), 'meeting', 'watch', '--json', '--model', 'ultra',
                '--output-dir', str(self.root / 'meetings'), *args]

    def spawn(self, *args):
        process = subprocess.Popen(self.command(*args), env=self.env, cwd=self.root,
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=0)
        self.addCleanup(meeting.MeetingCLI.stop, process)
        return process

    def request(self, process, action, **values):
        process.stdin.write((json.dumps(dict(action=action, **values)) + '\n').encode())
        process.stdin.flush()

    def event(self, process, timeout=8):
        return self.meeting.event(process, timeout)

    def through(self, process, kind):
        events = []
        while True:
            event = self.event(process)
            events.append(event)
            if event['type'] == kind:
                return events

    def collect(self, process, prefix=(), status=0):
        stdout, stderr = process.communicate(timeout=15)
        self.assertEqual(process.returncode, status, stderr.decode())
        events = [*prefix, *(json.loads(line) for line in stdout.splitlines())]
        self.assertEqual([e['sequence'] for e in events], list(range(len(events))))
        self.assertTrue(all(e['version'] == 1 for e in events))
        return events

    def test_complete_call_records_once_and_rearms_for_the_next_call(self):
        frames = [self.frame(t, ('us.zoom.xos',) if t <= 8000 or 34000 <= t <= 42000 else ())
                  for t in range(0, 66000, 2000)]
        self.write_trace(frames)
        events = self.collect(self.spawn())
        self.assertEqual(events[-1]['type'], 'watch_completed')
        self.assertEqual([e['data']['episode'] for e in events if e['type'] == 'accepted'], [1, 2])
        completed = [e['data']['event'] for e in events if e['type'] == 'meeting' and e['data']['event']['type'] == 'completed']
        self.assertEqual(len(completed), 2)
        for event in completed:
            result = event['data']['result']
            self.assertEqual(result['meeting']['state'], 'stopped')
            self.assertEqual(result['meeting']['track_frames'][0], result['meeting']['track_frames'][1])
            self.assertGreater(result['audio_seconds'], .1)
            output = Path(event['data']['output_path'])
            self.assertTrue(output.is_file())
            for segment in result['segments']:
                name = 'microphone.pcm' if segment['speaker'] == 'Microphone' else 'system.pcm'
                pcm = (output.parent / name).read_bytes()
                start, end = round(segment['start_s'] * 16000), round(segment['end_s'] * 16000)
                self.assertEqual(segment['text'], hashlib.sha256(pcm[start * 2:end * 2]).hexdigest())
        for episode in (1, 2):
            nested = [e['data']['event'] for e in events if e['type'] == 'meeting' and e['data']['episode'] == episode]
            self.assertEqual([e['sequence'] for e in nested], list(range(len(nested))))

    def test_overlapping_apps_share_one_recording_and_unknown_evidence_does_not_stop_it(self):
        frames = []
        for t in range(0, 64000, 2000):
            apps = tuple(app for app, present in [('us.zoom.xos', t < 16000), ('com.microsoft.teams2', 4000 <= t < 40000)] if present)
            frames.append(self.frame(t, apps, complete=not 18000 <= t < 24000))
        self.write_trace(frames)
        events = self.collect(self.spawn())
        self.assertEqual(len([e for e in events if e['type'] == 'accepted']), 1)
        self.assertEqual(len([e for e in events if e['type'] == 'session_exit']), 1)
        ended = next(e for e in events if e['type'] == 'call_status' and e['data']['observed_ms'] == 58000)
        self.assertTrue(ended['data']['active'])

    def test_control_waits_for_admission_and_manual_stop_suppresses_the_same_call(self):
        self.write_trace([self.frame(t) for t in range(0, 100000, 2000)], hold=True)
        process = self.spawn('--control')
        events = self.through(process, 'offer')
        self.assertFalse((self.root / 'workers').exists(), 'offers must not load a model or capture')
        episode = events[-1]['data']['episode']
        self.request(process, 'accept', episode=episode)
        while True:
            event = self.event(process)
            events.append(event)
            if event['type'] == 'meeting' and event['data']['event']['type'] == 'recording': break
        time.sleep(.15)
        self.request(process, 'stop', episode=episode)
        events += self.through(process, 'session_exit')
        for _ in range(4):
            event = self.event(process)
            self.assertNotEqual(event['type'], 'offer')
            events.append(event)
        self.request(process, 'shutdown')
        events = self.collect(process, events)
        self.assertEqual(len([e for e in events if e['type'] == 'accepted']), 1)

    def test_skipped_and_stale_offers_cannot_start_capture(self):
        self.write_trace([self.frame(t) for t in range(0, 18000, 2000)], hold=True)
        process = self.spawn('--control')
        events = self.through(process, 'offer')
        episode = events[-1]['data']['episode']
        self.request(process, 'skip', episode=episode)
        self.request(process, 'accept', episode=episode)
        events += self.through(process, 'offer_expired')
        self.request(process, 'shutdown')
        events = self.collect(process, events)
        self.assertFalse(any(e['type'] == 'meeting' for e in events))
        self.assertFalse((self.root / 'workers').exists())

    def test_watch_lock_is_exclusive_and_cancel_cleans_a_confirmed_blocked_model(self):
        self.write_trace([self.frame(t) for t in range(0, 100000, 2000)], hold=True)
        self.env['MEETING_INFERENCE_DELAY'] = '60'
        process = self.spawn()
        events = self.through(process, 'accepted')
        second = subprocess.run(self.command('--duration', '.1'), env=self.env, capture_output=True, timeout=3)
        self.assertNotEqual(second.returncode, 0)
        self.assertIn(b'already owns', second.stderr)
        deadline = time.monotonic() + 6
        while not (self.root / 'requests').exists() and time.monotonic() < deadline: time.sleep(.02)
        self.assertTrue((self.root / 'requests').exists(), 'the worker must actually be blocked before cancellation')
        pids = [int(pid) for pid in (self.root / 'workers').read_text().splitlines()]
        process.send_signal(signal.SIGINT)
        self.collect(process, events, status=130)
        for pid in pids:
            with self.assertRaises(ProcessLookupError): os.kill(pid, 0)

    def test_preferences_are_passive_and_trace_capture_requires_both_fixtures(self):
        self.env['FSCRIPT_CONFIG_FILE'] = str(self.root / 'broken-model.json')
        Path(self.env['FSCRIPT_CONFIG_FILE']).write_text('{broken')
        for action, enabled in [('status', False), ('enable', True), ('disable', False)]:
            result = subprocess.run([str(media.BINARY), 'meeting', 'auto', action, '--json'], env=self.env, capture_output=True, timeout=3)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout)['enabled'], enabled)
            self.assertFalse((self.root / 'workers').exists())
        self.env.pop('FSCRIPT_SYSTEM_CAPTURE_TEST_WAV')
        self.write_trace([self.frame(0)])
        result = subprocess.run(self.command(), env=self.env, capture_output=True, timeout=3)
        self.assertIn(b'both explicit capture WAV fixtures', result.stderr)

    def test_existing_manual_capture_covers_the_call_and_is_not_stopped(self):
        self.write_trace([self.frame(t) for t in range(0, 30000, 2000)], hold=True)
        manual = self.meeting.spawn('--duration', '2')
        prefix = self.meeting.through(manual, 'recording')
        events = self.collect(self.spawn('--duration', '.5'))
        self.assertTrue(any(e['type'] == 'covered' for e in events))
        self.assertFalse(any(e['type'] == 'accepted' for e in events))
        self.assertIsNone(manual.poll(), 'automatic watching must not stop a manual recording')
        result = self.meeting.finish(manual, prefix)[-1]['data']['result']
        self.assertGreater(result['audio_seconds'], 1.9)

    def test_plain_output_is_a_path_and_quiet_reaches_the_meeting_child(self):
        self.write_trace([self.frame(t, ('us.zoom.xos',) if t < 12000 else ()) for t in range(0, 34000, 2000)])
        command = [arg for arg in self.command('--quiet') if arg != '--json']
        result = subprocess.run(command, env=self.env, cwd=self.root, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, b'')
        self.assertTrue(Path(result.stdout.decode().strip()).is_file())


if __name__ == '__main__':
    unittest.main()
