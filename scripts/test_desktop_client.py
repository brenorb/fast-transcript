#!/usr/bin/env python3
"""Test the native process client against real fscript/FFmpeg and a fixture ASR worker."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/debug/fscript')
    args = parser.parse_args()
    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error('build fscript with cargo build first, or pass --binary')
    os.environ['FSCRIPT_TEST_BINARY'] = str(binary)
    sys.path.insert(0, str(ROOT))
    from tests.test_streaming_cli import StreamingCLI

    StreamingCLI.setUpClass()
    fixture = StreamingCLI()
    fixture.setUp()
    try:
        expected = fixture.fixture.run_cli(fixture.fixture.source, '--chunk', '2.5', '--overlap', '0.5')
        batch = fixture.root / 'batch.json'
        batch.write_text(json.dumps(expected), encoding='utf-8')
        environment = dict(fixture.env, FSCRIPT_DESKTOP_TEST_CLI=str(binary),
                           FSCRIPT_DESKTOP_TEST_SOURCE=str(fixture.fixture.source),
                           FSCRIPT_DESKTOP_TEST_BATCH=str(batch))
        subprocess.run(['swift', 'test', '--package-path', str(ROOT / 'desktop'),
                        '--scratch-path', str(ROOT / 'target/desktop-build')],
                       env=environment, check=True)
    finally:
        fixture.doCleanups()


if __name__ == '__main__':
    main()
