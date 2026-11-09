# Director Rhai API Reference

## Global Functions

### `new_director(width, height, fps[, config])`
Initializes a new `Movie` object.
- `width`: Integer
- `height`: Integer
- `fps`: Integer
- `config`: (Optional) Map with `mode` ("preview" or "export")

## Movie Methods

### `add_scene(duration)`
Adds a new scene to the movie.
- `duration`: Float (seconds)
- Returns: `SceneHandle`

### `add_transition(from_scene, to_scene, type, duration, easing)`
Adds a transition between two scenes.
- `type`: "fade", "slide_left", "slide_right", "wipe_left", "wipe_right", "circle_open"
- `duration`: Float (seconds)
- `easing`: Easing string (e.g., "ease_in_out", "cubic_out")

### `add_audio(path)`
Adds a global audio track.
- `path`: String (asset path): an audio file or a video file. A video's first audio stream plays, so a phone or downloaded H.264 + AAC MP4 loads as is. Any audio FFmpeg reads works.
- Returns: `AudioTrackHandle`
- `add_video` paints picture only. To keep a clip's own sound, also call `add_audio` with the same path (`scene.add_audio(path)` plays it for that scene).

## Scene Methods

### `add_box(props)`
Adds a container node to the scene root.
- `props`: Map of properties
- Returns: `NodeHandle`

### `add_text(props)`
Adds a text node to the scene root.
- `props`: Map of properties (including `content`)
- Returns: `NodeHandle`

### `add_caption(props)`
Adds word-highlight captions (also a node method). Each word's highlight starts on the first frame at or after its start time. Input, first match wins:
- `media`: a video or audio file. Speech to text runs on it with the Bark 3.0 model `model` (`bark-3.0`, `bark-3.0-plus`, `bark-3.0-clean`; default: the first installed) and graph set `graphs` (`int8` or `exact`). Needs `director-engine` built with `--features asr` and a model from `director-engine asr fetch`. Transcripts cache by file hash under the model cache.
- `words`: an array of `#{ text, start, end }` in seconds.
- `file`: a `.json` transcript (`asr transcribe --format json`, or an array of `{text, start, end}`), a `.vtt` file (inline `<00:00:01.200>` word timestamps set word starts, as `asr transcribe --format vtt` writes), or a `.srt` file. SRT holds cue times only, so its words split each cue evenly: an estimate.
- `source`: SRT or WebVTT text.
- `content`: `[#{ text, start, end, words: [#{ text, start, end }] }]`. Cues without `words` split evenly.

`media`, `words` and JSON group words into cues by `max_cue_chars` (42), `max_cue_duration` (5.0 s), `max_cue_gap` (0.8 s) and `break_on_sentence` (true). `auto_phrase_break` with `max_words_per_line` splits cues into display lines and keeps measured word times. Style keys include `font_size`, `font_family`, `uppercase`, `color`, `highlight_color`, `spoken_color`, `color_transition_duration` (seconds of highlight lead-in, default 0), `pill_enabled` and `caption_position`.

Video to captions:
```rhai
root.add_video("clip.mp4", #{ width: "100%", height: "100%", position: "absolute" });
movie.add_audio("clip.mp4");
root.add_caption(#{ media: "clip.mp4", model: "bark-3.0", auto_phrase_break: true, max_words_per_line: 4 });
// Or transcribe once: director-engine asr transcribe clip.mp4 --format json -o words.json
// root.add_caption(#{ file: "words.json", ... });
```
Check timing with `director-engine qa movie.rhai --words words.json`.

## Node Methods

### `add_box(props)` / `add_text(props)` / `add_image(path[, props])`
Adds a child node to the current node.

### `animate(property, start, end, duration, easing[, delay])`
Adds a keyframe animation segment.
- `property`: "x", "y", "scale", "rotation", "opacity", "blur", etc.
- `delay`: (Optional) Float (seconds) to wait before starting.

### `spring(property, start, end, config)`
Adds a physics-based spring animation.
- `config`: Map with `stiffness` and `damping`

### `add_animator(start_idx, end_idx, property, start, end, duration, easing[, stagger])`
(Text only) Adds a per-character animator.

### `apply_effect(name[, value/map])`
Applies a visual effect to a node. Named (no arg): grayscale, sepia, invert. Float: contrast, brightness, saturation, exposure, hue_rotate, blur, temperature. Map: shader, directional_blur, grain, vignette, chromatic_aberration, pixelate, sharpen, glow, warp, swirl, ripple, glass, shake, duotone, posterize, threshold, scanlines, glitch, edge_detect, zoom_blur, kaleidoscope.

### `set_style(props)`
Updates layout and visual style properties.

## Common Properties (Props Map)
- `width`, `height`: Number or percentage (e.g., "100%")
- `bg_color`, `color`: Hex string (e.g., "#ffffff") or RGBA (e.g., "rgba(0,0,0,0.5)")
- `flex_direction`: "row", "column"
- `justify_content`, `align_items`: "center", "flex_start", "flex_end", "space_between"
- `gap`, `padding`, `margin`: Number
- `border_radius`, `border_width`, `border_color`: Styling
- `z_index`: Integer
- `opacity`: 0.0 to 1.0
- `object_fit`: "cover", "contain", "fill"
