# Caption system — god-tier kinetic captions for short-form video

Each caption cue is a pre-laid-out, structurally stable text block.
Highlighting the active word never moves any other word; no per-line
reflows when a second line enters; exactly one global active word.

## Architecture

* `CaptionNode { lines: Vec<CaptionLine>, ... }` — a node renders a
  sequence of SRT-style caption cues.
* `CaptionLine { text, words, start_sec, end_sec }` — one cue. Words
  carry `display_line_idx` so `\n` in cue text maps to separate
  paragraphs.
* `CaptionWord { text, start_sec, end_sec, display_line_idx }` — one
  word. Per-cue word timing implicit (auto-timed) or explicit.
* `CaptionStyle` — visible defaults and motion configuration. Defaults:
  bold Poppins (700), black stroke outline, per-line pill background,
  drop shadow + active-word glow, auto phrase break (4 words/line),
  resolution scaling from 1920 px reference height.
* `CaptionStyle::resolved_for_canvas(h)` — applies `scale_with_canvas`,
  platform `safe_area_preset`, and dimensional scaling before layout.
* `compute_slot_assignments()` — overlap bookkeeping (legacy slot ids).
  Render uses a single shared anchor; overlapping cues replace in place.
* `CaptionNode::measure` — height from `build_block_layout` SkParagraph
  metrics at `layout_width_fraction * available_width` (no char-width
  estimate). Cached `block_layout` is reused when width + canvas height
  match.
* Per-display-line `Paragraph` — each line is its own Skia Paragraph,
  laid out with **uniform `font_size`**. Every TextStyle run on a single
  paragraph shares the same font size, so glyph metrics never vary
  across active / future / spoken states.
* Per-display-line **pill** — rounded `RRect` drawn behind text (before
  glyphs). `pill_adaptive` inverts fill on light/dark backgrounds.
* `find_global_active()` — exactly one `(line_idx, word_idx)` pair
  at any time, picking the latest `start_sec`. Uses original `lines`
  indices, not visible-cue subset indices.

## Per-word visual state

Each word paints with `compute_word_color(word, time, is_active)`,
producing exactly one of:

* **Active**: `highlight_color`. Renders with `text_shadow_color` and a
  subtle `textShadow` glow at `glow_blur_radius` (default 4 px).
* **Past**: `spoken_color` with `spoken_color_opacity` multiplier.
  Snapped instantly at `word.end_sec` (no transition phase — see
  "Boundary handling" below).
* **Future**: `color` with `future_opacity` multiplier. Smooth ramp from
  `color` to `highlight_color` over `color_transition_duration`
  (default 0.10 s) in the entry window `[start - t_dur, start)`.

Plus a 4-direction font padding inside the per-word TextStyle push so
inter-word spaces always advance by the same font_size as the words
glyph.

## Active word emphasis (overlay, not layout)

The active word gets a tiny **1.04 scale pop** at the moment of
activation. The pop is implemented as a `canvas.scale(1.04, 1.04)`
transform around the active word's center, applied with a second
full-line paragraph paint AFTER the base pass, clipped to the active
word's bbox. The renderer must always paint the full line first;
painting only inside the pop clip hides every other word for ~80 ms
and flashes on each word change. The base paragraph is not re-laid;
the scale affects only the active word's overlay. Scale returns to
`active_pop_duration` (0.08 s default) via `ease_out_quad`. Maximum
`active_pop_scale` is clamped to 1.10 (TikTok/Reels-appropriate).

**No continuous scale wobble. No kinetic lift. No ease_out_back
overshoot. No per-word pulse.** Motion is limited to:
1. Color entry ramp (100 ms).
2. Active pop scale (1.04 → 1.0 over 80 ms).
3. Per-display-line reveal (line 0 immediate, line 1 fades in via
   `save_layer` opacity ramp).

## Boundary handling — exactly one active word

Past → spoken is **instantaneous** at `word.end_sec`. Past → spoken
transition frames would create an additional yellow region when the
next word starts a future→active entry, breaking the "exactly one
active word" invariant. Test: `test_no_dual_yellow_at_word_boundary`.

## Two-line reserve + phrase auto-break

Cues can contain `\n` (multi-line SRT text) or wrap their words into
display lines via `auto_phrase_break` + `max_words_per_line` (default 4).
Phrase-broken display lines default to **`phrase_line_mode: Replace`**
(Opus/TikTok): one line at a time in the same bottom anchor; the next
phrase chunk replaces the previous instead of stacking. Set
`phrase_line_mode: "stack"` to keep all revealed lines visible.

* **Line 0**: visible from `cue.start_sec` (instant reveal, 0.10 s ramp).
* **Line 1+**: visible from `cue.start_sec + offset_frac * cue_duration`
  (default 0.45 for two-line cues) with `reveal_duration` (default
  0.20 s) ease-out ramp. Cues with **3+ display lines** use even
  thirds/fourths via `display_line_window_fracs()`.

Dimensional scaling uses `reference_canvas_height` (1920) when
`scale_with_canvas` is true — not the caption node's tight layout box
height (which would shrink fonts).

## Overlapping cues

Standard subtitle behavior: **one caption block at a time** in the
same bottom (or top/center) anchor. When SRT cues overlap in time, only
the **newest** cue (latest `start_sec`) is rendered; older cues are
hidden rather than stacked in separate Y slots.

`compute_slot_assignments()` still runs for layout bookkeeping, but
render uses a single anchor (`slot_y = 0`). Reserved height is the
tallest single cue block, not the sum of overlap slots.

With `phrase_line_mode: Stack`, display lines within one cue stack
vertically. With `Replace` (default), only the active phrase line
shows. Cue display lines never reflow mid-cue.

## Drop shadow + auto-adapt + pills

* All words carry a light drop shadow (offset `text_shadow_offset_x/y`,
  default `(0, 2)`) for any-background legibility.
* `auto_adapt_shadow` (default true) calls `Canvas::peek_pixels()` to
  sample bg luminance under the caption rect (`lum > 0.55` → black
  shadow; `lum < 0.40` → white shadow; mid → config-defaulted).
* `pill_enabled` (default true) draws a rounded rect behind each
  display line. `pill_adaptive` picks dark/light fill from bg luma.

## Platform safe areas

`safe_area` / `safe_area_preset` applies scaled margins from
`DesignSystem.safe_areas` (`tiktok`, `youtube_shorts`, `instagram_reel`,
etc.). Values scale from 1920 px reference height via
`scaled_safe_zone()`.

## Rhai props

* `font_size`, `color`, `highlight_color`, `spoken_color`
* `bottom_margin`, `top_margin`, `caption_position`, `align`
* `text_direction` — `"auto"` (default, first-strong detect), `"ltr"`, or `"rtl"`
* `line_spacing`, `font_family`, `font_weight` / `weight` (default 700)
* `stroke_enabled`, `stroke_width`, `stroke_color` (default on, 2 px black)
* `pill_enabled`, `pill_color`, `pill_adaptive`, `pill_padding_h`,
  `pill_padding_v`, `pill_radius`
* `auto_phrase_break`, `max_words_per_line`
* `safe_area` / `safe_area_preset`, `scale_with_canvas`,
  `reference_canvas_height`, `uppercase`
* `color_transition_duration` (default 0.10 s — entry ramp only)
* `future_opacity` (0.92), `spoken_opacity` (mapped to
  `spoken_color_opacity`, default 0.55), `active_opacity` (1.0)
* `glow_enabled`, `glow_blur` (default 4.0 px) — subtle glow only on
  active word
* `active_pop_scale` (1.04, clamped to ≤ 1.10), `active_pop_duration`
  (0.08 s)
* `line_reveal_offset_1` (0.45), `line_reveal_duration_1` (0.20)
* `text_shadow_enabled`, `text_shadow_color`, `text_shadow_blur`,
  `text_shadow_offset_x`, `text_shadow_offset_y`
* `auto_adapt_shadow`

## Tests

* SRT / VTT parsing — `test_parse_srt_*`, `test_parse_vtt_*`
* Word auto-timing across `\n`-separated lines —
  `test_word_display_assigns_line_idx`
* Phrase auto-break — `test_phrase_auto_break_splits_long_cue`
* Phrase-break word retiming — `test_phrase_break_retimes_per_display_line`
* Safe-area scaling — `test_safe_area_preset_scales_bottom_margin`
* Legibility score — `test_legibility_score_high_contrast`
* `max_overlapping_lines` — `test_max_overlapping_lines*`
* RTL / bidi — `test_caption_rtl_word_order`; typography `text_auto_detects_arabic_rtl`
* Visible-cue layout index — `test_visible_cue_uses_original_line_index`
* Measure height from Paragraph metrics — `test_measure_height_matches_block_layout`
* Slot stability — `test_slot_assignments_stable`
* Single global active word — `test_single_global_active_word`,
  `test_no_dual_yellow_at_word_boundary`
* Uniform font size across states — `test_word_font_size_uniform`,
  `test_no_ease_out_back_no_wobble_in_style`
* Line reveal schedule — `test_line_reveal_factor_step`
* Visual fixtures —
  `caption_word_highlight.rhai`, `caption_god_tier.rhai`,
  `caption_overlap_stress.rhai`
* Integration: `caption_basic`, `caption_renders_visible_pixels`

## Acceptance criteria

Every invariant the user list demands is enforced here:

* Each word uses uniform font_size, so glyph metrics never vary.
* Inter-word spaces are part of the word's TextStyle push (this is
  why "Wordbywordcaption" never happens).
* Display-line breaks preserved via `\n` injection in the builder.
* Single global active word (latest start_sec wins).
* Past→spoken is instant at boundary (no dual-yellow frames).
* Active pop is OVERLAY scale, never alters font_size on the base
  paragraph.
* Slot indices are permanent; adding/removing cues never moves
  existing cues.
* No continuous motion: no wobble, no pulse, no kinetic lift, no
  ease_out_back overshoot.
* Line reveal keeps geometry stable: only visual opacity ramps.
* Pills are visual-only; they never affect token layout positions.

## Bundled Fonts (deterministic text)

Text rendering must be reproducible across machines, so fonts are bundled
rather than relying on host system fonts.

* Fonts live in crates/director-core/assets/fonts/ (force-committed via
  .gitignore) and are registered at Director construction
  (director.rs, BUNDLED_FONTS).
* Multiple weights of a family (Regular/Bold/Italic) are registered under
  the same family alias so font_weight / font_style resolve deterministically.
* Roboto-Regular is required; Bold/Italic/Inter/Poppins are optional and
  warn (not error) if missing. setup_assets.sh and CI download the full set.
* Emoji fallback is loaded via DefaultAssetLoader::load_font_fallback
  (assets/fonts/emoji.ttf or DIRECTOR_EMOJI_FONT env var).
* Do NOT add host-system-font-dependent text paths; new caption/template
  fonts must be added to BUNDLED_FONTS AND downloaded in setup_assets.sh
  and .github/workflows/ci.yml, then force-kept in .gitignore.## Semantic Entity Registry

- `src/entity.rs` — persistent UUID-based identity for faces, speakers, objects, and
  caption phrases. Each entity stores a temporal bounding-box track and
  supports linearly interpolated sampling at any time.
- `Director.entities` owns the registry; `SceneNode.entity_refs` lets nodes
  declare which entities they track (e.g., a caption avoiding faces).
- Project save/load round-trips entity tracks and node entity_refs without
  remapping UUIDs.
- Rhai API: `create_entity`, `add_entity_sample`, `entity_bbox_at`,
  `find_entities`, `attach_entity` / `detach_entity`, `entity_bboxes_at`.
- Post-load verification: `Director::verify_smart_caption_trajectories` checks
  that every loaded `SmartCaptionNode` trajectory still avoids the tracked
  entities in the restored registry and emits `CaptionOverFace` issues when it
  does not.

## Smart Caption Node

- `src/semantic.rs` — `SemanticManifest`, `SolverConfig`, and `solve_caption_trajectory`.
  Reads face/subject tracks from the `EntityRegistry`, evaluates top/bottom anchor
  candidates per sample, and solves a temporal constraint problem (Viterbi DP) to
  produce a smooth caption trajectory that avoids faces while minimizing motion.
- `src/node/smart_caption.rs` — `SmartCaptionNode` wraps a `CaptionNode` and a
  `CaptionTrajectory`. At each `update(time)` it samples the trajectory and switches
  the inner caption's `CaptionPosition` between `Top` and `Bottom` based on the
  solved anchor.
- `add_smart_caption` in `src/scripting/api/nodes.rs` creates a smart caption from
  Rhai props. Props include `entity_refs` (array of UUID strings), `start_time`,
  `end_time`, `clearance`, `smoothing_window`, `motion_penalty`,
  `overlap_penalty`, `safe_zone_penalty`, and `sample_rate`.
- The solver samples the shot at `sample_rate` Hz, includes both start and end
  boundaries, and applies a moving-window smoothing pass to the resulting trajectory.
- The current solver uses two vertical anchors (top and bottom). It chooses the
  anchor that minimizes face overlap and motion, but it does not yet support
  continuous positioning, left/right anchors, or multi-face occlusion in both
  halves of the frame.
- SmartCaption is catalogued in `api_catalog.rs` as `smart_caption`.
- Project save/load: `SmartCaptionNode` round-trips its caption lines, style,
  trajectory, semantic manifest, config, and continuous flag.

## Project Save/Load Round-trip

- `src/project.rs` — versioned JSON serialization for `Director`. Round-trips
  director settings, scene graph topology, transforms, layout, animated channels,
  tracks, timeline, entity tracks, and node-specific props.
- Fully round-tripped node kinds: `Box`, `Text`, `SmartCaption`, `Image`,
  `Video`, `Lottie`, `Effect`, `Composition`, and `SpectrumVisualizer`.
- `Image`/`Video` preserve source path, object fit, border radius, opacity, and
  render mode; media is reloaded from disk on load when available.
- `Lottie` embeds the parsed model JSON and restores speed, loop flag, frame,
  and opacity.
- `Effect` serializes every `EffectType` variant to JSON and rebuilds the
  effect stack, sharing the director's shader cache.
- `Composition` recursively serializes its internal `Director` project.
- `SpectrumVisualizer` preserves bar count, colors, position, smoothing, and
  all layout parameters.
- Any node that fails to rebuild falls back to a `BoxNode` placeholder so the
  scene graph remains loadable and renderable.
- Tests: `crates/director-core/tests/project_roundtrip.rs` covers each
  round-tripped node kind.

## Compile Phase & Dependency Graph

- `src/compiler.rs` — `DependencyGraph` and `DependencyInput` for incremental
  invalidation. Inputs: `Entity(EntityId)`, `AudioTrack(usize)`, `Node(NodeId)`.
- `Director.compile()` builds the graph from entity refs, audio bindings,
  parent-child hierarchy, and mask references. `Director.invalidate(inputs)` returns
  the transitive set of affected node IDs and stores it in `Director.dirty_nodes`.
- `render_at_time` uses `Director.dirty_nodes` to drive incremental work:
  `Director.update()` skips `Element::update` for nodes that are not dirty or
  ancestors of dirty nodes, and `render_recursive` skips re-rasterizing static
  subtrees that are not in the dirty set. Ancestors are included so that transform
  and layout context is still set up correctly for the dirty subtree.
- `render_export` calls `compile()` before the frame loop so the graph is available
  during export.
- Rhai API: `compile(movie)`, `invalidate_entity(movie, uuid)`,
  `invalidate_audio_track(movie, idx)`, `invalidate_node(movie, id)`,
  `dirty_nodes(movie)`, `compiler_edge_count(movie)`.

## Child DOX Index

- `src/node/AGENTS.md` — effect/shader system: the 6-place shader registration
  contract, SkSL conventions (resolution-relative sampling, Rec.709 luma), and
  the `api_catalog` → `EffectConfig` auto-generation rule.