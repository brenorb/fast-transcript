#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != Darwin ]]; then
  echo 'Fast Transcript desktop requires macOS.' >&2
  exit 1
fi
root="$(cd "$(dirname "$0")/.." && pwd)"
configuration=release
signing_identity="${FSCRIPT_SIGNING_IDENTITY:--}"
output="$root/target/desktop/Fast Transcript.app"
hardened_runtime=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --debug) configuration=debug; shift ;;
    --output) output="$2"; shift 2 ;;
    --sign-identity) signing_identity="$2"; shift 2 ;;
    --hardened-runtime) hardened_runtime=true; shift ;;
    -h|--help)
      echo 'Usage: scripts/build_macos_app.sh [--debug] [--output APP_PATH] [--sign-identity IDENTITY] [--hardened-runtime]'
      exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

if [[ "$signing_identity" != - ]]; then hardened_runtime=true; fi

cargo_args=(build --locked --manifest-path "$root/Cargo.toml")
if [[ "$configuration" == release ]]; then cargo_args+=(--release); fi
cargo_bin="$(cargo "${cargo_args[@]}" --message-format=json-render-diagnostics | python3 -c '
import json, sys
paths = set()
for line in sys.stdin:
    event = json.loads(line)
    if (event.get("reason") == "compiler-artifact" and event.get("target", {}).get("name") == "fscript"
            and "bin" in event["target"]["kind"] and event.get("executable")):
        paths.add(event["executable"])
if len(paths) != 1:
    raise SystemExit("Cargo did not report exactly one fscript executable")
print(paths.pop())
')"
swift_args=(--package-path "$root/desktop" --scratch-path "$root/target/desktop-build" --configuration "$configuration" --product FastTranscriptApp)
swift build "${swift_args[@]}"
app_bin="$(swift build "${swift_args[@]}" --show-bin-path)/FastTranscriptApp"
python3 - "$root" "$cargo_bin" "$app_bin" "$output" "$signing_identity" "$hardened_runtime" <<'PY'
from pathlib import Path
import os
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile

root, cargo_binary, app_binary, output, signing_identity, hardened_runtime = sys.argv[1:]
hardened_runtime = hardened_runtime == "true"
root, output = Path(root), Path(output).absolute()
sys.path.insert(0, str(root / 'scripts'))
from macos_metadata import check_cli_permissions
from macos_bundle import check_bundle
minimum = check_cli_permissions(Path(cargo_binary), root / 'resources/fscript-Info.plist')
version = re.search(r'^version = "([^"]+)"', (root / 'Cargo.toml').read_text(), re.M)[1]
output.parent.mkdir(parents=True, exist_ok=True)
# Build the complete bundle beside the destination before replacing a prior build.
with tempfile.TemporaryDirectory(prefix='.fast-transcript-app-', dir=output.parent) as scratch:
    bundle = Path(scratch) / output.name
    executables = bundle / 'Contents/MacOS'
    helpers = bundle / 'Contents/Helpers'
    resources = bundle / 'Contents/Resources'
    for directory in (executables, helpers, resources):
        directory.mkdir(parents=True)
    shutil.copy2(app_binary, executables / 'FastTranscriptApp')
    for name in ('fscript',):
        binary = Path(cargo_binary)
        actual = subprocess.check_output([str(binary), '--version'], text=True).strip()
        if actual != f'{name} {version}':
            raise SystemExit(f'{name} does not match app version {version}: {actual}')
        shutil.copy2(binary, helpers / name)
    for name in ('LICENSE', 'README.md'):
        shutil.copy2(root / name, resources / name)
    shutil.copy2(root / 'resources/FastTranscript.icns', resources / 'FastTranscript.icns')
    info = dict(CFBundleName='Fast Transcript', CFBundleDisplayName='Fast Transcript',
                CFBundleIdentifier='com.brenorb.fast-transcript', CFBundleExecutable='FastTranscriptApp',
                CFBundlePackageType='APPL', CFBundleShortVersionString=version, CFBundleVersion=version,
                LSMinimumSystemVersion=minimum, NSHighResolutionCapable=True,
                CFBundleIconFile='FastTranscript.icns',
                CFBundleDocumentTypes=[dict(CFBundleTypeName='Audio and video', CFBundleTypeRole='Viewer',
                                           LSHandlerRank='Alternate', LSItemContentTypes=['public.audio', 'public.movie'])])
    permissions = plistlib.loads((root / 'resources/fscript-Info.plist').read_bytes())
    for key in ('NSMicrophoneUsageDescription', 'NSAudioCaptureUsageDescription', 'NSCalendarsFullAccessUsageDescription'):
        info[key] = permissions[key]
    (bundle / 'Contents/Info.plist').write_bytes(plistlib.dumps(info))
    # Sign nested code before the app. A persistent certificate gives the same
    # signer/identifier requirement across rebuilds; ad-hoc identity is per-build.
    signing = ['--options', 'runtime', '--entitlements', str(root / 'resources/FastTranscript.entitlements')] if hardened_runtime else []
    if signing_identity != '-': signing += ['--timestamp']
    for binary in helpers.iterdir():
        subprocess.run(['codesign', '--force', '--sign', signing_identity,
                        '--identifier', 'com.brenorb.fast-transcript.cli', *signing, str(binary)], check=True)
    subprocess.run(['codesign', '--force', '--sign', signing_identity, *signing, str(bundle)], check=True)
    check_bundle(bundle, root, require_hardened=hardened_runtime)
    if output.is_symlink() or (output.exists() and not (output / 'Contents/Info.plist').is_file()):
        raise SystemExit(f'refusing to replace a non-app destination: {output}')
    if output.exists():
        previous = plistlib.loads((output / 'Contents/Info.plist').read_bytes())
        if previous.get('CFBundleIdentifier') != info['CFBundleIdentifier']:
            raise SystemExit(f'refusing to replace a different application: {output}')
        shutil.rmtree(output)
    os.replace(bundle, output)
print(output)
PY
