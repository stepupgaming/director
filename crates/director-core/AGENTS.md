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
- GPU acceleration (`--gpu` flag + `vulkan` feature) creates Vulkan-backed Skia surfaces in `render_export()` via `create_render_surface()` and `gpu::surfaces::render_target()`. The pixel readback uses `read_pixels()` with `flush_and_submit_surface()` for GPU surfaces and falls back to CPU raster when no Vulkan context is available.
- SkSL shader effects live in `src/node/effect_shaders.rs` as builder functions that return `Option<ImageFilter>`. New shader-color-panels effects: Thermal, Liquid, Acid, Halftone, Ascii, Sketch, Mirror, TriTone, VHS, NeonEdge — all ported from GLSL to SkSL and registered in the Rhai effects API and capability registry.
- Layout improvements closing the gap with Remotion: radial and conic gradients via `GradientType` enum on `GradientConfig`, backdrop blur (glassmorphism) via `save_layer` with `backdrop_layer_saved` flag for save/restore balance, stacked shadows via `BoxShadow` Vec on `BoxNode`, `aspect_ratio` exposed in layout style parsing, and Series/Sequence helper functions for sequential scene composition in `scripting/api/lifecycle.rs`.
- HTTP data fetching (`fetch_text`, `fetch_json`) available in Rhai via `scripting/api/http.rs`, backed by `reqwest`. Enables live data, API integration, and web research as a first-class input path.
- Audio-reactive spectrum visualizer (`SpectrumVisualizerNode`) in `node/spectrum_visualizer.rs` renders FFT-driven frequency bars. Bar heights are computed per-frame in `Director::update()` by downcasting to the node type, with configurable smoothing, position, and styling.
- Caption system (`node/caption.rs`) provides SRT and WebVTT parsing, `CaptionNode` with word-by-word highlighting, **scale-up pop on the active word**, smooth color transitions, and bg-adaptive shadow for any-video legibility. `CaptionLine` holds split `words: Vec<CaptionWord>` with per-word timing via `auto_time_words()` (divides line duration equally, or uses explicit timings). Three rendering colors: `color` (future words, default white), `highlight_color` (active word, default gold `#FFD60A`), `spoken_color` (already-spoken words, default `#E5E5E5`). **Default style targets Remotion / TikTok / IG**: no pill bg (alpha=0), scale-up pop on active word (`active_scale` default 1.18 → 1.22), future words at `future_opacity` 0.85, spoken words at `spoken_opacity` 0.50–0.55, active at 1.0. **Smooth animations**: word colors transition via OKLab interpolation (`Color::ease`) over `word_transition_duration` (default 0.18s); scale ramps triangularly (1.0 → peak → 1.0) over `scale_transition_duration` (default 0.14s); caption lines fade in/out via per-line opacity ramps over `fade_in_duration`/`fade_out_duration` (default 0.25s each). **Legibility for any background**: text shadow + optional 8-direction outline via `TextStyle.add_shadow`. With `auto_adapt_shadow: true` (default), `Canvas::peek_pixels()` samples bg luminance under the caption rect and flips shadow between black/white so text stays readable on light or dark video. Drop shadow offset defaults to (0,2); outline thickness 2.0–2.5. Pill mode is opt-in: setting `background_color.a > 0` enables rounded bg behind the text stack. Rhai props: `highlight_color`, `spoken_color`, `line_spacing`, `word_transition_duration`, `scale_transition_duration`, `fade_in_duration`, `fade_out_duration`, `active_scale`, `future_opacity`, `spoken_opacity`, `active_opacity`, `text_shadow_enabled`, `text_shadow_color`, `text_shadow_blur`, `text_shadow_offset_x`, `text_shadow_offset_y`, `outline_enabled`, `outline_thickness`, `auto_adapt_shadow`. Multi-line: `active_lines()` returns all overlapping lines; `render()` uses a wider window including fade grace periods. Layout `measure()` uses `max_overlapping_lines()` for height estimation. Visual regression: `tests/visual/fixtures/captions/caption_word_highlight.rhai` captures at t=1,3,5,7s. Preview tool: `cargo run -p director-cli --bin preview_frame -- <script> -t <sec>`. Unit tests in `node/caption.rs` (16 tests covering SRT/VTT parsing, word auto-timing, active word detection, multi-line, max overlap, smooth color transitions, scale ramps, per-word opacity, default style config). Integration tests in `tests/api_showcase.rs` (`caption_basic`, `caption_renders_visible_pixels`).

## Work Guidance

- Keep node-specific behavior in `src/node/` and shared orchestration in systems or scene/director modules.
- Add or update tests near the behavior changed; use visual tests for user-visible rendering changes.
- Avoid broad rewrites across scripting, rendering, and layout unless the feature actually crosses those layers.
- GPU surface creation relies on `skia_safe::gpu::surfaces::render_target()` which is available in the base skia-safe crate without feature flags. The `vulkan` feature only controls context creation; fallback to raster is automatic.
- `Canvas::new_surface()` or `Surface::new_surface()` should be preferred over `surfaces::raster()` in nodes that render to an existing canvas, as this preserves GPU compatibility.

## Verification

- Narrow: `cargo test -p director-core`
- Visual: `cargo test --test visual_suite --no-default-features --features mock_video`
- Workspace impact: `cargo check --workspace --all-targets`

## Child DOX Index

- No child AGENTS.md files currently required.
