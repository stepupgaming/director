# Purpose

- Own the Rust workspace crates for Director.

## Ownership

- `director-core/` owns the engine model: scene graph, layout, animation, scripting API, media, encoder and decoder wrappers, and visual tests.
- `director-html/` owns HTML/CSS/HyperFrames compilation, native CSS adapters, portable authoring projects and bundled assets, optional browser extraction, and Hyper CN compatibility/parity tooling.
- `director-cli/` owns command-line rendering tools and helper binaries.
- `director-schema/` owns schema-facing types and JSON schema reflection.
- `director-scene/` owns the serializable scene document. It has no renderer types. `director-core` exports every golden scene into it and imports it back.
- `director-render/` owns every frame path: painting and exporting a live `Director` or a scene document on worker-owned surfaces.
- `lottie-core/`, `lottie-data/`, and `lottie-skia/` own Lottie parsing, data models, animation logic, and Skia rendering.

## Local Contracts

- Keep crate responsibilities separated; cross-crate changes should respect the dependency direction in `Cargo.toml`.
- Public API changes must be reflected in README, examples, generated truth, or skill references when those materials describe the changed behavior.
- `director-core::api_catalog` is the single source of truth for effects, transitions, nodes, and Rhai lifecycle helpers; `registry` and `director-schema` must stay synced with it.
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

- `director-core/AGENTS.md` - Main engine, Rhai API, encoder, and visual regression contracts.
- `director-html/AGENTS.md` - HTML compiler, portable native projects, capture format, compatibility corpus, and benchmarks.
- `director-cli/AGENTS.md` - CLI binaries and render command behavior.
- `director-schema/AGENTS.md` - Schema crate and JSON schema reflection.
- `director-scene/AGENTS.md` - Serializable scene document with no renderer types.
- `director-render/AGENTS.md` - Frame renderer and export for live and document scenes, and its parity tests.
- `lottie-core/AGENTS.md` - Lottie animation logic and expression support.
- `lottie-data/AGENTS.md` - Lottie data models and JSON fixtures.
- `lottie-skia/AGENTS.md` - Lottie Skia renderer and render fixtures.
