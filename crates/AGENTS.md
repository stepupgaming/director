# Purpose

- Own the Rust workspace crates for Director.

## Ownership

- `director-core/` owns the main rendering engine, scene graph, layout, animation, scripting API, media, export, and visual tests.
- `director-cli/` owns command-line rendering tools and helper binaries.
- `director-schema/` owns schema-facing types and JSON schema reflection.
- `director-pipeline/` owns asset and pipeline utilities.
- `director-developer/` owns the introspection and spec-driven development CLI.
- `director-view/` owns the Rust preview server and React frontend.
- `lottie-core/`, `lottie-data/`, and `lottie-skia/` own Lottie parsing, data models, animation logic, and Skia rendering.

## Local Contracts

- Keep crate responsibilities separated; cross-crate changes should respect the dependency direction in `Cargo.toml`.
- Public API changes must be reflected in README, examples, specs, generated truth, or skill references when those materials describe the changed behavior.
- Tests and fixtures live with the crate that owns the behavior.

## Work Guidance

- Prefer `cargo test -p <crate>` or `cargo check -p <crate>` for narrow iteration.
- Run broader workspace verification when shared contracts or dependencies change.
- Keep package manifests minimal and use workspace dependencies where the root already defines them.

## Verification

- `cargo check --workspace --all-targets`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`

## Child DOX Index

- `director-core/AGENTS.md` - Main engine, Rhai API, export, and visual regression contracts.
- `director-cli/AGENTS.md` - CLI binaries and render command behavior.
- `director-schema/AGENTS.md` - Schema crate and JSON schema reflection.
- `director-pipeline/AGENTS.md` - Pipeline utility crate.
- `director-developer/AGENTS.md` - Developer introspection CLI, prompts, and specs support.
- `director-view/AGENTS.md` - Rust preview server and frontend boundary.
- `lottie-core/AGENTS.md` - Lottie animation logic and expression support.
- `lottie-data/AGENTS.md` - Lottie data models and JSON fixtures.
- `lottie-skia/AGENTS.md` - Lottie Skia renderer and render fixtures.
