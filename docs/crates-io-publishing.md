# crates.io trusted publishing

This repository is wired to publish `fast-transcript` to crates.io directly from GitHub Actions on tagged releases.

The GitHub-side workflow is:

- workflow file: `.github/workflows/release.yml`
- job: `publish-crates-io`
- permissions: `id-token: write`
- environment: `crates-io`

## One-time crates.io setup

Before the workflow can publish, crates.io needs a trusted publisher entry for this crate that matches:

- crate: `fast-transcript`
- repository: `brenorb/fast-transcript`
- workflow: `.github/workflows/release.yml`
- environment: `crates-io`

After that, tagged pushes such as `v1.1.3` can publish without storing a long-lived cargo token in GitHub.

## Local verification

Before cutting a release, the useful checks are:

```bash
cargo fmt --check
cargo test --locked
cargo publish --dry-run --locked
```

The main CI workflow already runs `cargo fmt --check`, `cargo test --locked`, and `cargo publish --dry-run --locked` on pushes and pull requests so packaging regressions fail before a tag is cut.

## After it is live

The intended cargo UX is:

```bash
cargo install fast-transcript
fscript --version
```
