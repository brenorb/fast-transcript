"""Calendar CLI E2E with explicit EventKit data; never requests a real OS grant."""
import datetime
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get('FSCRIPT_TEST_BINARY', ROOT / 'target/release/fscript')).resolve()
START = '2026-11-27T14:00:00Z'
END = '2026-11-27T15:00:00Z'


def milliseconds(value):
    return int(datetime.datetime.fromisoformat(value.replace('Z', '+00:00')).timestamp() * 1000)


class CalendarCLI(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not BINARY.is_file():
            raise RuntimeError('Build fscript first, or set FSCRIPT_TEST_BINARY')

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='fscript-calendar-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.fixture = self.root / 'events.json'
        self.preferences = self.root / 'calendar.json'
        speech = self.root / 'speech.json'
        speech.write_text('{broken')
        self.env = dict(os.environ, PATH='/missing/tools', FSCRIPT_CONFIG_FILE=str(speech),
                        FSCRIPT_PYTHON_BINARY='/missing/worker', FSCRIPT_CALENDAR_CONFIG_FILE=str(self.preferences),
                        FSCRIPT_CALENDAR_TEST_JSON=str(self.fixture))
        self.write_fixture()

    def write_fixture(self, authorization='full_access', events=(), **extra):
        self.fixture.write_text(json.dumps(dict(authorization=authorization, events=list(events),
            calendars=[dict(id='work', title='Work', source='Google'), dict(id='personal', title='Personal', source='iCloud')], **extra)))

    def event(self, identity='meeting', start=START, end=END, **extra):
        event = dict(id=identity, calendar_id='work', title='Planning é ✍️', start_ms=milliseconds(start),
                     end_ms=milliseconds(end), with_others=True)
        event.update(extra)
        return event

    def cli(self, *args, success=True):
        result = subprocess.run([str(BINARY), 'calendar', *args], env=self.env, cwd=self.root,
                                text=True, capture_output=True, timeout=5)
        self.assertEqual(result.returncode == 0, success, result.stderr)
        return result

    def test_status_and_unconfigured_inspection_do_not_query_or_create_preferences(self):
        self.write_fixture('not_determined', [self.event()], request_authorization='full_access', request_delay_ms=60_000)
        before = set(self.root.iterdir())
        status = json.loads(self.cli('status', '--json').stdout)
        self.assertEqual(status['authorization'], 'not_determined')
        self.assertFalse(status['name_meetings'])
        self.assertFalse(status['queried'])
        self.assertEqual(status['events'], [])
        result = json.loads(self.cli('--json').stdout)
        self.assertEqual(result['window']['end_ms'] - result['window']['start_ms'], 86_400_000)
        self.assertFalse(result['queried'])
        self.assertIn('calendar connect', self.cli().stdout)
        self.assertEqual(set(self.root.iterdir()), before)

    def test_complete_period_inventory_retains_personal_and_recurring_events(self):
        first = self.event(all_day=True)
        second = self.event(start='2031-11-27T14:00:00Z', end='2031-11-27T15:00:00Z')
        personal = dict(self.event('focus'), with_others=False)
        self.write_fixture(events=[first, dict(first), second, personal])
        data = json.loads(self.cli('--start', '2026-01-01T00:00:00Z', '--end', '2032-01-01T00:00:00Z', '--json').stdout)
        self.assertTrue(data['queried'])
        self.assertEqual({c['id'] for c in data['calendars']}, {'work', 'personal'})
        self.assertEqual(len(data['events']), 3)
        self.assertEqual(sum(e['id'] == 'meeting' for e in data['events']), 2)
        self.assertEqual(data['window']['end_ms'], milliseconds('2032-01-01T00:00:00Z'))
        self.assertIsNone(data['matched_event'])

    def test_match_filters_nonmeetings_and_chooses_shorter_equal_overlap(self):
        long = self.event('all-hands', start='2026-11-27T09:00:00Z', end='2026-11-27T17:00:00Z')
        short = self.event('team', start='2026-11-27T14:10:00Z', end='2026-11-27T14:50:00Z')
        focus = dict(self.event('focus'), with_others=False)
        others = [self.event('cancelled', cancelled=True), self.event('declined', declined=True), self.event('all-day', all_day=True)]
        self.write_fixture(events=[long, short, focus, *others])
        data = json.loads(self.cli('match', '2026-11-27T14:15:00Z', '2026-11-27T14:45:00Z', '--json').stdout)
        self.assertEqual(data['matched_event']['id'], 'team')
        self.assertEqual(len(data['events']), 6)
        self.write_fixture(events=[focus])
        self.assertIsNone(json.loads(self.cli('match', START, END, '--json').stdout)['matched_event'])

    def test_connect_disconnect_and_revocation_keep_auth_and_preference_separate(self):
        self.write_fixture('not_determined', request_authorization='full_access')
        state = json.loads(self.cli('connect', '--json').stdout)
        self.assertTrue(state['name_meetings'])
        self.assertEqual(state['authorization'], 'full_access')
        saved = json.loads(self.preferences.read_text())
        self.assertEqual(saved, dict(version=1, name_meetings=True))
        self.write_fixture('denied', [self.event()])
        state = json.loads(self.cli('status', '--json').stdout)
        self.assertEqual(state['authorization'], 'denied')
        self.assertTrue(state['name_meetings'])
        self.assertIn('connect', self.cli('match', START, END, '--json', success=False).stderr)
        before = self.fixture.read_bytes()
        state = json.loads(self.cli('disconnect', '--json').stdout)
        self.assertFalse(state['name_meetings'])
        self.assertEqual(state['authorization'], 'denied')
        self.assertEqual(self.fixture.read_bytes(), before)

    def test_denial_does_not_write_preferences_or_repeat_a_request(self):
        marker = self.root / 'requested'
        self.env['FSCRIPT_CALENDAR_TEST_REQUEST_LOG'] = str(marker)
        self.write_fixture('denied', request_authorization='full_access', request_delay_ms=60_000)
        self.cli('connect', success=False)
        self.assertFalse(marker.exists())
        self.assertFalse(self.preferences.exists())
        self.write_fixture('not_determined', request_authorization='denied')
        self.cli('connect', success=False)
        self.assertTrue(marker.exists())
        self.assertFalse(self.preferences.exists())

    def test_invalid_period_and_preferences_fail_before_access_requests(self):
        self.env['FSCRIPT_CALENDAR_TEST_JSON'] = '/missing/calendar-data'
        for args in [('--start', START), ('--start', END, '--end', START), ('--start', '2026-11-27', '--end', END),
                     ('--start', START, '--end', END, 'connect')]:
            result = self.cli(*args, success=False)
            self.assertNotIn('cannot read calendar test data', result.stderr)
        self.preferences.write_text(json.dumps(dict(version=99, name_meetings=False)))
        self.assertIn('preference version', self.cli('connect', success=False).stderr)

    def test_blocked_permission_request_is_cancelled_without_persisting_a_grant(self):
        self.write_fixture('not_determined', request_authorization='full_access', request_delay_ms=60_000)
        marker = self.root / 'requested'
        self.env['FSCRIPT_CALENDAR_TEST_REQUEST_LOG'] = str(marker)
        process = subprocess.Popen([str(BINARY), 'calendar', 'connect', '--json'], env=self.env,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.addCleanup(lambda: process.poll() is None and process.kill())
        import time
        deadline = time.monotonic() + 3
        while not marker.exists() and time.monotonic() < deadline:
            time.sleep(.01)
        self.assertTrue(marker.exists())
        process.send_signal(signal.SIGINT)
        stdout, stderr = process.communicate(timeout=3)
        self.assertEqual(process.returncode, 130, stderr)
        self.assertEqual(stdout, b'')
        self.assertFalse(self.preferences.exists())


if __name__ == '__main__':
    unittest.main()
