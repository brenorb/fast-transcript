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

CI builds both release-wheel targets, runs wrapper tests, compares the bundled
fscript version/help/manuals with the release binary, and smoke-tests the installed
entrypoint before publishing. Wheels and archives contain one fscript executable.


## After it is live

The published wheel requires Python 3.9+.

The intended UX is:

```bash
uvx fscript lecture.mp3
uv tool install fscript
fscript notes lecture.txt
```

For an ephemeral notes invocation, use `uvx fscript notes lecture.txt`.
