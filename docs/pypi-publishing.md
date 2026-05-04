# PyPI trusted publishing

This repository is already wired to publish `fscript` to PyPI directly from GitHub Actions.

The GitHub-side workflow is:

- workflow file: `.github/workflows/release.yml`
- job: `publish-pypi`
- permissions: `id-token: write`
- environment: `pypi`

What still needs to happen is the PyPI-side publisher registration.

## Recommended setup

Create a **pending publisher** on PyPI for the project name `fscript` with:

- owner: `brenorb`
- repository: `fast-transcript`
- workflow name: `release.yml`
- environment name: `pypi`

After that, pushing a tag like `v0.2.3` will let GitHub Actions publish the built wheel(s) directly to PyPI.

## Why pending publisher

`fscript` does not need to exist on PyPI first.
PyPI can create the project on the first successful trusted publish.

## After it is live

The intended UX is:

```bash
uvx fscript lecture.mp3
uv tool install fscript
```
