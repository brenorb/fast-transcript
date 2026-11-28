# macOS app packaging

The app and `fscript` are built together and verified as one artifact. Packaging
produces a ZIP, its SHA-256 sidecar, and a machine-readable manifest. The archive
is extracted and verified before publication; the CLI inside the extracted app
must have the expected version, permissions, architecture and code identity.

From a macOS checkout with Rust, Python 3 and the full Xcode developer tools:

```bash
python3 scripts/package_macos_app.py
```

If `xcode-select -p` points to Command Line Tools and SwiftUI compilation reports
a missing macro plugin, select the installed full Xcode for this command only:

```bash
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer python3 scripts/package_macos_app.py
```

This leaves the system's developer-tool selection unchanged.

The default is a release build with an ad-hoc developer signature, saved under
`dist-macos/fast-transcript-VERSION-macos-ARCH-developer/`. Stdout prints that
artifact directory; build diagnostics use stderr. Existing artifact directories
are preserved: use another `--output-dir` or a new app version for another build.
The builder stages its app privately; packaging never replaces the installed app
or `target/desktop/Fast Transcript.app`. `--debug` makes a faster development
build. `--hardened-runtime` exercises the same protections with an ad-hoc identity.

These are repeatable build/package steps, not a bit-for-bit reproducibility claim
across toolchains, signature timestamps or builds. The manifest describes the
actual bytes and architecture. `source.revision` and `source.dirty` identify a
Git checkout when available; source archives without Git report null values.
CI requires the exact workflow commit and `source.dirty: false`. Generated CLI
archives under `dist/` and the `.wheel-smoke/` test environment are ignored build
outputs; edited tracked files and other untracked files still mark a checkout
dirty. Local developer packaging retains that dirty state rather than hiding it.

## Developer ID and notarization

For public distribution, supply an existing Developer ID Application identity
and an existing notarytool keychain profile:

```bash
python3 scripts/package_macos_app.py \
  --sign-identity 'Developer ID Application: YOUR NAME (TEAMID)' \
  --notary-profile YOUR_EXISTING_PROFILE
```

`FSCRIPT_SIGNING_IDENTITY` and `FSCRIPT_NOTARY_PROFILE` provide the same defaults.
The scripts do not create certificates, import private keys, change keychain trust,
store credentials or prompt for passwords. Configure those separately with Apple's
tools and your developer account. No notarization submission occurs in the default
developer build.

Certificate signing enables hardened runtime and secure timestamps for both the
app and its CLI. The checked-in entitlements permit audio input and calendar
access, matching the implemented features. macOS consent is still required when
those resources are used. No JIT, debugger, unsigned-memory or library-validation
exceptions are added. The current executables link only system libraries; a
developer-machine or other unbundled library dependency fails verification.

Notarization requires a release build and Developer ID signatures on both
executables. The script submits an initial ZIP with `notarytool --wait`, requires
an Accepted receipt with a submission ID, staples/validates the ticket, and runs
Gatekeeper assessment. It then archives and verifies the stapled app. The output
omits the developer suffix and its manifest has `kind: distribution` only after
all these steps succeed. Pending, rejected, malformed or failed submissions
cannot produce a distribution artifact. The wait is bounded at 20 minutes; Apple's
server may continue processing a timed-out submission.

Apple's primary contracts:
[hardened runtime and resource access](https://developer.apple.com/documentation/xcode/configuring-the-hardened-runtime),
[calendar entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.personal-information.calendars),
and [notarization/stapling](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution).

## Manifest and integrity

`manifest.json` version 1 contains:

- `kind`: developer or distribution; `configuration`: debug or release.
- `source`: revision and dirty state, or nulls for an archive without Git.
- `app`: bundle ID, app version, minimum macOS, actual Mach-O architecture,
  and executable records containing relative path, SHA-256, architectures and
  signature identity/kind/team/hardened-runtime/Developer-ID metadata.
- `archive`: ZIP basename and SHA-256; the sidecar uses the same digest.
- `notarization`: accepted submission ID/status, or null.

The app/CLI versions and signer teams must agree. Permission descriptions must
match the shared source template; microphone, system-audio and calendar strings
are present in the app and embedded native CLI metadata. Hardened executables
must have exactly the checked-in resource entitlements. Both code signatures,
bundle resources, minimum OS and executable hashes are checked again after ZIP
extraction. The app targets macOS 14.2 or later and is built for the host's actual
single architecture; this workflow does not claim a universal app.

To independently verify an artifact:

```bash
cd dist-macos/fast-transcript-VERSION-macos-ARCH-developer
shasum -a 256 -c fast-transcript-VERSION-macos-ARCH-developer.zip.sha256
```

## Installation and everyday use

Extract the ZIP and move **Fast Transcript.app** to Applications. Open the app,
drop audio/video or paste a URL, and transcribe. Install FFmpeg/ffprobe for media
conversion and yt-dlp (or uv for its fallback) for supported video sites, as for
the CLI. Model weights download on first use when needed. SDK models and external
note agents keep their existing runtime/login requirements. Models, account
credentials and user transcripts are not bundled in the archive.

The included command is
`/Applications/Fast Transcript.app/Contents/Helpers/fscript`. Scripts may invoke
that path directly. To expose this exact helper as `fscript`, create a symlink in
a directory already on your PATH; preserve any existing installation rather than
overwriting it. The app and helper use the same model/agent/calendar/meeting
preferences. Ordinary CLI use remains `fscript ./video.mp4` or `fscript "URL"`.

Developer archives are intended for source-build validation. Ad-hoc code identity
can change across rebuilds, which can cause macOS to require renewed resource
permission. A persistent Developer ID identity is needed for stable signed updates.
The scripts never reset TCC, weaken Gatekeeper or modify permission databases.

## CI and current delivery state

Main CI builds a hardened ad-hoc debug archive, verifies its extracted code, checks
ZIP CRC/paths/executable modes, compares independent `shasum` and ZIP-member hashes,
and runs the extracted CLI's everyday/default and structured transcription against
an independent PCM hash. Property checks ensure 128 non-accepted/incomplete notary
receipts never reach stapling; a valid receipt must staple, validate and assess in
order. Those receipt tests use an explicit stub and no Apple account.

A manual **Release** workflow run on a branch (normally main) additionally builds the release-mode developer
archive on macOS, validates it, and uploads `desktop-macos-developer`. Existing
tagged CLI/PyPI/crates publication stays separate; this change does not publish an
unaccepted app as a stable desktop release. Stable desktop publication waits for
the milestone-7 native acceptance gates and a real signed/notarized acceptance.

On 2026-11-28 the local hardened ad-hoc debug archive passed all six packaging
checks, including its extracted CLI with private audio/model fixtures. No usable
code-signing identity was available to this build (`security find-identity`
reported zero valid identities). Real Developer ID signing, Apple notarization
and Gatekeeper distribution acceptance remain unverified. The native automatic
call and first-use calendar grant checks also remain recorded in the roadmap.


The local default release archive also passed the same six checks. A separate
real-model run from the extracted hardened developer ZIP used cached
parakeet-v3-int8 on CPU and matched all three reference segments of the public
speech track exactly. This demonstrates the default ONNX inference path under
hardened runtime without library-validation exceptions. It does not validate
native recording permissions under that signature or the credentialed notary path.
