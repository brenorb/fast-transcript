"""Completed-document export E2E and differential checks against batch renderers."""
import json
import os
from pathlib import Path
import subprocess
import unittest

from tests import test_media_range_cli as media_fixture

CONTROL = Path(os.environ.get('FSCRIPT_TEST_BINARY', Path(__file__).resolve().parents[1] / 'target/release/fscript')).resolve()


class TranscriptExportCLI(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        media_fixture.MediaRangeCLI.setUpClass()

    def setUp(self):
        self.fixture = media_fixture.MediaRangeCLI()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.root = self.fixture.root
        self.document = self.fixture.run_cli(self.fixture.source)
        self.source = self.root / 'meeting.json'
        self.source.write_text(json.dumps(self.document), encoding='utf-8')

    def export(self, *args, input=None, success=True):
        # Export must not need the media tools or model worker.
        env = dict(self.fixture.env, FSCRIPT_PYTHON_BINARY='/missing/model-worker', PATH='/missing/media-tools')
        result = subprocess.run([str(CONTROL), '--quiet', 'export', *map(str, args)],
                                cwd=self.root, env=env, input=input, capture_output=True, timeout=10)
        self.assertEqual(result.returncode == 0, success, result.stderr.decode())
        return result

    def test_all_export_formats_match_batch_without_loading_a_model(self):
        formats = {'text': '--text', 'plain': '--text=plain', 'compact': '--text=compact',
                   'speakers': '--speakers', 'srt': '--srt', 'vtt': '--vtt'}
        for name, flag in formats.items():
            with self.subTest(format=name):
                batch = subprocess.run([str(media_fixture.BINARY), str(self.fixture.source),
                                        '--model', 'ultra', '--quiet', '--no-diarization', flag, '--stdout'],
                                       env=self.fixture.env, cwd=self.root, text=True, capture_output=True, check=True)
                exported = self.export(self.source, '--format', name, '--stdout')
                self.assertEqual(exported.stdout.decode(), batch.stdout)
        document = self.export(self.source, '--format', 'json', '--stdout')
        self.assertEqual(json.loads(document.stdout), self.document)
        default = self.export(self.source)
        target = self.root / 'meeting.transcript.txt'
        self.assertEqual(default.stdout.decode().strip(), str(target.resolve()))
        self.assertEqual(target.read_text().strip(), self.export(self.source, '--stdout').stdout.decode().strip())
        conventional = self.root / 'lecture.transcript.json'
        conventional.write_bytes(self.source.read_bytes())
        saved = self.export(conventional)
        self.assertEqual(saved.stdout.decode().strip(), str((self.root / 'lecture.transcript.txt').resolve()))

    def test_document_completed_event_and_jsonl_stdin_produce_identical_exports(self):
        events = [dict(version=1, job_id='j', sequence=i, type=kind, data=data)
                  for i, (kind, data) in enumerate([('started', {}), ('update', {'text': 'partial'}),
                                                   ('completed', {'result': self.document})])]
        stream = '\n'.join(map(json.dumps, events))
        for name in ['text', 'plain', 'compact', 'speakers', 'srt', 'vtt', 'json']:
            expected = self.export(self.source, '--format', name, '--stdout').stdout
            for content in [json.dumps(self.document), json.dumps(events[-1]), stream]:
                with self.subTest(format=name, content=content[:40]):
                    actual = self.export('-', '--format', name, input=content.encode()).stdout
                    self.assertEqual(actual, expected)
        target = self.root / 'subtitle.srt'
        result = self.export(self.source, target, '--format', 'srt', '--json')
        metadata = json.loads(result.stdout)
        self.assertEqual(metadata['version'], 1)
        self.assertEqual(metadata['output_path'], str(target.resolve()))
        self.assertEqual(metadata['content'] + '\n', target.read_text())

    def test_invalid_incomplete_and_source_replacement_leave_existing_files_intact(self):
        target = self.root / 'keep.txt'
        target.write_text('previous')
        for value in ['', '{broken', '{"text":"not a full result"}',
                      '{"version":1,"job_id":"j","sequence":0,"type":"started","data":{}}\n']:
            self.export('-', target, input=value.encode(), success=False)
            self.assertEqual(target.read_text(), 'previous')
        before = self.source.read_bytes()
        self.export(self.source, self.source, '--format', 'json', success=False)
        self.assertEqual(self.source.read_bytes(), before)
        link = self.root / 'source-link.json'
        link.symlink_to(self.source)
        self.export(self.source, link, success=False)
        self.assertEqual(self.source.read_bytes(), before)

    def test_explicit_markdown_export_preserves_notes_without_calling_an_agent(self):
        source = self.root / 'notes.md'
        text = '# Summary\n\nDecisions: ship Friday. 🦀\n\n'
        source.write_text(text, encoding='utf-8')
        result = self.export(source, '--format', 'markdown')
        saved = Path(result.stdout.decode().strip())
        self.assertEqual(saved, (self.root / 'notes.export.md').resolve())
        self.assertEqual(saved.read_text(), text)
        self.assertEqual(self.export(source, '--format', 'markdown', '--stdout').stdout.decode(), text)
        self.export(source, source, '--format', 'markdown', success=False)
        self.assertEqual(source.read_text(), text)


if __name__ == '__main__':
    unittest.main()
