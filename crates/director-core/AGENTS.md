# Purpose

- Own the Director engine: scene graph, timeline, layout, animation, Rhai API, captions, media, encoder and decoder wrappers, project I/O, and visual regression tests. `director-render` paints and exports frames of a `Director`; core integration tests paint through it as a dev-dependency.

## Ownership

- Sections below are the local contracts for each engine area: captions, bundled fonts, core hardening spines, video export and decoding, entities and smart captions, project round-trip, the dependency-graph compiler, the authoring stack, and named compositions.
- Normative cross-crate spines live in `docs/architecture/core-contracts.md`.

## Caption system: kinetic captions for short-form video

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
* `CaptionNode::measure` — content height from `build_block_layout`
  SkParagraph metrics at `max(layout_width_fraction * available_width, 20)`.
  Anchor margins belong to Taffy insets, not content height. Cached
  `block_layout` is reused when width + canvas height match.
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
* Roboto Regular/Bold/Italic, Inter Regular/Bold, and Poppins Bold are
  required; export fails via `ensure_required_fonts` when one is missing.
  The six files are committed; CI downloads nothing.
* Emoji fallback is loaded via DefaultAssetLoader::load_font_fallback
  (assets/fonts/emoji.ttf or DIRECTOR_EMOJI_FONT env var).
* Do NOT add host-system-font-dependent text paths; new caption/template
  fonts must be added to BUNDLED_FONTS and force-kept in .gitignore.

## Visual golden corpus

* `tests/visual_suite.rs` renders 40 frames across layout, text, images,
  masks, effects, shapes, animation, compositions, transitions, captions,
  templates and temporal sampling, plus a one-pixel harness self-test.
* Each frame first exports its scene and round-trips it through
  `director-scene` JSON. An export failure fails the frame.
* Goldens live in `tests/visual/golden/<module>/<name>.png`. A missing
  golden fails. `DIRECTOR_UPDATE_GOLDENS=1` rewrites goldens; review the PNG
  diff before committing.
* A pixel differs when any channel delta exceeds 16; a frame fails above a
  0.001 differing fraction. `assert_visual_match_with` overrides per test.
* Fonts and assets resolve from `CARGO_MANIFEST_DIR`, so the suite runs from
  any working directory.

## Core hardening

Engine solidity work is specified under `docs/plans/` (index `00`). Local
spines that this crate owns: time/timeline (`director`/`timeline`/`lifecycle`),
identity (`scene`/`types` — generational `NodeId`), frame pipeline
(compile, dirty sets, `static_cache`), animation (`animation.rs`), project model
(`project.rs`), determinism (fonts, video/audio clocks). Update
`docs/architecture/core-contracts.md` when those contracts change.

Identity (plan 02): `NodeId { index, generation }`; free-list reuse bumps
generation; Rhai `ensure_alive` errors on stale handles. `apply_effect`
rewrite returns wrapper id (attach-in-place when target is already
`EffectNode`). `set_mask` keeps mask id under exclusive ownership. Nested
composition ids are arena-local to the inner director.

Frame pipeline (plan 03): `note_mutation` auto-evicts static/comp caches;
`director-render` paints a full frame when uncompiled; dirty static nodes
never serve cache hits; `expand_dirty_set` includes reverse mask users.
A full pass reuses `static_cache` when `StaticCacheKey` matches: layout
width and height, the local transform baked into the snapshot, pivot,
inherited opacity, and time. Time is `None` when the subtree has no
time-varying paint, video, expression, or `Element::varies_with_time`
paint, so a later frame may hit. Authored SVG SMIL inside an HTML
element sets that flag.
Time is `Some(local_time)` otherwise, so a later frame misses. An
incremental pass still hits a clean static node without comparing the
key. Image-only caches (composition surfaces, captured HTML frames)
still miss on every full pass. Acceptance: `tests/pipeline_equivalence.rs`
and `tests/static_reuse.rs`.
`director-render::paint` owns the traversal and key; core owns
`StaticCacheKey`, `StaticCacheEntry` and the `static_cache` map.
`Element::shares_3d` defaults false and `view_depth` defaults to 0.
When `shares_3d` is true, `director-render` collects one depth list.
A direct child that also shares 3d contributes its descendants. Depth
sums `view_depth` along that chain. A non-sharing child stays one plane.
The list sorts by ascending depth, then z-index, then source order.
HtmlElement reports the Z of the element center after its own local
matrix. Parent perspective is not part of that Z. A plane whose Z
changes across its quad, including crossing `rotateX` cards, is
occluded per pixel. Constant Z stays on the center sort.
Composition clip order is unchanged.

Layout synchronization: compare child order including masks before calling Taffy
`set_children`; identical lists must not invalidate the layout cache. Compare
dirty styles before `set_style`; unchanged intrinsic-measure nodes still need
`mark_dirty` after content changes. Persistent layouts must match fresh layouts
after reordering, reparenting, mask edits, size changes, and generational reuse.
Taffy 0.9 passes a known size to the measure closure as the border box and the
available space as the content box. The closure hands `Element::measure` the
content size for a known axis, so padded text wraps at the width it paints at
(`director-html/tests/padded_text.rs`).
After the parallel `update` pass, `Director::update` calls `Element::pre_layout`
on every visited node, one at a time; `true` marks the node's style dirty, so a
measured node is measured again. HTML text reads the live sizes its atomic
inline boxes published while updating.
`run_post_layout` runs every `post_layout` first, then moves each node whose
`Element::layout_origin` returns a point to that origin in its parent's box.
HTML inline boxes read their slot from the text line a sibling laid out.

Performance tracking: `python -B crates/director-core/benches/frame_regression.py`
provides `record --output <baseline>`, `check --baseline <baseline> --output <run>`,
and offline `compare --baseline <baseline> --current <run> --output <report>`.
Each capture runs the existing Criterion frame workloads in an isolated output
directory and pairs 12 timing metrics with 15 exact RGBA frames. Existing output
directories are refused. Compare only the same machine/toolchain/build settings,
benchmark source digest, and suite metadata. Default timing allowance is 15%,
configurable with `--max-regression-percent`; fail when the current 95% lower
bound exceeds the baseline upper bound plus that allowance. Exit codes: 0 pass,
1 pixel/timing regression, 2 invalid capture/setup, 3 noisy/inconclusive timing.
Noisy results require a repeat, not baseline replacement. Reports are JSON and
Markdown; raw captures stay under ignored `target/`, not durable source.

Tracker verification: `python -B -m unittest discover -s
crates/director-core/benches -p 'test_*.py'`. The benchmark owns `suite.json` and
five 640x360 premultiplied RGBA frames per workload via
`DIRECTOR_PROFILE_PIXELS`. CLI-driven comparisons never auto-update baselines.

## Native video export

- `director-render` owns shutter sampling, the export frame loop and Vulkan readback (`director-render/AGENTS.md`). This crate owns the encoder, decoder and audio mix that export feeds.
- Export passes per-channel sample-frame counts to `mix_audio`, which returns interleaved stereo. The total output remains `duration_to_frame_count` × `audio_samples_per_video_frame(sr, fps, 2)`.
- Software x264 defaults to preset `slow` and CRF 18. `software_x264_quality(true)` is `ultrafast` and CRF 30. Hardware qp and the alpha VP9 settings stay put under the draft switch. `director_render::render_export` keeps draft off. Unit test: `draft_software_x264`.
- `video_wrapper::Encoder::new` streams audio per frame through `encode_audio` into a raw temp file, and `finish` muxes it in a second FFmpeg pass. `Encoder::with_audio_input` spawns FFmpeg with a second input mapped `0:v` and `1:a` that reads interleaved stereo f32 from a loopback TCP listener (`tcp://127.0.0.1:<port>`), so FFmpeg starts before the audio exists. `send_audio` hands the whole premixed track once to a feeder thread that accepts the FFmpeg connection and writes it; video frames may be encoded before and after. `finish` waits for FFmpeg and reports a feeder error; it aborts and errors when the track was never sent. `Encoder::with_audio` is `with_audio_input` plus `send_audio`. `encode_audio` errors on both. `Encoder::abort` kills FFmpeg, wakes a feeder still waiting to accept, and deletes its temp files and a partial single-pass destination. `director-render` exports through `with_audio_input`; acceptance is its `document_export_decodes_like_the_live_export`.
- End-to-end A/V acceptance is `cargo test -p director-cli --release --test integration html_export::`, with FFmpeg and ffprobe in PATH.
- Opaque export is full-range `yuv420p` tagged BT.709. FFmpeg 7.1+ takes primaries and transfer from the frames and ignores `-color_primaries` and `-color_trc`, so the encoder runs `setparams=color_primaries=bt709:color_trc=bt709` first. FFmpeg before 7.1 (Ubuntu 24.04 ships 6.1) converts RGB to limited-range BT.601 whatever `-color_range` and `-colorspace` say, so `scale=out_color_matrix=bt709:out_range=full` follows the setparams, and alpha export runs `scale=out_range=full`. Acceptance: `export_tags_bt709_primaries_and_transfer` for the tags and `export_samples_are_full_range_bt709_like_their_tags` for the samples, which decodes as full-range BT.709 and checks a red and a black patch.
- `FFmpegDriver::ensure_available` reads `libavcodec_version` and warns below 61.19 (FFmpeg 7.1). Git builds print `N-...` in place of a release number, so the libavcodec line is the only reliable version. Unit test: `reads_libavcodec_version_from_release_and_git_builds`.
- Alpha export (`--alpha`, lossless VP9 `yuva420p` at `-deadline realtime -cpu-used 8`, where the speed only changes file size) sets `Director::transparent_background`, so every frame clears to transparent, and reads unpremultiplied pixels because FFmpeg treats `rgba` input as straight alpha. Opaque export clears to black. Decode alpha WebM with `-c:v libvpx-vp9` before `-i` to keep the alpha plane. Acceptance: `alpha_export_keeps_empty_pixels_transparent_and_alpha_straight` in `crates/director-cli/tests/html_export.rs`.

## Video decoder lifecycle and verification

- `src/video_wrapper.rs`: `DecodeSession` owns its FFmpeg child and kills/reaps it on drop, including read errors and restarts. Drop the old session before spawning its replacement.
- `ThreadedDecoder` disconnects command and response channels before joining its worker so a full response queue cannot block shutdown. Session cleanup completes before decoder drop returns.
- Decoder unit tests require FFmpeg and ffprobe in PATH. Generate lossless, frame-numbered fixtures in temporary directories; fail on missing tools or fixture-generation errors instead of silently passing.
- Verify exact RGBA content, child PID reuse, eight-frame cache eviction, uncached backward and large forward seeks, EOF recovery, and worker shutdown. Cover 24, 25, 30000/1001, 30, and 60 FPS.
- Run `cargo test -p director-core --lib video_wrapper::tests` for decoder verification. Exact VFR presentation-timestamp sampling remains unsupported.
- `parse_video_size` reads the first two `x`-separated fields and `parse_frame_rate` the first comma field. A stream with side data, such as the ICC profile a screen recorder writes, makes ffprobe's CSV writer add an empty trailing field (`1920x1080x`, `30/1,`). Unit test: `reads_probe_output_with_a_side_data_field`.

## Semantic Entity Registry

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
- `Image`/`Video` preserve source path, object fit, object position, border
  radius, opacity, and render mode. Object position is a leftover-space
  fraction plus a pixel offset on each axis. The default is 50% 50% with no
  offset. A missing `object_position` key stays at that default. Media is
  reloaded from disk on load when available. Tests: `tests/object_position.rs`.
- `Lottie` embeds the authored Lottie JSON (`LottieNode::source`) and restores
  speed, loop flag, frame, and opacity.
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
- `director-render` frames use `Director.dirty_nodes` to drive incremental work:
  `Director.update()` skips `Element::update` for nodes that are not dirty or
  ancestors of dirty nodes, and the painter skips re-rasterizing static
  subtrees that are not in the dirty set. Ancestors are included so that transform
  and layout context is still set up correctly for the dirty subtree.
- `director_render::render_export` calls `compile()` before the frame loop so the graph is available
  during export.
- `compile()` also scores `style_policy`. Motion sampling reads `node.transform`
  composed with `Element::local_transform`, which elements that animate their own
  transform (HTML CSS keyframes) implement. Layout does not run while sampling, so
  `layout_rect` keeps its pre-layout size and percent lengths and origins resolve
  against it. Animated `left` and `top` do not count as motion.
- The audio policy meters each track as `AudioMixer::mix` plays it
  (`mixed_track_samples`): volume and constant-power pan in 10 ms blocks, cut at
  the track duration, effects left out. The decoder copies mono to both channels at
  full level and center pan plays each at 0.707, so a mono file exports at its own
  mono loudness. Tests: `audio_loudness_matches_the_mixed_export`,
  `audio_loudness_ignores_samples_past_the_track_duration`.
- `compile()` logs the violation list only when it differs from
  `last_policy_score`. The HTML frontend compiles and export compiles again, so an
  unchanged list prints once. Test: `compile_logs_an_unchanged_violation_list_once`
  in `tests/compiler.rs`.
- Rhai API: `compile(movie)`, `invalidate_entity(movie, uuid)`,
  `invalidate_audio_track(movie, idx)`, `invalidate_node(movie, id)`,
  `dirty_nodes(movie)`, `compiler_edge_count(movie)`.

## Authoring stack (L0–L3)

- **L0** — engine + raw Rhai (`animate`, nodes, timeline).
- **L1** — `src/scripting/api/stdlib.rs`: `fade_in`/`out`, `pop_in`, `fly_in_*`/`fly_out_*`, `pulse`, `pulse_on_beat`, `typewriter`, `ken_burns`. Catalogued in `RHAI_FUNCTION_CATALOG`.
- **L2** — `templates.rs` (`add_*` incl. `add_smart_lower_third`) + `register_composition` + shallow props. Catalogued helpers: `add_smart_lower_third`, `register_composition`.
- **L3** — `examples/recipes/*` (must register compositions): `faceless_short`, `kinetic_lower_third`, `image_beats` (media props), `stat_count`, `quote_card`, `list_reveal`, `end_card`, `chapter_card`, `countdown`. The `examples` test filter `example_recipes_` validates them. Plan 07 **Done at v1 stop-line** (`docs/plans/07-stdlib-and-recipes.md`).
- No separate `director-sdk` crate. TTS Historian, Podcast Clipper, full component library deferred.

## Authoring errors

- Unknown effect names, effect parameters, transition names, easing names, `add_box`/`add_text`/`set_style` keys, and blend modes fail the script with the valid names. They do not fall back.
- `add_audio`, `add_image`, `add_svg`, `add_lottie`, and `add_video` fail the script when the asset loader cannot read the path, or when image, audio, svg, or lottie bytes are empty or undecodable. The error names the path.
- `add_box` and `add_text` accept per-side `margin_top`, `margin_right`, `margin_bottom`, `margin_left` and the matching `padding_*` keys. A single `margin` or `padding` still sets all four sides.
- `set_layout_style` stores the new Taffy style on box, text, effect, shape, and composition nodes.
- Tests: `tests/fail_loud.rs`, `tests/fail_loud_catalog.rs`, `tests/quick_wins.rs`.

## Scene document

- `src/scene_export/` copies a live `Director` into `director_scene::SceneDocument`. `export_scene` returns `Result<SceneDocument, ExportError>`.
- Each element writes its own body through `Element::export_body`. The default returns `ExportError::Unsupported`, so a new element kind fails export until it gains a body. `Element::export_layout_style` supplies the recorded layout and defaults to `layout_style`; an element that animates its layout returns its unanimated style.
- `mod.rs` owns node records, channels, easing, gradients, blend modes, entities and `export_graph`, which exports one `SceneGraph`. `nodes.rs`, `caption.rs` and `effect.rs` own element bodies. `layout.rs`, `timeline.rs` and `audio.rs` own layout, timeline and mixer export.
- Downstream crates build their bodies from the public helpers: `channel`, `color_channel`, `rgba`, `export_easing`, `node_ref`, `blend_mode_name`, `export_layout`, `export_length`, `export_graph`, and `nodes::{box_body, text_export, image_asset, video_media, object_fit, object_position, text_align, text_shadow}`. `text_export` leaves out a prebuilt stroke paragraph for callers that rebuild it.
- `ExportContext` builds the asset table. Media with a source path become path assets; decoded pixels, raw samples and in-memory SVG are embedded as base64.
- `AssetManager::register_font_faces` installs `FontFace`s (family plus TrueType or OpenType bytes, such as HTML `@font-face` rules) on the font collection's dynamic font manager after the faces already registered and keeps them in `font_faces`. `export_scene` embeds each as a `font/ttf` asset in `SceneDocument::fonts`; a composition's nested document shares its parent's fonts and writes none. Import registers the document's faces before any node decodes.
- `AudioTrack::source_start` is the seconds of `source_path` skipped before the first decoded sample. A path asset exports it as `source_start`; embedded samples are already trimmed and export 0.
- Values the document cannot hold fail with `ExportError::Unrepresentable`, such as unresolved `calc()` lengths and text with a prebuilt stroke paragraph.
- Effects and transitions export under their primary `api_catalog` names.
- `src/scene_import/` is the inverse: `import_scene(document, loader, registry, render_mode)` rebuilds a live `Director`. An `ImportRegistry` maps a record `kind` to a `Decoder`; `ImportRegistry::core()` covers box, text, image, video, vector, shape, composition, effect, caption, smart caption, Lottie and spectrum visualizer, and frontends add their kinds through `register`. A kind with no decoder, an unknown audio effect kind and entity refs return `ImportError::Unsupported` (`is_unsupported` looks through node wrappers); malformed data returns `Invalid` or `Asset`.
- Nodes import in arena order with ids remapped by `(index, generation)`. Parent links come from `children` lists, and a recorded parent outside them (a mask under its owner) is restored. `import_layout` restores every element's Taffy style. Compositions import their nested document through `ImportContext::nested`, sharing the parent fonts. `ImportContext::element` decodes a body nested inside another element.
- `import_graph(records, scene, cx)` adds records with their node state and parent links to any `SceneGraph` and returns the `NodeIds` map that `resolve` reads; `import_scene` uses it for the root graph and frontends use it for nested graphs such as captures. `ImportContext::shared(key, make)` memoizes a value for one import, such as a shared video decoder.
- `audio.rs` rebuilds the mixer at the document sample rate with each track at its slot, empty slots kept. A path asset decodes through the loader with `source_start` seconds skipped; an `audio/x-f32le` asset holds the samples. Effects rebuild by kind (reverb, delay, eq with its filter, compressor) from their named parameters. Node audio bindings import with the node state.
- Lottie nodes export and import `LottieNode::source`, the authored JSON, because the `lottie-data` model serializes into a form its deserializer does not read. `LottieBody::images` carries the override images by Lottie asset id.
- `nodes::video_node` decodes a path asset in place through `AssetLoader::media_path` (default: the path when it names a file) and an embedded asset through a temporary file.
- Downstream decoders build on the public helpers: `animated`, `color_animated`, `animated_with`, `easing`, `color`, `gradient`, `blend_mode`, `length`, `effect_type`, `caption_node`, `smart_caption_node`, `resolve` and `nodes::{box_node, text_node, image, image_node, video_node, vector_node, shape_node, text_shadow, text_align, object_fit, object_position}`.
- The visual suite exports every golden scene, checks the JSON round trip and catalog names, renders the scene, then paints the document through `director_render::Renderer` (a dev-dependency) and compares that frame with the same golden. Failing document renders write `<name>.document.actual.png` and `.diff.png`; an unsupported kind skips the document render with a message.
- Tests: `tests/scene_document.rs` and `cargo test -p director-core --release --test integration visual_suite::`.

## Named compositions (Rhai)

- `src/scripting/api/compositions.rs` — `CompositionRegistry`, `register_composition`, shallow props merge, JSON listing.
- Hosts that need compositions call `register_rhai_api_with_compositions` (or `register_composition_api`) and keep the compiled AST alive for `FnPtr::call`.
- Base `register_rhai_api*` does **not** install the composition registry (opt-in per host).
- Acceptance: unit tests in `compositions.rs`; example `examples/features/compositions.rhai`; `tests/examples.rs` accepts Movie **or** registry builders.

## Child DOX Index

- `src/node/AGENTS.md` — caption measurement/render equivalence and the
  effect/shader system: 6-place registration, SkSL sampling/luma conventions,
  and `api_catalog` → `EffectConfig` auto-generation.
