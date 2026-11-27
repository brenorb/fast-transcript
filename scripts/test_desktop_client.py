#!/usr/bin/env python3
"""Test the native process client against real fscript/FFmpeg and a fixture ASR worker."""
import argparse
import json
import os
import platform
from pathlib import Path
import shutil
import subprocess
import sys
import wave

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
        # Compile the opt-in GUI acceptance fixtures without launching or granting them access.
        for name in ('PasteTarget', 'NativePasteCheck'):
            subprocess.run(['swiftc', str(ROOT / 'tests/fixtures' / (name + '.swift')),
                            '-target', platform.machine() + '-apple-macosx14.0',
                            '-o', str(fixture.root / name)], check=True)
        # Invalid acceptance specs fail before checking a grant or posting keys.
        for positive, error in [(None, 'at least one asserted positive'), (True, 'receiver is not ready')]:
            specification = fixture.root / 'invalid-paste-spec.json'
            report = fixture.root / 'invalid-paste-report.json'
            specification.write_text(json.dumps(dict(targetPid=1, observations=str(fixture.root / 'missing-receiver'),
                                                     report=str(report), cases=[dict(name='validation', binary='/missing/fscript',
                                                                                   transcript='/missing/transcript', expectedInsertion=positive)])))
            result = subprocess.run([str(fixture.root / 'NativePasteCheck'), str(specification)], capture_output=True, timeout=10)
            if result.returncode == 0 or error not in json.loads(report.read_text())['error']:
                raise AssertionError('Invalid native paste acceptance specification was not rejected correctly')
        source = fixture.root / 'speech é 🦀 # %.wav'
        shutil.copyfile(fixture.fixture.source, source)
        expected = fixture.fixture.run_cli(source, '--chunk', '2.5', '--overlap', '0.5')
        batch = fixture.root / 'batch.json'
        batch.write_text(json.dumps(expected), encoding='utf-8')
        capture = fixture.root / 'capture.wav'
        with wave.open(str(source), 'rb') as reader, wave.open(str(capture), 'wb') as writer:
            writer.setparams(reader.getparams())
            writer.writeframes(reader.readframes(reader.getframerate()))
        recording_batch = fixture.root / 'recording-batch.json'
        recording_batch.write_text(json.dumps(fixture.fixture.run_cli(capture)), encoding='utf-8')
        environment = dict(fixture.env, FSCRIPT_DESKTOP_TEST_CAPTURE=str(capture),
                           FSCRIPT_DESKTOP_TEST_RECORDING_BATCH=str(recording_batch), FSCRIPT_DESKTOP_TEST_CLI=str(binary),
                           FSCRIPT_DESKTOP_TEST_SOURCE=str(source),
                           FSCRIPT_DESKTOP_TEST_BATCH=str(batch))
        subprocess.run(['swift', 'test', '--package-path', str(ROOT / 'desktop'),
                        '--scratch-path', str(ROOT / 'target/desktop-build')],
                       env=environment, check=True)
    finally:
        fixture.doCleanups()


if __name__ == '__main__':
    main()
