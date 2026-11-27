"""Meeting E2E: real converters/storage, independent channel/ASR checks, recovery.

Only native audio arrival and the model worker are explicit fixtures. No device
permission is requested. Run with a fresh compiled FSCRIPT_TEST_BINARY.
"""
import hashlib
import json
import os
from pathlib import Path
import selectors
import signal
import struct
import subprocess
import time
import unittest
import wave

from tests import test_media_range_cli as fixture


class MeetingCLI(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        fixture.MediaRangeCLI.setUpClass()

    def setUp(self):
        self.fixture = fixture.MediaRangeCLI()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.root, self.env = self.fixture.root, self.fixture.env
        self.remote = self.root / 'remote.wav'
        with wave.open(str(self.remote), 'wb') as audio:
            audio.setparams((2, 2, 48000, 0, 'NONE', 'not compressed'))
            audio.writeframes(b''.join(struct.pack('<hh', (i * 17) % 20001 - 10000, (i * 23) % 10001) for i in range(48000 * 10)))
        self.env.update(FSCRIPT_CAPTURE_TEST_WAV=str(self.fixture.source), FSCRIPT_SYSTEM_CAPTURE_TEST_WAV=str(self.remote))
        self.output = self.root / 'meetings'
        worker = fixture.WORKER.replace('import hashlib, json, os, shutil, sys, wave', 'import hashlib, json, os, shutil, sys, wave, time\nwith open(os.environ["MEETING_WORKERS"], "a") as log: log.write(str(os.getpid()) + "\\n")')
        worker = worker.replace('    path = json.loads(line)["audio_path"]', '    with open(os.environ["MEETING_REQUEST_LOG"], "a") as log: log.write("request\\n")\n    time.sleep(float(os.environ.get("MEETING_INFERENCE_DELAY", "0")))\n    if os.environ.get("MEETING_INFERENCE_FAIL"): print(json.dumps({"error":"fixture recognition failure"}), flush=True); continue\n    path = json.loads(line)["audio_path"]')
        (self.root / 'worker').write_text(worker)
        self.env['MEETING_WORKERS'] = str(self.root / 'workers')
        self.env['MEETING_REQUEST_LOG'] = str(self.root / 'requests')

    def command(self, *args):
        return [str(fixture.BINARY), 'meeting', '--json', '--model', 'ultra', '--output-dir', str(self.output), *map(str, args)]

    def spawn(self, *args):
        process = subprocess.Popen(self.command(*args), stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=self.env, cwd=self.root, bufsize=0)
        self.addCleanup(self.stop, process)
        return process

    @staticmethod
    def stop(process):
        if process.poll() is None:
            process.terminate()
            try: process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.kill(); process.wait(timeout=3)
        for stream in (process.stdin, process.stdout, process.stderr):
            if stream: stream.close()

    def event(self, process, timeout=8):
        deadline, line = time.monotonic() + timeout, bytearray()
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            while time.monotonic() < deadline:
                if not selector.select(max(0, deadline - time.monotonic())): break
                byte = process.stdout.read(1)
                if not byte: self.fail('meeting ended before event: ' + process.stderr.read().decode())
                if byte == b'\n': return json.loads(line)
                line.extend(byte)
        self.fail(f'no meeting event within {timeout}s: {line[:100]!r}')

    def through(self, process, kind):
        events = []
        while True:
            event = self.event(process)
            events.append(event)
            if event['type'] == kind: return events

    def finish(self, process, prefix=(), status=0):
        stdout, stderr = process.communicate(timeout=20)
        self.assertEqual(process.returncode, status, stderr.decode())
        events = [*prefix, *(json.loads(line) for line in stdout.splitlines())]
        self.assertEqual([event['sequence'] for event in events], list(range(len(events))))
        self.assertTrue(all(event['version'] == 1 for event in events))
        return events

    def inspect(self, directory, action='status', success=True):
        result = subprocess.run([str(fixture.BINARY), 'meeting', action, str(directory), '--json'], capture_output=True, env=self.env, timeout=5)
        self.assertEqual(result.returncode == 0, success, result.stderr.decode())
        return json.loads(result.stdout) if success else result

    def test_live_updates_precede_stop_and_final_chunks_match_saved_audio(self):
        process = self.spawn('--duration', '5')
        prefix = self.through(process, 'update')
        self.assertIsNone(process.poll())
        directory = Path(prefix[1]['data']['directory'])
        self.assertTrue((directory / 'transcript.partial.json').is_file())
        self.assertFalse((directory / 'transcript.json').exists())
        self.assertEqual(self.inspect(directory)['state'], 'recording')
        self.inspect(directory, 'recover', success=False)
        events = self.finish(process, prefix)
        result = events[-1]['data']['result']
        self.assertEqual(events[-1]['type'], 'completed')
        self.assertEqual(result['chunk_seconds'], 30)
        self.assertEqual(result['chunk_overlap_seconds'], 2)
        self.assertGreater(result['preview_transcribe_seconds'], 0)
        self.assertAlmostEqual(result['transcribe_seconds'], sum(chunk['transcribe_seconds'] for chunk in result['chunks']) + result['preview_transcribe_seconds'])
        self.assertNotEqual(result['text'], prefix[-1]['data']['text'], 'final recognition replaces provisional text')
        self.assertEqual(json.loads((directory / 'transcript.json').read_text()), result)
        manifest = self.inspect(directory)
        self.assertEqual(manifest['state'], 'stopped')
        self.assertEqual(result['meeting'], manifest)
        self.assertEqual(manifest['track_frames'][0], manifest['track_frames'][1])
        self.assertEqual(len((self.root / 'workers').read_text().splitlines()), 1, 'one warm model handles both tracks')
        labels = {'Microphone': 'microphone.pcm', 'System': 'system.pcm'}
        self.assertEqual({s['speaker'] for s in result['segments']}, set(labels))
        for segment in result['segments']:
            audio = (directory / labels[segment['speaker']]).read_bytes()
            start, end = round(segment['start_s'] * 16000), round(segment['end_s'] * 16000)
            self.assertEqual(segment['text'], hashlib.sha256(audio[start * 2:end * 2]).hexdigest())
        self.assertNotEqual((directory / 'microphone.pcm').read_bytes(), (directory / 'system.pcm').read_bytes())
        exported = subprocess.run([str(fixture.BINARY), 'export', str(directory / 'transcript.json'), '--stdout', '--format', 'speakers'], capture_output=True, env=self.env, timeout=5)
        self.assertEqual(exported.returncode, 0, exported.stderr.decode())
        self.assertIn(b'Microphone', exported.stdout)
        self.assertIn(b'System', exported.stdout)

    def test_duration_and_disk_progress_do_not_wait_for_slow_inference(self):
        self.env['MEETING_INFERENCE_DELAY'] = '4'
        process = self.spawn('--duration', '3')
        prefix = self.through(process, 'recording')
        directory = Path(prefix[1]['data']['directory'])
        time.sleep(3.5)
        self.assertIsNone(process.poll(), 'model is still transcribing')
        sizes = [(directory / name).stat().st_size for name in ('microphone.pcm', 'system.pcm')]
        self.assertTrue(all(2.95 * 32000 < size < 3.15 * 32000 for size in sizes), sizes)
        time.sleep(.3)
        self.assertEqual(sizes, [(directory / name).stat().st_size for name in ('microphone.pcm', 'system.pcm')], 'capture stopped independently of inference')
        events = self.finish(process, prefix)
        self.assertEqual(events[-1]['type'], 'completed')

    def test_recognition_failure_continues_saving_until_requested_duration(self):
        self.env['MEETING_INFERENCE_FAIL'] = '1'
        process = self.spawn('--duration', '3.5')
        prefix = self.through(process, 'recognition_error')
        directory = Path(prefix[1]['data']['directory'])
        self.assertIsNone(process.poll())
        events = self.finish(process, prefix, status=1)
        self.assertEqual(events[-1]['type'], 'error')
        manifest = self.inspect(directory)
        self.assertEqual(manifest['state'], 'stopped')
        self.assertTrue(all(frames > 3.4 * 16000 for frames in manifest['track_frames']))
        self.assertTrue(any(event['type'] == 'saved' for event in events))

    def test_control_start_stop_and_closed_stdin_after_stop_complete(self):
        process = self.spawn('--control')
        prefix = [self.event(process)]
        self.assertEqual(prefix[0]['type'], 'ready')
        self.assertFalse(self.output.exists())
        process.stdin.write(b'{"action":"start"}\n'); process.stdin.flush()
        prefix += self.through(process, 'recording')
        time.sleep(.3)
        process.stdin.write(b'{"action":"stop"}\n'); process.stdin.flush()
        events = self.finish(process, prefix)
        self.assertEqual(events[-1]['type'], 'completed')
        self.assertLess(events[-1]['data']['result']['audio_seconds'], 1)

    def test_controller_crash_retains_checkpointed_and_later_audio_for_recovery(self):
        process = self.spawn('--control')
        prefix = [self.event(process)]
        process.stdin.write(b'{"action":"start"}\n'); process.stdin.flush()
        prefix += self.through(process, 'recording')
        directory = Path(prefix[1]['data']['directory'])
        time.sleep(1.4)
        process.stdin.close(); process.stdin = None
        events = self.finish(process, prefix, status=1)
        self.assertEqual(events[-1]['type'], 'error')
        recovered = self.inspect(directory, 'recover')
        self.assertEqual(recovered['state'], 'interrupted')
        self.assertTrue(all(count > 16000 for count in recovered['track_frames']))
        self.assertEqual(self.inspect(directory, 'recover'), recovered)

    def test_cancel_after_audio_saved_interrupts_a_blocked_final_inference(self):
        self.env['MEETING_INFERENCE_DELAY'] = '60'
        process = self.spawn('--control')
        prefix = [self.event(process)]
        process.stdin.write(b'{"action":"start"}\n'); process.stdin.flush()
        prefix += self.through(process, 'recording')
        time.sleep(.2)
        process.stdin.write(b'{"action":"stop"}\n'); process.stdin.flush()
        prefix += self.through(process, 'saved')
        directory = Path(prefix[1]['data']['directory'])
        if not any(event['type'] == 'model_ready' for event in prefix):
            prefix += self.through(process, 'model_ready')
        deadline = time.monotonic() + 5
        while not (self.root / 'requests').exists() and time.monotonic() < deadline:
            time.sleep(.01)
        self.assertTrue((self.root / 'requests').exists(), 'the final inference request must actually have started')
        self.inspect(directory, 'recover', success=False)
        began = time.monotonic()
        process.stdin.write(b'{"action":"cancel"}\n'); process.stdin.flush()
        events = self.finish(process, prefix)
        self.assertLess(time.monotonic() - began, 3)
        self.assertEqual(events[-1]['type'], 'cancelled')
        self.assertTrue(events[-1]['data']['audio_stopped'])
        self.assertFalse(events[-1]['data']['recoverable'])
        self.assertEqual(self.inspect(directory)['state'], 'stopped')
        self.assertFalse((directory / 'transcript.json').exists())
        for pid in (self.root / 'workers').read_text().splitlines():
            with self.assertRaises(ProcessLookupError): os.kill(int(pid), 0)

    def test_meeting_normalization_does_not_launch_an_external_converter(self):
        converter = self.root / 'ffmpeg'
        marker = self.root / 'unexpected-converter'
        converter.write_text('#!/usr/bin/env python3\nfrom pathlib import Path\nPath(' + repr(str(marker)) + ').touch()\nraise SystemExit(2)\n')
        converter.chmod(0o755)
        process = self.spawn('--duration', '.3')
        events = self.finish(process)
        self.assertEqual(events[-1]['type'], 'completed')
        self.assertFalse(marker.exists(), 'meeting normalization must remain inside the Rust capture service')
        self.assertFalse(events[-1]['data']['result']['used_ffmpeg_normalization'])
        directory = Path(events[1]['data']['directory'])
        self.assertEqual(self.inspect(directory)['state'], 'stopped')

    def test_partial_file_is_kept_separate_when_the_final_decode_fails(self):
        worker = (self.root / 'worker').read_text().replace('        text = hashlib.sha256', '        if seconds > 2.5: print(json.dumps({"error":"final inference failed"}), flush=True); continue\n        text = hashlib.sha256')
        (self.root / 'worker').write_text(worker)
        process = self.spawn('--control')
        prefix = [self.event(process)]
        process.stdin.write(b'{"action":"start"}\n'); process.stdin.flush()
        prefix += self.through(process, 'update')
        time.sleep(1)
        process.stdin.write(b'{"action":"stop"}\n'); process.stdin.flush()
        events = self.finish(process, prefix, status=1)
        directory = Path(events[1]['data']['directory'])
        self.assertEqual(self.inspect(directory)['state'], 'stopped')
        self.assertTrue((directory / 'transcript.partial.json').is_file())
        self.assertFalse((directory / 'transcript.json').exists())
        self.assertFalse(any(event['type'] == 'completed' for event in events))
        self.assertTrue(all(not event['data']['recording_continues'] for event in events if event['type'] == 'recognition_error'))

    def test_plain_output_returns_the_final_path_and_keeps_preview_on_stderr(self):
        for quiet in [False, True]:
            command = [str(fixture.BINARY), 'meeting', '--duration', '2.4', '--model', 'ultra', '--output-dir', str(self.output / str(quiet))]
            if quiet: command.append('--quiet')
            result = subprocess.run(command, capture_output=True, env=self.env, cwd=self.root, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr.decode())
            path = Path(result.stdout.decode().strip())
            self.assertEqual(path.name, 'transcript.json')
            self.assertTrue(path.is_file())
            final = json.loads(path.read_text())
            self.assertEqual(final['chunk_seconds'], 30)
            self.assertEqual(final['chunk_overlap_seconds'], 2)
            self.assertEqual(result.stdout.count(b'\n'), 1)
            if quiet: self.assertEqual(result.stderr, b'')
            else: self.assertIn(b'Microphone:', result.stderr)

    def test_missing_source_is_interrupted_and_never_fabricated_as_silence(self):
        self.env['FSCRIPT_SYSTEM_CAPTURE_TEST_WAV'] = str(self.root / 'missing.wav')
        process = self.spawn('--duration', '1')
        events = self.finish(process, status=1)
        directory = Path(events[1]['data']['directory'])
        self.assertEqual(self.inspect(directory)['state'], 'recording')
        manifest = self.inspect(directory, 'recover')
        self.assertEqual(manifest['state'], 'interrupted')
        self.assertEqual(manifest['track_frames'][1], 0)

    def test_valid_empty_source_never_becomes_a_recording_of_fabricated_silence(self):
        empty = self.root / 'empty.wav'
        with wave.open(str(empty), 'wb') as audio:
            audio.setparams((1, 2, 16000, 0, 'NONE', 'not compressed'))
        self.env['FSCRIPT_SYSTEM_CAPTURE_TEST_WAV'] = str(empty)
        process = self.spawn('--duration', '1.2')
        events = self.finish(process, status=1)
        directory = Path(events[1]['data']['directory'])
        manifest = self.inspect(directory, 'recover')
        self.assertEqual(manifest['state'], 'interrupted')
        self.assertEqual(manifest['track_frames'][1], 0)

    def test_completion_is_one_commit_point_even_when_cancel_arrives_at_completion(self):
        for _ in range(5):
            process = self.spawn('--control')
            prefix = [self.event(process)]
            process.stdin.write(b'{"action":"start"}\n'); process.stdin.flush()
            prefix += self.through(process, 'recording')
            time.sleep(.1)
            process.stdin.write(b'{"action":"stop"}\n'); process.stdin.flush()
            prefix += self.through(process, 'completed')
            try:
                process.stdin.write(b'{"action":"cancel"}\n'); process.stdin.flush()
            except BrokenPipeError: pass
            events = self.finish(process, prefix)
            self.assertEqual([e['type'] for e in events if e['type'] in ('completed', 'cancelled')], ['completed'])
            directory = Path(prefix[1]['data']['directory'])
            self.assertTrue((directory / 'transcript.json').is_file())
            self.assertFalse((directory / '.transcript.pending.json').exists())

    def test_recording_state_does_not_wait_for_model_loading(self):
        worker = (self.root / 'worker').read_text().replace('print(json.dumps(dict(ready=True', 'time.sleep(1)\nprint(json.dumps(dict(ready=True')
        (self.root / 'worker').write_text(worker)
        began = time.monotonic()
        process = self.spawn('--duration', '.3')
        prefix = self.through(process, 'recording')
        self.assertLess(time.monotonic() - began, 1)
        self.assertFalse(any(event['type'] == 'model_ready' for event in prefix))
        events = self.finish(process, prefix)
        self.assertEqual(events[-1]['type'], 'completed')

    def test_model_startup_failure_preserves_healthy_capture(self):
        (self.root / 'worker').write_text('#!/usr/bin/env python3\nraise SystemExit(2)\n')
        process = self.spawn('--duration', '.8')
        events = self.finish(process, status=1)
        directory = Path(events[1]['data']['directory'])
        self.assertTrue(any(event['type'] == 'recognition_error' for event in events))
        manifest = self.inspect(directory)
        self.assertEqual(manifest['state'], 'stopped')
        self.assertTrue(all(frames > .7 * 16000 for frames in manifest['track_frames']))
        self.assertFalse((directory / 'transcript.json').exists())

    def saved_fixture(self, frames=(16000 * 59 + 123, 16000 * 31 + 17), state='recording'):
        directory = self.root / 'saved meeting é'
        directory.mkdir(mode=0o700)
        (directory / '.lock').touch(mode=0o600)
        for count, name in zip(frames, ('microphone.pcm', 'system.pcm')):
            (directory / name).write_bytes(b''.join(struct.pack('<h', (i * 137) % 60001 - 30000) for i in range(count)))
        manifest = dict(version=1, sample_rate=16000, channels=1, sample_format='s16le', started_at_unix_ms=1234567890, state=state, track_frames=list(frames) if state != 'recording' else [0, 0])
        (directory / 'meeting.json').write_text(json.dumps(manifest))
        # Replay must never attempt either capture endpoint or an audio converter.
        self.env['FSCRIPT_CAPTURE_TEST_WAV'] = str(self.root / 'missing-mic.wav')
        self.env['FSCRIPT_SYSTEM_CAPTURE_TEST_WAV'] = str(self.root / 'missing-system.wav')
        return directory

    def replay(self, directory, *args):
        process = subprocess.Popen([str(fixture.BINARY), 'meeting', 'transcribe', str(directory), '--json', '--model', 'ultra', *args], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=self.env, cwd=self.root, bufsize=0)
        self.addCleanup(self.stop, process)
        return process

    def test_saved_transcription_repairs_audio_and_matches_independent_batch_windows(self):
        directory = self.saved_fixture()
        original = {name: (directory / name).read_bytes() for name in ('microphone.pcm', 'system.pcm')}
        # An incomplete crash write is repaired without losing a complete sample.
        with (directory / 'system.pcm').open('ab') as output: output.write(b'x')
        events = self.finish(self.replay(directory))
        self.assertEqual(events[-1]['type'], 'completed')
        self.assertEqual(events[2]['type'], 'restored')
        result = events[-1]['data']['result']
        self.assertEqual(result['meeting']['state'], 'interrupted')
        self.assertEqual(result['meeting'], self.inspect(directory))
        self.assertEqual(result['preview_transcribe_seconds'], 0)
        self.assertFalse(any(e['type'] in ('recording', 'saved', 'update') for e in events))
        self.assertEqual(len((self.root / 'workers').read_text().splitlines()), 1)
        for label, name in [('Microphone', 'microphone.pcm'), ('System', 'system.pcm')]:
            self.assertEqual((directory / name).read_bytes(), original[name])
            wav = self.root / (name + '.wav')
            with wave.open(str(wav), 'wb') as output:
                output.setparams((1, 2, 16000, 0, 'NONE', 'not compressed'))
                output.writeframes(original[name])
            batch = self.fixture.run_cli(wav, '--chunk', '30', '--overlap', '2')
            segments = [{k: v for k, v in segment.items() if k != 'speaker'} for segment in result['segments'] if segment['speaker'] == label]
            self.assertEqual(segments, batch['segments'])
        self.assertEqual(json.loads((directory / 'transcript.json').read_text()), result)

    def test_saved_transcription_has_exclusive_ownership_and_cancel_keeps_previous_result(self):
        directory = self.saved_fixture(frames=(16000, 8000), state='stopped')
        previous = b'previous transcript remains byte-for-byte'
        (directory / 'transcript.json').write_bytes(previous)
        self.env['MEETING_INFERENCE_DELAY'] = '60'
        process = self.replay(directory)
        prefix = self.through(process, 'model_ready')
        self.inspect(directory, 'recover', success=False)
        conflict = self.replay(directory)
        self.finish(conflict, status=1)
        self.assertEqual(len((self.root / 'workers').read_text().splitlines()), 1)
        process.send_signal(signal.SIGINT)
        process.communicate(timeout=5)
        self.assertEqual(process.returncode, 130)
        self.assertEqual((directory / 'transcript.json').read_bytes(), previous)
        self.assertEqual(self.inspect(directory, 'recover')['state'], 'stopped')
        for pid in (self.root / 'workers').read_text().splitlines():
            with self.assertRaises(ProcessLookupError): os.kill(int(pid), 0)

    def test_failed_saved_transcription_retains_audio_and_previous_result_then_can_retry(self):
        directory = self.saved_fixture(frames=(16000, 0), state='interrupted')
        previous = b'previous transcript'
        (directory / 'transcript.json').write_bytes(previous)
        audio = (directory / 'microphone.pcm').read_bytes()
        self.env['MEETING_INFERENCE_FAIL'] = '1'
        events = self.finish(self.replay(directory), status=1)
        self.assertEqual(events[-1]['type'], 'error')
        self.assertFalse(any(e['data']['recording_continues'] for e in events if e['type'] == 'recognition_error'))
        self.assertEqual((directory / 'transcript.json').read_bytes(), previous)
        self.assertEqual((directory / 'microphone.pcm').read_bytes(), audio)
        del self.env['MEETING_INFERENCE_FAIL']
        command = [str(fixture.BINARY), 'meeting', 'transcribe', str(directory), '--model', 'ultra', '--quiet']
        result = subprocess.run(command, capture_output=True, env=self.env, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertEqual(result.stderr, b'')
        self.assertEqual(result.stdout.decode(), str(directory / 'transcript.json') + '\n')
        final = json.loads((directory / 'transcript.json').read_text())
        self.assertEqual(final['meeting']['state'], 'interrupted')
        self.assertEqual({s['speaker'] for s in final['segments']}, {'Microphone'})

    def test_saved_audio_truncated_during_model_loading_fails_instead_of_waiting(self):
        directory = self.saved_fixture(frames=(16000, 0), state='stopped')
        worker = (self.root / 'worker').read_text().replace('print(json.dumps(dict(ready=True', 'time.sleep(1)\nprint(json.dumps(dict(ready=True')
        (self.root / 'worker').write_text(worker)
        process = self.replay(directory)
        prefix = self.through(process, 'restored')
        (directory / 'microphone.pcm').write_bytes(b'')
        events = self.finish(process, prefix, status=1)
        self.assertEqual(events[-1]['type'], 'error')
        self.assertIn('truncated', str(events))
        self.assertFalse((directory / 'transcript.json').exists())

    def test_saved_transcription_preflight_and_help_never_load_models(self):
        directory = self.saved_fixture(frames=(16000, 0), state='stopped')
        (directory / 'microphone.pcm').write_bytes(b'broken')
        self.finish(self.replay(directory), status=1)
        result = subprocess.run([str(fixture.BINARY), 'meeting', 'transcribe', '--help'], capture_output=True, env=self.env, timeout=5)
        self.assertEqual(result.returncode, 0)
        self.assertFalse((self.root / 'workers').exists())
        # A FIFO is invalid storage, not input to wait for indefinitely.
        (directory / 'microphone.pcm').unlink()
        os.mkfifo(directory / 'microphone.pcm')
        self.finish(self.replay(directory), status=1)
        for option in ('--duration', '--control'):
            args = [str(fixture.BINARY), 'meeting', option]
            if option == '--duration': args.append('60')
            args += ['transcribe', str(directory)]
            result = subprocess.run(args, capture_output=True, env=self.env, timeout=5)
            self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / 'workers').exists())

    def test_help_status_and_invalid_duration_have_no_capture_side_effects(self):
        for args in [('--help',), ('--duration', '0'), ('--duration', 'NaN'), ('--duration', '-1'), ()]:
            result = subprocess.run(self.command(*args), stdin=subprocess.DEVNULL, capture_output=True, env=self.env, timeout=5)
            self.assertEqual(result.returncode == 0, args == ('--help',), result.stderr.decode())
            self.assertFalse(self.output.exists())
            self.assertFalse((self.root / 'workers').exists())

    @unittest.skipUnless(os.name == 'posix', 'POSIX signal supervision')
    def test_first_interrupt_finishes_and_second_interrupt_cancels_owned_workers(self):
        process = self.spawn('--duration', '30')
        prefix = self.through(process, 'recording')
        time.sleep(.2)
        process.send_signal(signal.SIGINT)
        events = self.finish(process, prefix)
        self.assertEqual(events[-1]['type'], 'completed')
        self.assertLess(events[-1]['data']['result']['audio_seconds'], 1)
        self.env['MEETING_INFERENCE_DELAY'] = '60'
        process = self.spawn('--duration', '30')
        self.through(process, 'recording')
        time.sleep(2.3)
        process.send_signal(signal.SIGINT)
        time.sleep(.1)
        process.send_signal(signal.SIGINT)
        process.communicate(timeout=5)
        self.assertEqual(process.returncode, 130)
