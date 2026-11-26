"""Single-command UX E2E: no-flag files/URLs, reserved filenames, and bundled skill."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import unittest
from tests import test_media_range_cli as media_fixture


class SingleCLI(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        media_fixture.MediaRangeCLI.setUpClass()

    def setUp(self):
        self.fixture = media_fixture.MediaRangeCLI()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.root = self.fixture.root
        self.env = dict(self.fixture.env, FSCRIPT_SKILL_DIR=str(self.root / 'skill'))

    def cli(self, *args, success=True):
        result = subprocess.run([str(media_fixture.BINARY), *map(str, args)], env=self.env,
                                cwd=self.root, text=True, capture_output=True, timeout=20)
        self.assertEqual(result.returncode == 0, success, result.stderr)
        return result

    def test_everyday_file_and_url_need_no_flags_or_subcommand(self):
        self.cli('models', 'set', 'ultra')
        for source in [self.fixture.source, 'https://example.test/video']:
            with self.subTest(source=source):
                result = self.cli(source)
                output = Path(result.stdout.strip())
                self.assertTrue(output.is_file())
                self.assertTrue(output.name.endswith('.speakers.txt'))
                self.assertTrue(output.read_text().strip())

    def test_reserved_filenames_use_explicit_paths_and_action_help_stays_offline(self):
        self.cli('models', 'set', 'ultra')
        for name in ['notes', 'models', 'agents', 'export', 'skill']:
            shutil.copyfile(self.fixture.source, self.root / name)
            result = self.cli('./' + name)
            self.assertTrue(Path(result.stdout.strip()).is_file())
            self.assertTrue(self.cli(name, '--help').stdout)
            self.assertTrue(self.cli('help', name).stdout)
        self.assertTrue(self.cli('--', 'notes').stdout)

    def test_skill_prints_bundled_markdown_and_sayit_style_path(self):
        result = self.cli('skill')
        self.assertTrue(result.stdout.startswith('---\nname: fast-transcript\n'))
        self.assertFalse((self.root / 'skill').exists())
        self.assertFalse(self.fixture.capture.exists())
        path = Path(self.cli('skill', 'path').stdout.strip())
        self.assertEqual(path.read_text(), result.stdout)
        metadata = json.loads(self.cli('skill', '--json').stdout)
        self.assertEqual(metadata['version'], 1)
        self.assertEqual(metadata['content'], result.stdout)
        self.assertIsNone(metadata['path'])
        self.assertFalse(self.fixture.capture.exists())
        self.assertFalse((self.root / 'config.json').exists())


if __name__ == '__main__':
    unittest.main()
