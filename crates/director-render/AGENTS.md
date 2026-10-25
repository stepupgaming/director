# Purpose

- Paint frames of a `director_scene::SceneDocument`. This crate is the phase 3 renderer of `docs/plans/10-native-platform-overhaul.md`.

## Ownership

- `src/lib.rs` owns `Renderer`, `FrameSession`, `Frame` and `RenderError`.
- `tests/html_examples.rs` owns parity between the document render and the compiled `Director` render for HTML: every example, a paint fixture, SMIL, looped video, a sampled capture with path and embedded video, text, SVG and nested boxes, and a Paper shader. Video fixtures need FFmpeg on the path.
- `tests/core_nodes.rs` owns the same parity for core Lottie (with an override image) and audio scenes: the document re-exports unchanged, the mix is sample-equal and frames match, including a spectrum visualizer and an audio binding.

## Local Contracts

- `Renderer` holds an `Arc<SceneDocument>`, the asset loader, an `ImportRegistry` (core kinds by default, `with_registry` for frontend kinds such as `director_html::register_import`) and a render mode (export by default).
- `Renderer::session` imports the document into a fresh `Director` through `director_core::scene_import`. A `FrameSession` owns that `Director`, its layout engine and raster surfaces, and reuses them across frames.
- `FrameSession::render(time)` paints through `render_sampled_at_time`, so motion blur and transitions match export. It returns straight-alpha RGBA8 at the document output size (`width * output_scale_x`, rounded).
- `Renderer::render_frames(times)` paints on the rayon pool with one session per worker and returns results in the order of `times`. A session that fails to import fails each of its frames with `RenderError::Session`.
- The renderer paints on the CPU raster backend.

## Work Guidance

- Painting still runs through the core `Director` render path. Move evaluation and paint into this crate kind by kind while the visual suite and `tests/html_examples.rs` stay green.

## Verification

- `cargo test -p director-render --release`
- `cargo test -p director-core --release --test visual_suite` checks the golden corpus through `scene_import`.

## Child DOX Index

- None.
