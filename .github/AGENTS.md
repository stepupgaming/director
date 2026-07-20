# Purpose

- Own GitHub Actions workflows and issue templates.

## Ownership

- `workflows/` owns CI and visual regression automation.
- `ISSUE_TEMPLATE/` owns GitHub issue forms and template routing.

## Local Contracts

- CI should reflect commands that contributors can run locally from this repository.
- Keep system dependency installation aligned with Skia, FFmpeg, font, and audio requirements used by tests.
- Do not add workflow-only behavior that bypasses repository verification documented in AGENTS.md files.
- **Auto CI is disabled** on push/PR to conserve private-repo Actions minutes. Workflows stay in tree and run only via `workflow_dispatch` until re-enabled.

## Work Guidance

- Update workflow commands when package names, feature flags, or test entry points change.
- Prefer explicit setup steps for external binary/font dependencies needed by tests.
- To re-enable auto CI, restore `push` / `pull_request` triggers on `ci.yml` and `visual_regression.yml`.

## Verification

- For workflow edits, run the matching local command when feasible:
- `cargo check --workspace --all-targets`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --test visual_suite --no-default-features --features mock_video` from `crates/director-core`

## Child DOX Index

- `workflows/` - CI and visual regression workflow YAML.
- `ISSUE_TEMPLATE/` - Bug, feature, release, and template config files.
