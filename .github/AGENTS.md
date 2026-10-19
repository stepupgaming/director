# Purpose

- Own GitHub Actions workflows and issue templates.

## Ownership

- `workflows/ci.yml` owns the single Linux CI job: release clippy and release workspace tests, including the visual suite.
- `ISSUE_TEMPLATE/` owns GitHub issue forms and template routing.

## Local Contracts

- CI runs on push to `main`, on pull requests and on `workflow_dispatch`. The repository is public, so standard runners carry no minute quota.
- CI commands match the local verification in the root `AGENTS.md`.
- Bundled fonts are committed under `crates/director-core/assets/fonts/`; CI downloads no fonts.
- FFmpeg runs as a subprocess. CI installs the `ffmpeg` package and verifies `ffmpeg` and `ffprobe`; no FFmpeg development libraries are linked.

## Work Guidance

- Update workflow commands when package names, feature flags, or test entry points change.
- Add Windows or macOS jobs only for a platform-specific regression; the Linux job is the gate.

## Verification

- `cargo clippy --workspace --all-targets --release -- -D warnings`
- `cargo test --workspace --release`

## Child DOX Index

- `workflows/` - CI workflow YAML.
- `ISSUE_TEMPLATE/` - Bug, feature, release, and template config files.
