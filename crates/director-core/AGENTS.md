# Purpose

- Own Director's main engine: scene graph, node model, layout, rendering, animation, scripting, assets, audio, video export, and visual tests.

## Ownership

- `src/director.rs`, `scene.rs`, and `element.rs` own timeline and scene graph coordination.
- `src/node/` owns node implementations.
- `src/scripting/` owns Rhai types, helpers, and API registration.
- `src/systems/` owns renderer, layout, transitions, and asset systems.
- `src/export/` owns export surfaces.
- `tests/` owns integration, scripting, visual, fixture, and snapshot coverage for engine behavior. Visual tests live in `tests/visual/` and are aggregated by `tests/visual_suite.rs`; a bundled Roboto font in `crates/director-core/assets/fonts/` ensures deterministic text rendering across platforms.

## Local Contracts

- Rhai-facing behavior is public product surface. Update examples, docs, generated truth, and agent API references when it changes.
- The default render mode for 3-argument `new_director(width, height, fps)` is supplied by the caller that registers the Rhai API; preview surfaces use `Preview`, while CLI export uses `Export`.
- Rendering and layout changes must account for visual snapshots and platform-specific outputs.
- Media and export changes must preserve `mock_video` testability where tests or CI depend on it.

## Work Guidance

- Keep node-specific behavior in `src/node/` and shared orchestration in systems or scene/director modules.
- Add or update tests near the behavior changed; use visual tests for user-visible rendering changes.
- Avoid broad rewrites across scripting, rendering, and layout unless the feature actually crosses those layers.

## Verification

- Narrow: `cargo test -p director-core`
- Visual: `cargo test --test visual_suite --no-default-features --features mock_video`
- Workspace impact: `cargo check --workspace --all-targets`

## Child DOX Index

- No child AGENTS.md files currently required.
