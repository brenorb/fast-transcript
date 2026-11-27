"""Validate native CLI metadata without opening devices or requesting access."""
import plistlib
import re
import subprocess


def check_cli_permissions(binary, template):
    commands = subprocess.check_output(['otool', '-l', str(binary)], text=True)
    sections = re.findall(r'sectname __info_plist\s+segname __TEXT\s+addr \S+\s+size (0x[0-9a-fA-F]+)\s+offset ([0-9]+)', commands)
    if len(sections) != 1:
        raise AssertionError('Expected one embedded Info.plist in the native CLI test binary')
    deployment = re.search(r'\bminos ([0-9.]+)', commands)
    if not deployment or tuple(map(int, deployment[1].split('.')[:2])) < (14, 2):
        raise AssertionError('CLI deployment target must match its macOS 14.2 Core Audio imports')
    size, offset = sections[0]
    with binary.open('rb') as source:
        source.seek(int(offset))
        info = plistlib.loads(source.read(int(size, 16)))
    version = subprocess.check_output([str(binary), '--version'], text=True).strip().split()[1]
    expected = plistlib.loads(template.read_bytes())
    for key in ('NSMicrophoneUsageDescription', 'NSAudioCaptureUsageDescription', 'CFBundleIdentifier'):
        if not info.get(key) or info[key] != expected[key]:
            raise AssertionError(f'CLI metadata does not match the shared template: {key}')
    if info.get('CFBundleVersion') != version or info.get('CFBundleShortVersionString') != version:
        raise AssertionError('CLI embedded metadata does not match its executable version')
    return deployment[1]
