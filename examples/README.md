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

#### Content sources (provide one of; a caption without cues fails)

| Prop | Type | Description |
|------|------|-------------|
| `media` | string | Audio or video file transcribed with Bark (`--features asr`); `model` and `graphs` pick the model. |
| `words` | array or string | Word-timed transcript: `asr transcribe --format json` output or `[{ text, start, end }]`. |
| `file` | string | Path to a `.json` transcript, `.vtt` or `.srt` file. |
| `source` | string | Raw SRT or WebVTT text. |
| `content` | array | Inline caption lines. Each item is a map with `text`, `start`, `end`, and optional `words` (array of `{ word, start, end }`). |

#### Placement

Captions span their parent's width and sit by `caption_position`. Keys outside the tables in this section fail the script with the valid list, and the CSS layout keys (`position`, `left`, `width` and the rest) fail with the caption key that replaces them. Numbers take ints or floats; colours take `#RGB`, `#RGBA`, `#RRGGBB` or `#RRGGBBAA`.

| Prop | Type | Description |
|------|------|-------------|
| `caption_position` | string | `bottom` (default), `top`, `center`, `left`, `right`, `top_left`, `top_right`, `bottom_left`, `bottom_right`. Line alignment follows it. |
| `bottom_margin` / `top_margin` / `left_margin` / `right_margin` | float | Frame pixels between the caption and that edge. |
| `safe_area` / `safe_area_preset` | string | Platform margins: `tiktok`, `youtube_shorts`, `instagram_reel` and the other `DesignSystem` presets. |
| `layout_width_fraction` | float | Text column width as a fraction of the caption width, 0.05 to 1. |
| `positions` | array | Placement track: maps with `at` (seconds) and any placement keys, each holding until the next. |

#### Caption styling

Every style key, with its type, range, default and doc, lives in the generated [caption reference](../docs/reference/captions.md) (`director-engine captions keys`). `preset` starts from a built-in look (`bold-outline`, `karaoke`, `pill`, `pop`, `subtitle`, `boxed`) and explicit keys override it. `size` sets the font size as a fraction of the frame height. `positions: [#{ at, caption_position, bottom_margin, ... }]` moves the caption at those times. `director-engine captions sheet -o looks.png` renders every preset on the same words. In HTML, `<captions src="words.json" preset="pop" highlight-color="#22C55E">` takes the same keys in kebab case with `<position at>` children ([`html/captions.html`](html/captions.html)).

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
cargo test -p director-video-core examples
```
