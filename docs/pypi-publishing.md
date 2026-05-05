# PyPI trusted publishing

This repository is already wired to publish `fscript` to PyPI directly from GitHub Actions.

The GitHub-side workflow is:

- workflow file: `.github/workflows/release.yml`
- job: `publish-pypi`
- permissions: `id-token: write`
- environment: `pypi`

At the moment, the trusted-publishing job uploads the macOS arm64 wheel only.
The Linux wheel is still built in CI and attached to GitHub Releases, but it is not uploaded to PyPI yet because PyPI rejects the plain `linux_x86_64` platform tag. A proper manylinux-compatible Linux wheel can be added later.

## After it is live

The intended UX is:

```bash
uvx fscript lecture.mp3
uv tool install fscript
```
