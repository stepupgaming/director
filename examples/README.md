# Director Engine Examples

Reference scripts demonstrating the Rhai API. All examples are validated by the test suite.

## Basics

| Script | Description |
|--------|-------------|
| [hello_world.rhai](basics/hello_world.rhai) | Minimal script - create movie, add scene, add text |
| [layout_flexbox.rhai](basics/layout_flexbox.rhai) | Flexbox layout with rows, columns, spacing |
| [animation.rhai](basics/animation.rhai) | Keyframe and spring animations |
| [text.rhai](basics/text.rhai) | Rich text, styling, shrink-to-fit |

## Features

| Script | Description |
|--------|-------------|
| [effects.rhai](features/effects.rhai) | Blur, grayscale, sepia, invert filters |
| [shaders_gallery.rhai](features/shaders_gallery.rhai) | Extended shader/effect library (warp, swirl, ripple, glass, shake, duotone, posterize, threshold, scanlines, glitch, edge_detect, zoom_blur, kaleidoscope) |
| [masking.rhai](features/masking.rhai) | Alpha masking and blend modes |
| [transitions.rhai](features/transitions.rhai) | Scene transitions (fade, slide, wipe) |
| [new_transitions.rhai](features/new_transitions.rhai) | New transitions (dissolve, flip, clock_wipe, cube) |
| [new_templates.rhai](features/new_templates.rhai) | Counter ticker, waveform visualizer, loop, freeze |
| [text_and_shapes.rhai](features/text_and_shapes.rhai) | Typography utilities, clip-path polygons, color interpolation, noise |
| [audio_spectrum_visualizer.rhai](features/audio_spectrum_visualizer.rhai) | FFT-driven audio-reactive spectrum bar visualizer |
| [z_index.rhai](features/z_index.rhai) | Z-index ordering with absolute positioning |
| [image.rhai](features/image.rhai) | Image loading and animation |

## Smart Captions

[smart_caption.rhai](features/smart_caption.rhai) demonstrates a caption that automatically avoids tracked faces by solving a temporal constraint problem over the shot.

### `add_smart_caption(parent_or_scene, props)`

Creates a `SmartCaptionNode`. The solver chooses the caption anchor (top, bottom, left, right, or continuous x/y) based on the position of tracked entities, so `caption_position` is ignored.

#### Content sources (provide one of)

| Prop | Type | Description |
|------|------|-------------|
| `file` | string | Path to an SRT subtitle file. |
| `source` | string | Raw SRT subtitle string. |
| `content` | array | Inline caption lines. Each item is a map with `text`, `start`, `end`, and optional `words` (array of `{ word, start, end }`). |

#### Layout & sizing

All standard layout props parsed by `parse_layout_style` are supported: `width`, `height`, `max_width`, `max_height`, `position`, `left`, `right`, `top`, `bottom`, `flex_direction`, `align_items`, `justify_content`, `flex_grow`, `flex_shrink`, `padding`, `margin`, `gap`, `display`, `grid_template_columns`, `grid_template_rows`, `grid_row`, `grid_column`, `aspect_ratio`.

| Prop | Type | Description |
|------|------|-------------|
| `position` | string | `"relative"` or `"absolute"`. Use `"absolute"` with `left`/`right`/`top`/`bottom` to fill the canvas. |
| `width` | string/number | e.g. `"100%"` or `1080`. |
| `height` | string/number | e.g. `"100%"` or `1920`. |

#### Caption styling

| Prop | Type | Description |
|------|------|-------------|
| `font_size` | float | Base font size. |
| `color` | string | Hex color, e.g. `"#FFFFFF"`. |
| `highlight_color` | string | Hex color for highlighted words. |
| `spoken_color` | string | Hex color for spoken words. |
| `align` | string | `"left"`, `"center"`, or `"right"`. |
| `text_direction` | string | `"ltr"`, `"rtl"`, or `"auto"`. |
| `bottom_margin` | float | Bottom safe-zone margin. |
| `top_margin` | float | Top safe-zone margin. |
| `font_family` | string | Font family name. |
| `font_weight` / `weight` | string/number | e.g. `"bold"` or `700`. |
| `stroke_enabled` | bool | Enable text stroke. |
| `stroke_width` | float | Stroke width. |
| `stroke_color` | string | Hex stroke color. |
| `glow_enabled` | bool | Enable glow effect. |
| `glow_blur` | float | Glow blur radius. |
| `line_spacing` | float | Line spacing multiplier. |
| `color_transition_duration` | float | Duration of color transitions. |
| `future_opacity` | float | Opacity of future words. |
| `spoken_opacity` | float | Opacity of spoken words. |
| `active_opacity` | float | Opacity of the active word. |
| `text_shadow_enabled` | bool | Enable text shadow. |
| `text_shadow_color` | string | Shadow color. |
| `text_shadow_blur` | float | Shadow blur. |
| `text_shadow_offset_x` | float | Shadow X offset. |
| `text_shadow_offset_y` | float | Shadow Y offset. |
| `active_pop_scale` | float | Pop scale for active word (clamped 1.0–1.10). |
| `active_pop_duration` | float | Pop duration (clamped 0.05–0.15 s). |
| `auto_adapt_shadow` | bool | Auto-adapt shadow color. |
| `pill_enabled` | bool | Draw a pill behind the caption. |
| `pill_color` | string | Hex pill color. |
| `pill_adaptive` | bool | Adapt pill color automatically. |
| `pill_padding_h` | float | Horizontal pill padding. |
| `pill_padding_v` | float | Vertical pill padding. |
| `pill_radius` | float | Pill corner radius. |
| `auto_phrase_break` | bool | Automatically break lines at phrase boundaries. |
| `max_words_per_line` | int | Maximum words per line. |
| `phrase_line_mode` | string | `"stack"` or `"replace"`. |
| `safe_area` / `safe_area_preset` | string | Safe-area preset name. |
| `scale_with_canvas` | bool | Scale font size with canvas height. |
| `reference_canvas_height` | float | Canvas height used for scaling. |
| `uppercase` | bool | Render text in uppercase. |

#### Smart solver tuning (simple props API)

These props are read directly by `add_smart_caption(parent_or_scene, props)`:

| Prop | Type | Description |
|------|------|-------------|
| `entity_refs` | array of strings | UUIDs of entities to avoid (faces, speakers, subjects). |
| `start_time` | float | Shot start in seconds (defaults to first caption line). |
| `end_time` | float | Shot end in seconds (defaults to last caption line). |
| `clearance` | float | Minimum clearance between caption and entity bbox in pixels. |
| `smoothing_window` | int | Smoothing window size in samples. |
| `motion_penalty` | float | Penalty for moving the caption (higher = smoother, less reactive). |
| `overlap_penalty` | float | Penalty for caption overlapping an entity. |
| `safe_zone_penalty` | float | Penalty for caption leaving the safe zone. |
| `sample_rate` | float | Solver samples per second. |

When using the `SemanticManifest` overload (see below), you can also tune `grid_steps`, `negative_space_weight`, and `smoothness_weight`.

#### Filters & ordering

| Prop | Type | Description |
|------|------|-------------|
| `filter_blur` | float | Blur radius. |
| `filter_brightness` | float | Brightness multiplier. |
| `filter_contrast` | float | Contrast multiplier. |
| `filter_saturation` | float | Saturation multiplier. |
| `filter_hue_rotate` | float | Hue rotation in degrees. |
| `z_index` | int | Render order. |

### Advanced overloads

You can also build the caption from a `SemanticManifest` and `SmartCaptionConfig`:

```rhai
let manifest = create_semantic_manifest(movie, #{
    entity_refs: [face_id],
    start_time: 0.0,
    end_time: 4.0,
    clearance: 40.0,
    motion_penalty: 0.0,
    overlap_penalty: 1000.0,
    safe_zone_penalty: 500.0,
    grid_steps: 16,
    negative_space_weight: 10.0,
    smoothness_weight: 0.5
});

let config = create_smart_caption_config(movie, #{
    canvas_w: 1080.0,
    canvas_h: 1920.0,
    caption_w: 800.0,
    caption_h: 100.0,
    safe_top: 60.0,
    safe_bottom: 110.0,
    safe_left: 60.0,
    safe_right: 60.0,
    sample_rate: 10.0,
    continuous: false
});

let smart_caps = root.add_smart_caption(manifest, config, #{
    source: "...",
    font_size: 48.0,
    color: "#FFFFFF"
});
```

Use `add_smart_caption_continuous(parent_or_scene, manifest, config, props)` to use the continuous (x, y) solver instead of discrete anchors.

## Running Examples

```bash
# Render to video
cargo run --release -- examples/basics/hello_world.rhai output.mp4

# Preview (if preview mode is implemented)
cargo run --release -- examples/basics/hello_world.rhai --preview
```

## Testing

All examples are validated by CI:

```bash
cargo test -p director-core examples
```
