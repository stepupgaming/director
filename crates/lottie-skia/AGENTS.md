# Purpose

- Own Skia rendering for Lottie animations and renderer output fixtures.

## Ownership

- `src/lib.rs` owns Lottie-to-Skia rendering behavior.
- `tests/` owns render, compositing, context, animation, and heart-eyes coverage.
- PNG files at crate root are render outputs or fixtures used for inspection.

## Local Contracts

- Rendering changes should be validated visually or with render tests.
- Keep dependencies aligned with `lottie-core` and `lottie-data` contracts.

## Work Guidance

- Add or update render tests when visual output semantics change.
- Avoid checking in large render artifacts unless they are durable fixtures.

## Verification

- `cargo test -p lottie-skia`

## Child DOX Index

- No child AGENTS.md files currently required.
