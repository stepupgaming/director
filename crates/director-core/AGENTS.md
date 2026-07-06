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
- Caption system (`node/caption.rs`) provides SRT and WebVTT parsing, `CaptionNode` with **pre-laid-out, structurally stable text blocks**: every word's x position and baseline is fixed at layout time and never reflows when the active word highlights. Each cue is one Skia `Paragraph` with per-word `TextStyle` runs sharing a **uniform `font_size`** (only fill color / glow / opacity vary across words, never glyph metrics). `CaptionLine` holds split `words: Vec<CaptionWord>` with per-word timing via `auto_time_words()` (divides cue duration equally across the joined stream, or uses explicit timings). Each `CaptionWord` carries a `display_line_idx` so multi-line SRT text keeps its `\n` line breaks in the laid-out paragraph (the builder injects `\n` at display-line transitions and only emits spaces within the same display line). Three rendering colors: `color` (future words, default white), `highlight_color` (active word, default gold `#FFD60A`), `spoken_color` (already-spoken words, default `#E5E5E5`). **Active-word overlay** is drawn as part of the same paragraph run — only fill color, glow, and opacity change for the active word: highlight color, glow shadow (no offset, time-varying blur), color shimmer toward white at 1.5Hz up to 0.10, opacity micro-pulse at 2.4Hz ±0.04. **Pre-laid slot allocation** so adding/removing cues never reflows existing ones: `compute_slot_assignments()` walks cue start/end events in chronological order and assigns each new cue the earliest-free slot; slot indices are permanent for the lifetime of the cue. **Single global active word**: across all currently visible cues we pick the word with the latest `start_sec`; non-active words on the same cue render in dim future/spoken colors. **Line-level kinetic lift**: paragraphs containing any active word get a sub-pixel y bob ±2px at 1.4Hz so the line breathes; x stays stable. **Legibility for any background**: light drop shadow applied to every word (offset 0,2 by default). With `auto_adapt_shadow: true` (default), `Canvas::peek_pixels()` samples bg luminance under the caption rect and flips shadow between black/white so text stays readable on light or dark video. Outline is **opt-in** (defaults OFF) — when enabled it's a 4-direction 1.0px outline on non-active words only; the active word's glow replaces its purpose, so the outline doesn't dominate text. Rhai props: `color`, `highlight_color`, `spoken_color`, `line_spacing`, `caption_position`, `bottom_margin`, `top_margin`, `align`, `font_family`, `color_transition_duration`, `color_pulse_amplitude`, `future_opacity`, `spoken_opacity`, `active_opacity`, `glow_enabled`, `glow_blur`, `glow_pulse_amplitude`, `glow_pulse_hz`, `entry_glow_ramp`, `text_shadow_enabled`, `text_shadow_color`, `text_shadow_blur`, `text_shadow_offset_x`, `text_shadow_offset_y`, `outline_enabled`, `outline_thickness`, `auto_adapt_shadow`. Multi-line: handled inside a single cue via `\n` in cue text — words carry `display_line_idx` so the paragraph builder inserts `\n` between lines. Layout `measure()` reserves `max_overlapping_lines()` heights. Visual regression: `tests/visual/fixtures/captions/caption_word_highlight.rhai` captures at t=1,3,5,7s. Preview tool: `cargo run -p director-cli --bin preview_frame -- <script> -t <sec>`. Unit tests in `node/caption.rs` (16 tests covering SRT/VTT parsing, word auto-timing, active word detection, multi-line, max overlap, slot stability, exactly-one-active-global invariant, uniform font_size, default style config). Integration tests in `tests/api_showcase.rs` (`caption_basic`, `caption_renders_visible_pixels`).

### Structural caption invariants (must hold at every frame)

- **Stable layout**: every word's x position and baseline is fixed at layout time and stays fixed for the cue's lifetime. No glyph reflow caused by highlighting the active word.
- **Whitespace preserved**: inter-word spaces use the same `font_size` as the word (the space rune is included inside the word's `push_style` block via `format!("{} ", word.text)`).
- **Display-line breaks preserved**: `\n` in cue text is injected as a real paragraph line break by the builder (spaces only between words on the same display line).
- **Single global active word**: at most one word across all visible cues has the highlight treatment; selection is `latest start_sec wins`.
- **Slot-stable blocks**: each cue is permanently assigned a stacked slot index before any rendering; adding/removing cues never moves existing cues.
- **No font_size variation across words**: every `TextStyle` run uses `style.font_size`. Highlighting never enlarges glyphs.
- **Continuous motion is overlay-only**: glow, color, opacity, drop shadow, and line-level y-lift all change without affecting layout glyph metrics.

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
