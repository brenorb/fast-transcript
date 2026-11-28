#!/usr/bin/env python3
"""Build, verify, and archive Fast Transcript with its matching CLI.

Default: an explicitly named developer archive. A notarized distribution additionally
requires an existing Developer ID identity and notarytool keychain profile.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile

from macos_bundle import check_bundle, run, sha256

ROOT = Path(__file__).resolve().parents[1]


def source_state(root):
    try:
        revision = run(['git', '-C', root, 'rev-parse', 'HEAD'], text=True).stdout.strip()
        dirty = bool(run(['git', '-C', root, 'status', '--porcelain'], text=True).stdout.strip())
        return dict(revision=revision, dirty=dirty)
    except subprocess.CalledProcessError:
        return dict(revision=None, dirty=None)


def archive(app, destination):
    run(['ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', app, destination])


def notarize(app, submission, profile, *, execute=run):
    response = execute(['xcrun', 'notarytool', 'submit', submission, '--keychain-profile', profile,
                        '--wait', '--timeout', '20m', '--output-format', 'json'], text=True)
    receipt = json.loads(response.stdout)
    if receipt.get('status') != 'Accepted' or not receipt.get('id'):
        raise ValueError(f"Notarization was not accepted (submission {receipt.get('id', 'unknown')}, status {receipt.get('status', 'unknown')}); no distribution archive was produced")
    execute(['xcrun', 'stapler', 'staple', app])
    execute(['xcrun', 'stapler', 'validate', app])
    execute(['spctl', '--assess', '--type', 'execute', '--verbose=4', app])
    return dict(id=receipt['id'], status=receipt['status'])


def package(args):
    if platform.system() != 'Darwin':
        raise ValueError('macOS app packaging requires macOS and Xcode developer tools')
    if args.notary_profile and args.sign_identity == '-':
        raise ValueError('--notary-profile requires --sign-identity with an existing Developer ID Application certificate')
    if args.notary_profile and args.debug:
        raise ValueError('notarized distribution requires a release build; omit --debug')
    source = source_state(ROOT)
    output = args.output_dir.absolute()
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.fscript-package-', dir=output) as temporary:
        scratch = Path(temporary)
        app = scratch / 'Fast Transcript.app'
        command = ['bash', ROOT / 'scripts/build_macos_app.sh', '--output', app,
                   '--sign-identity', args.sign_identity]
        if args.debug:
            command.append('--debug')
        if args.hardened_runtime:
            command.append('--hardened-runtime')
        # Build diagnostics stay on stderr; successful stdout is one artifact path.
        subprocess.run(list(map(str, command)), stdin=subprocess.DEVNULL, stdout=sys.stderr, check=True)
        info = check_bundle(app, ROOT, require_hardened=args.hardened_runtime or args.sign_identity != '-')
        notarization = None
        if args.notary_profile:
            if not all(record['signature']['developer_id'] for record in info['executables']):
                raise ValueError('notarization requires Developer ID Application signatures on app and CLI')
            submission = scratch / 'submission.zip'
            archive(app, submission)
            notarization = notarize(app, submission, args.notary_profile)
            # Stapling changes the bundle; inspect the final delivered code again.
            info = check_bundle(app, ROOT, require_hardened=True)
        kind = 'distribution' if notarization else 'developer'
        suffix = '' if notarization else '-developer'
        stem = f"fast-transcript-{info['app_version']}-macos-{info['architecture']}{suffix}"
        destination = output / stem
        if destination.exists():
            raise ValueError(f'artifact already exists: {destination}; choose another --output-dir or a new version')
        staged = scratch / 'artifact'
        staged.mkdir()
        zipped = staged / (stem + '.zip')
        archive(app, zipped)
        # Validate the delivered archive, not merely the source build directory.
        extracted = scratch / 'roundtrip'
        run(['ditto', '-x', '-k', zipped, extracted])
        recovered = check_bundle(extracted / app.name, ROOT,
                                 require_hardened=args.hardened_runtime or args.sign_identity != '-')
        if recovered != info:
            raise ValueError('Archive roundtrip changed app metadata, signatures, or executable bytes')
        checksum = sha256(zipped)
        (staged / (zipped.name + '.sha256')).write_text(f'{checksum}  {zipped.name}\n')
        manifest = dict(version=1, kind=kind, configuration='debug' if args.debug else 'release',
                        source=source, app=info, archive=dict(filename=zipped.name, sha256=checksum),
                        notarization=notarization)
        (staged / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
        os.rename(staged, destination)
    return destination


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, default=ROOT / 'dist-macos', help='Artifact parent directory (default: dist-macos)')
    parser.add_argument('--debug', action='store_true', help='Build a development debug app instead of release')
    parser.add_argument('--sign-identity', default=os.environ.get('FSCRIPT_SIGNING_IDENTITY', '-'),
                        help='Existing codesign identity; default is ad-hoc (-), or FSCRIPT_SIGNING_IDENTITY')
    parser.add_argument('--hardened-runtime', action='store_true', help='Also test hardened runtime with an ad-hoc development identity; certificate signing enables it automatically')
    parser.add_argument('--notary-profile', default=os.environ.get('FSCRIPT_NOTARY_PROFILE'),
                        help='Existing notarytool keychain profile; submit, staple, assess, then archive')
    args = parser.parse_args(argv)
    try:
        artifact = package(args)
    except (ValueError, OSError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        parser.exit(1, f'error: {error}\n')
    print(artifact)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
