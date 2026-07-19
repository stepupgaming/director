# Purpose

- Own asset pipeline and preprocessing utilities for Director.

## Ownership

- `src/lib.rs` owns pipeline library behavior.
- `tests/` owns parity and error-handling checks.
- `README.md` owns crate usage notes.

## Local Contracts

- Pipeline utilities should preserve compatibility with `director-core` and `director-schema` data contracts.
- Pipeline errors should be actionable for callers.
- **Scene document SSOT:** versioned `director_core::project` JSON is the canonical Director save/load path (Plan 05). `MovieRequest` / `load_movie` is a secondary inbound adapter — do not add new node/effect features only on the pipeline path without project-document parity.

## Work Guidance

- Add tests for parity, malformed input, and integration boundaries when pipeline behavior changes.

## Verification

- `cargo test -p director-pipeline`

## Child DOX Index

- No child AGENTS.md files currently required.
