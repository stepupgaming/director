# Caption system — Discord-style kinetic captions for short-form video

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
* `CaptionStyle` — visible defaults and motion configuration.
* `compute_slot_assignments()` — pre-laid Y-slot allocation per cue.
  Permanent for the lifetime of the cue. Adding/removing cues never
  moves an existing cue.
* Per-display-line `Paragraph` — each line is its own Skia Paragraph,
  laid out with **uniform `font_size`**. Every TextStyle run on a single
  paragraph shares the same font size, so glyph metrics never vary
  across active / future / spoken states.
* `find_global_active()` — exactly one `(cue_idx, word_idx)` pair
  at any time, picking the latest `start_sec`.

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
transform around the active word's center, applied with second
Paragraph containing only the active word, painted AFTER the base
paragraph. The base paragraph is not re-laid; the scale affects only
the active word's overlay. Scale returns to 1.0 within
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

## Two-line reserve

Cues can contain `\n` (multi-line SRT text) or wrap their words into
two display lines via the layout. Each cue's display lines render as
separate paragraphs stacked at the cue's pre-laid slot.

* **Line 0**: visible from `cue.start_sec` (instant reveal, 0.10 s ramp).
* **Line 1+**: visible from `cue.start_sec + offset_frac * cue_duration`
  (default 0.45) with `reveal_duration` (default 0.20 s) ease-out
  ramp, applied via `save_layer` alpha on the line's bbox.

When a cue enters or exits, the existing cue's lines never move.
Slot Y is computed once from the cue's pre-laid layout dimensions.
Consistent rendering across the entire cue lifetime.

## Drop shadow + auto-adapt

* All words carry a light drop shadow (offset `text_shadow_offset_x/y`,
  default `(0, 2)`) for any-background legibility.
* `auto_adapt_shadow` (default true) calls `Canvas::peek_pixels()` to
  sample bg luminance under the caption rect (`lum > 0.55` → black
  shadow; `lum < 0.40` → white shadow; mid → config-defaulted).

## Rhai props

* `font_size`, `color`, `highlight_color`, `spoken_color`
* `bottom_margin`, `top_margin`, `caption_position`, `align`
* `line_spacing`, `font_family`
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
* `max_overlapping_lines` — `test_max_overlapping_lines*`
* Slot stability — `test_slot_assignments_stable`
* Single global active word — `test_single_global_active_word`,
  `test_no_dual_yellow_at_word_boundary`
* Uniform font size across states — `test_word_font_size_uniform`,
  `test_no_ease_out_back_no_wobble_in_style`
* Line reveal schedule — `test_line_reveal_factor_step`
* Visual fixtures — `tests/visual/fixtures/captions/caption_word_highlight.rhai`
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
  and .github/workflows/ci.yml, then force-kept in .gitignore.
