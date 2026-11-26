#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != Darwin ]]; then
  echo 'Fast Transcript desktop requires macOS.' >&2
  exit 1
fi
root="$(cd "$(dirname "$0")/.." && pwd)"
configuration=release
output="$root/target/desktop/Fast Transcript.app"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --debug) configuration=debug; shift ;;
    --output) output="$2"; shift 2 ;;
    -h|--help)
      echo 'Usage: scripts/build_macos_app.sh [--debug] [--output APP_PATH]'
      exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

cargo_args=(build --locked --manifest-path "$root/Cargo.toml")
if [[ "$configuration" == release ]]; then cargo_args+=(--release); fi
cargo "${cargo_args[@]}"
swift_args=(--package-path "$root/desktop" --scratch-path "$root/target/desktop-build" --configuration "$configuration" --product FastTranscriptApp)
swift build "${swift_args[@]}"
app_bin="$(swift build "${swift_args[@]}" --show-bin-path)/FastTranscriptApp"
python3 - "$root" "$configuration" "$app_bin" "$output" <<'PY'
from pathlib import Path
import os
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile

root, configuration, app_binary, output = sys.argv[1:]
root, output = Path(root), Path(output).absolute()
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
        binary = root / 'target' / configuration / name
        actual = subprocess.check_output([str(binary), '--version'], text=True).strip()
        if actual != f'{name} {version}':
            raise SystemExit(f'{name} does not match app version {version}: {actual}')
        shutil.copy2(binary, helpers / name)
    for name in ('LICENSE', 'README.md'):
        shutil.copy2(root / name, resources / name)
    info = dict(CFBundleName='Fast Transcript', CFBundleDisplayName='Fast Transcript',
                CFBundleIdentifier='com.brenorb.fast-transcript', CFBundleExecutable='FastTranscriptApp',
                CFBundlePackageType='APPL', CFBundleShortVersionString=version, CFBundleVersion=version,
                LSMinimumSystemVersion='14.0', NSHighResolutionCapable=True,
                NSMicrophoneUsageDescription='Transcribe your voice locally while you record or hold the dictation button.',
                CFBundleDocumentTypes=[dict(CFBundleTypeName='Audio and video', CFBundleTypeRole='Viewer',
                                           LSHandlerRank='Alternate', LSItemContentTypes=['public.audio', 'public.movie'])])
    (bundle / 'Contents/Info.plist').write_bytes(plistlib.dumps(info))
    subprocess.run(['codesign', '--force', '--deep', '--sign', '-', str(bundle)], check=True)
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
