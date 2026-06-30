# Purpose

- Own Lottie JSON data structures and test fixtures.

## Ownership

- `src/model.rs` owns data model definitions.
- `tests/` owns JSON fixtures and parsing integration tests.

## Local Contracts

- Model changes must keep fixture parsing behavior intentional and tested.
- Preserve fixture files as representative Lottie inputs.

## Work Guidance

- Prefer serde-compatible model changes with explicit optional fields for missing JSON properties.
- Update parsing tests when fixture expectations change.

## Verification

- `cargo test -p lottie-data`

## Child DOX Index

- No child AGENTS.md files currently required.
