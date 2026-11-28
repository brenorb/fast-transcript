"""Notarization failure properties and native archive/CLI differential acceptance."""
import contextlib
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import platform
import plistlib
import stat
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
from package_macos_app import main, notarize


class NotaryContract(unittest.TestCase):
    def test_property_only_accepted_receipts_with_ids_can_be_stapled(self):
        for index in range(128):
            receipt = dict(status=['Invalid', 'In Progress', 'Rejected', 'Accepted'][index % 4])
            if index % 4 != 3:
                receipt['id'] = f'submission-{index}'
            calls = []
            def execute(arguments, **kwargs):
                calls.append(list(map(str, arguments)))
                return SimpleNamespace(stdout=json.dumps(receipt))
            with self.assertRaises(ValueError):
                notarize(Path('/private/tmp/app é.app'), Path('/private/tmp/archive.zip'), 'profile with spaces', execute=execute)
            self.assertEqual(len(calls), 1, 'rejected/pending receipts must not reach stapler or Gatekeeper')

    def test_accepted_submission_is_stapled_validated_and_assessed_in_order(self):
        calls = []
        def execute(arguments, **kwargs):
            calls.append(list(map(str, arguments)))
            return SimpleNamespace(stdout='{"id":"accepted-id","status":"Accepted"}')
        receipt = notarize(Path('/private/tmp/app é.app'), Path('/private/tmp/archive.zip'), 'keychain profile', execute=execute)
        self.assertEqual(receipt['id'], 'accepted-id')
        self.assertEqual([call[:3] for call in calls[1:]], [
            ['xcrun', 'stapler', 'staple'], ['xcrun', 'stapler', 'validate'], ['spctl', '--assess', '--type']])
        self.assertIn('keychain profile', calls[0])
        self.assertFalse(any('store-credentials' in call for call in calls))

    def test_notarization_without_an_identity_fails_before_building_or_writing(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / 'must-not-exist'
            errors = io.StringIO()
            with patch('package_macos_app.platform.system', return_value='Darwin'), contextlib.redirect_stderr(errors):
                with self.assertRaises(SystemExit) as failure:
                    main(['--notary-profile', 'profile', '--sign-identity=-', '--output-dir', str(output)])
            self.assertEqual(failure.exception.code, 1)
            self.assertIn('Developer ID', errors.getvalue())
            self.assertFalse(output.exists())


@unittest.skipUnless(platform.system() == 'Darwin', 'Native app archive acceptance requires macOS')
class MacOSPackage(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        configured = os.environ.get('FSCRIPT_MACOS_PACKAGE_DIR')
        if not configured:
            raise unittest.SkipTest('Set FSCRIPT_MACOS_PACKAGE_DIR to the artifact directory')
        cls.artifact = Path(configured).resolve()
        cls.manifest = json.loads((cls.artifact / 'manifest.json').read_text())
        cls.temporary = tempfile.TemporaryDirectory(prefix='fscript-package-check-')
        cls.addClassCleanup(cls.temporary.cleanup)
        cls.extracted = Path(cls.temporary.name)
        name = cls.manifest['archive']['filename']
        if PurePosixPath(name).name != name:
            raise AssertionError('Archive filename must be a basename')
        cls.archive = cls.artifact / name
        subprocess.run(['ditto', '-x', '-k', str(cls.archive), str(cls.extracted)], check=True)
        cls.app = cls.extracted / 'Fast Transcript.app'
        cls.binary = cls.app / 'Contents/Helpers/fscript'

    def test_archive_crc_file_modes_checksums_and_independent_executable_bytes(self):
        expected = self.manifest['archive']['sha256']
        independent = subprocess.check_output(['shasum', '-a', '256', str(self.archive)], text=True).split()[0]
        self.assertEqual(independent, expected)
        self.assertEqual((self.artifact / (self.archive.name + '.sha256')).read_text(), f'{expected}  {self.archive.name}\n')
        with zipfile.ZipFile(self.archive) as archive:
            self.assertIsNone(archive.testzip())
            for entry in archive.infolist():
                path = PurePosixPath(entry.filename)
                self.assertFalse(path.is_absolute() or '..' in path.parts)
                self.assertIn(path.parts[0], ('Fast Transcript.app', '__MACOSX'))
            for executable in self.manifest['app']['executables']:
                entry = archive.getinfo('Fast Transcript.app/' + executable['path'])
                self.assertTrue((entry.external_attr >> 16) & stat.S_IXUSR)
                self.assertEqual(hashlib.sha256(archive.read(entry)).hexdigest(), executable['sha256'])
        subprocess.run(['codesign', '--verify', '--deep', '--strict', str(self.app)], check=True)

    def test_extracted_metadata_version_and_offline_agent_skill_match_source(self):
        expected_revision = os.environ.get('FSCRIPT_EXPECTED_SOURCE_REVISION')
        if expected_revision:
            self.assertEqual(self.manifest['source'], dict(revision=expected_revision, dirty=False),
                             'CI artifacts must identify the exact clean checkout, without masking source changes')
        info = plistlib.loads((self.app / 'Contents/Info.plist').read_bytes())
        result = subprocess.run([str(self.binary), '--version'], capture_output=True, text=True, check=True)
        self.assertEqual(result.stdout.strip(), 'fscript ' + info['CFBundleShortVersionString'])
        self.assertEqual(info['CFBundleIdentifier'], self.manifest['app']['bundle_id'])
        skill = subprocess.check_output([str(self.binary), 'skill'], text=True)
        self.assertEqual(skill, (ROOT / 'skills/fast-transcript/SKILL.md').read_text())
        help_text = subprocess.check_output([str(self.binary), '--help'], text=True)
        self.assertIn('calendar', help_text)
        self.assertIn('meeting', help_text)
        self.assertFalse((self.app / 'Contents/Helpers/fscriptctl').exists())

    def test_installed_cli_defaults_and_structured_output_match_independent_pcm_hash(self):
        from tests import test_media_range_cli as media
        fixture = media.MediaRangeCLI()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        environment = fixture.env
        subprocess.run([str(self.binary), 'models', 'set', 'ultra'], env=environment, capture_output=True, check=True)
        expected = hashlib.sha256(media.pcm(fixture.source)).hexdigest()
        completed = subprocess.run([str(self.binary), str(fixture.source)], env=environment,
                                   cwd=fixture.root, capture_output=True, text=True, timeout=20)
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIn(expected, Path(completed.stdout.strip()).read_text())
        structured = subprocess.run([str(self.binary), str(fixture.source), '--json', '--stdout', '--no-diarization'],
                                    env=environment, cwd=fixture.root, capture_output=True, text=True, timeout=20)
        self.assertEqual(structured.returncode, 0, structured.stderr)
        result = json.loads(structured.stdout)
        self.assertEqual(result['text'], expected)
        self.assertEqual(result['segments'][0]['text'], expected)
        self.assertEqual(result['audio_seconds'], 6)


if __name__ == '__main__':
    unittest.main()
