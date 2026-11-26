# PyPI trusted publishing

This repository is wired to publish `fscript` to PyPI from the tagged release workflow in GitHub Actions.

The GitHub-side workflow is:

- workflow file: `.github/workflows/release.yml`
- trigger: pushes to tags matching `v*`
- job: `publish-pypi`
- permissions: `id-token: write`
- environment: `pypi`

At the moment, the trusted-publishing job uploads the macOS arm64 wheel only.
The Linux wheel is still built in CI and attached to GitHub Releases, but it is not uploaded to PyPI yet because PyPI rejects the plain `linux_x86_64` platform tag. A proper manylinux-compatible Linux wheel can be added later.

CI still builds both release-wheel targets, runs the Python wrapper tests, verifies both bundled binaries expose the same version, help, and manuals as their release counterparts, and smoke-tests the installed `fscript` and `fscriptctl` entrypoints before any tagged publish happens. Every wheel and release archive includes both binaries.

## After it is live

The published wheel requires Python 3.9+.

The intended UX is:

```bash
uvx fscript lecture.mp3
uv tool install fscript
fscriptctl notes lecture.txt
```

For an ephemeral control invocation, use `uvx --from fscript fscriptctl notes lecture.txt`.
