"""Inspect a built app without opening a window, model, device or permission dialog."""
import hashlib
from pathlib import Path
import plistlib
import re
import subprocess

from macos_metadata import check_cli_permissions


def run(arguments, **kwargs):
    return subprocess.run(list(map(str, arguments)), stdin=subprocess.DEVNULL,
                          capture_output=True, check=True, **kwargs)


def sha256(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as source:
        for block in iter(lambda: source.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def signature(path):
    run(['codesign', '--verify', '--strict', path])
    details = run(['codesign', '--display', '--verbose=4', path], text=True).stderr
    runtime = re.search(r'\bflags=0x([0-9a-fA-F]+)', details)
    identifier = re.search(r'^Identifier=(.+)$', details, re.M)
    team = re.search(r'^TeamIdentifier=(.+)$', details, re.M)
    return dict(identifier=identifier[1] if identifier else None,
                kind='ad-hoc' if 'Signature=adhoc' in details else 'certificate',
                developer_id='Authority=Developer ID Application:' in details,
                team_id=team[1] if team and team[1] != 'not set' else None,
                hardened_runtime=bool(runtime and int(runtime[1], 16) & 0x10000))


def check_bundle(bundle, root, *, require_hardened=False):
    bundle, root = Path(bundle), Path(root)
    run(['codesign', '--verify', '--deep', '--strict', bundle])
    info = plistlib.loads((bundle / 'Contents/Info.plist').read_bytes())
    template = plistlib.loads((root / 'resources/fscript-Info.plist').read_bytes())
    version = re.search(r'^version = "([^"]+)"', (root / 'Cargo.toml').read_text(), re.M)[1]
    expected = dict(CFBundleIdentifier='com.brenorb.fast-transcript', CFBundleName='Fast Transcript',
                    CFBundleExecutable='FastTranscriptApp', CFBundlePackageType='APPL',
                    CFBundleVersion=version, CFBundleShortVersionString=version,
                    CFBundleIconFile='FastTranscript.icns')
    for key in ('NSMicrophoneUsageDescription', 'NSAudioCaptureUsageDescription', 'NSCalendarsFullAccessUsageDescription'):
        expected[key] = template[key]
    for key, value in expected.items():
        if info.get(key) != value:
            raise ValueError(f'App metadata does not match its shared CLI/source: {key}')
    for name in ('LICENSE', 'README.md', 'FastTranscript.icns'):
        if not (bundle / 'Contents/Resources' / name).is_file():
            raise ValueError(f'App resource is missing: {name}')
    cli = bundle / 'Contents/Helpers/fscript'
    minimum = check_cli_permissions(cli, root / 'resources/fscript-Info.plist')
    if info.get('LSMinimumSystemVersion') != minimum:
        raise ValueError('App and CLI deployment targets differ')
    records = []
    for relative, identifier in [('Contents/MacOS/FastTranscriptApp', 'com.brenorb.fast-transcript'),
                                 ('Contents/Helpers/fscript', 'com.brenorb.fast-transcript.cli')]:
        binary = bundle / relative
        architectures = sorted(run(['lipo', '-archs', binary], text=True).stdout.split())
        signed = signature(binary)
        if signed['identifier'] != identifier:
            raise ValueError(f'Unexpected code identity: {relative}')
        if require_hardened and not signed['hardened_runtime']:
            raise ValueError(f'Hardened runtime is missing: {relative}')
        if signed['hardened_runtime']:
            actual = plistlib.loads(run(['codesign', '--display', '--entitlements', ':-', binary]).stdout)
            required = plistlib.loads((root / 'resources/FastTranscript.entitlements').read_bytes())
            if actual != required:
                raise ValueError(f'Unexpected hardened runtime entitlements: {relative}')
        # No developer-machine runtime search paths can escape into a release.
        for line in run(['otool', '-L', binary], text=True).stdout.splitlines()[1:]:
            dependency = line.strip().split(' (compatibility version', 1)[0]
            if not dependency.startswith(('/System/Library/', '/usr/lib/')):
                raise ValueError(f'Unbundled non-system library in {relative}: {dependency}')
        records.append(dict(path=relative, sha256=sha256(binary), architectures=architectures, signature=signed))
    if records[0]['architectures'] != records[1]['architectures'] or len(records[0]['architectures']) != 1:
        raise ValueError('App and CLI must target the same single architecture')
    architecture = records[0]['architectures'][0]
    if architecture not in ('arm64', 'x86_64'):
        raise ValueError(f'Unsupported app architecture: {architecture}')
    if records[0]['signature']['team_id'] != records[1]['signature']['team_id']:
        raise ValueError('App and CLI signer teams differ')
    return dict(bundle_id=info['CFBundleIdentifier'], app_version=version,
                minimum_macos=minimum, architecture=architecture, executables=records)
