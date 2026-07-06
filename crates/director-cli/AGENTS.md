# Purpose

- Own command-line binaries for rendering Director scripts and reviewing visual outputs.

## Ownership

- `src/main.rs` owns the `director-engine` render CLI.
- `src/bin/review_visuals.rs` owns the visual review helper binary.
- `src/bin/preview_frame.rs` owns the single-frame preview binary.
- `README.md` owns CLI usage and exit-code documentation.

## Local Contracts

- CLI behavior must remain consistent with `README.md`.
- User-facing errors should distinguish script failures from render failures.
- CLI changes that expose or require Rhai API behavior must stay aligned with `director-core`.
- `director-engine` registers the Rhai API with `Export` as the default mode so 3-argument `new_director(width, height, fps)` scripts render export-ready video nodes.

## Work Guidance

- Keep argument parsing explicit and documented.
- Prefer package-scoped checks during CLI work.

## Verification

- `cargo test -p director-cli`
- `cargo run -p director-cli --bin director-engine -- --help`

## Child DOX Index

- No child AGENTS.md files currently required.
