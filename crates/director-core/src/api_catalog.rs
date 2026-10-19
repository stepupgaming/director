//! Canonical Director API catalog — single source of truth for Rhai bindings,
//! introspection (`registry`), and JSON schema generation (`director-schema`).
//!
//! Effect `name` values are the primary strings accepted by `apply_effect(name, ...)`.
//! Transition `name` values match `add_transition(..., type_str, ...)`.
//! `aliases` lists additional strings the Rhai layer accepts for the same capability.

use serde::Serialize;

/// API manifest version; bump when catalog shape or semantics change.
pub const API_MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct NodeCatalogEntry {
    pub name: &'static str,
    pub description: &'static str,
    pub supports_children: bool,
    pub animatable_properties: &'static [&'static str],
    pub features: &'static [&'static str],
    /// `NodeKind` serde tag when `json_schema` is true (snake_case).
    pub schema_tag: &'static str,
    /// `true` when representable as a `NodeKind` variant in `MovieRequest` JSON.
    pub json_schema: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct EffectCatalogEntry {
    /// Primary `apply_effect` name (snake_case).
    pub name: &'static str,
    /// `EffectConfig` / JSON schema serde tag (snake_case).
    pub schema_tag: &'static str,
    pub description: &'static str,
    pub params: &'static [(&'static str, &'static str)],
    /// Extra Rhai aliases (not exported as separate schema variants).
    pub aliases: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct TransitionCatalogEntry {
    /// Primary transition type string (snake_case).
    pub name: &'static str,
    pub description: &'static str,
    pub params: &'static [(&'static str, &'static str)],
    pub aliases: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct EasingCatalogEntry {
    pub name: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct RhaiFunctionEntry {
    pub name: &'static str,
    pub description: &'static str,
    pub params: &'static [(&'static str, &'static str)],
}

pub const NODE_CATALOG: &[NodeCatalogEntry] = &[
    node(
        "Box",
        "box",
        "Rectangular container with optional border radius and shadow",
        true,
        &[
            "opacity", "x", "y", "scale", "rotation", "bg_color", "border_radius",
        ],
        &["flexbox_layout", "grid_layout"],
        true,
    ),
    node(
        "Text",
        "text",
        "Rich text with font styling and per-glyph animation",
        false,
        &[
            "opacity", "x", "y", "scale", "rotation", "font_size", "color",
        ],
        &["per_glyph_animation", "text_animators", "typewriter", "text_direction"],
        true,
    ),
    node(
        "Image",
        "image",
        "Static image with object-fit options",
        false,
        &["opacity", "x", "y", "scale", "rotation"],
        &["object_fit", "lazy_loading"],
        true,
    ),
    node(
        "Video",
        "video",
        "Video playback with frame stepping",
        false,
        &["opacity", "x", "y", "scale", "rotation", "volume"],
        &["frame_accurate_seeking", "audio_extraction", "loop"],
        true,
    ),
    node(
        "Lottie",
        "lottie",
        "Lottie JSON animation playback",
        false,
        &["opacity", "x", "y", "scale", "rotation", "speed"],
        &["gpu_accelerated", "dynamic_speed", "frame_control"],
        true,
    ),
    node(
        "Vector",
        "vector",
        "SVG vector graphics",
        false,
        &["opacity", "x", "y", "scale", "rotation"],
        &["path_animation", "dynamic_colors"],
        true,
    ),
    node(
        "Effect",
        "effect",
        "Visual effect wrapper (blur, shaders, color matrix)",
        true,
        &["sigma", "blur", "intensity"],
        &["effect_chaining", "sksl_runtime_shader"],
        true,
    ),
    node(
        "Composition",
        "composition",
        "Nested timeline (pre-comp)",
        true,
        &["opacity", "x", "y", "scale", "rotation"],
        &["time_remapping", "nested_scenes", "independent_timeline"],
        true,
    ),
    node(
        "Caption",
        "caption",
        "Timed kinetic captions (SRT/VTT) with per-word highlight",
        false,
        &["opacity", "font_size"],
        &["srt_import", "vtt_import", "auto_caption", "text_direction", "rtl"],
        true,
    ),
    node(
        "SmartCaption",
        "smart_caption",
        "Caption that follows a semantic trajectory to avoid faces/subjects",
        false,
        &["opacity", "font_size"],
        &["semantic_trajectory", "face_avoidance", "temporal_solver", "entity_refs"],
        false,
    ),
];

pub const EFFECT_CATALOG: &[EffectCatalogEntry] = &[
    e("blur", "blur", "Gaussian blur", &[("sigma", "f32")], &[]),
    e(
        "drop_shadow",
        "drop_shadow",
        "Drop shadow with offset and color",
        &[
            ("blur", "f32"),
            ("offset_x", "f32"),
            ("offset_y", "f32"),
            ("color", "Color"),
        ],
        &[],
    ),
    e(
        "color_matrix",
        "color_matrix",
        "4x5 color transformation matrix",
        &[("matrix", "[f32; 20]")],
        &[],
    ),
    e("grayscale", "grayscale", "Desaturate to grayscale", &[], &[]),
    e("sepia", "sepia", "Sepia tone preset", &[], &[]),
    e("invert", "invert", "Invert RGB channels", &[], &[]),
    e("contrast", "contrast", "Adjust contrast", &[("value", "f32")], &[]),
    e("brightness", "brightness", "Adjust brightness", &[("value", "f32")], &[]),
    e("saturation", "saturation", "Adjust saturation", &[("value", "f32")], &[]),
    e("exposure", "exposure", "Adjust exposure", &[("value", "f32")], &[]),
    e("hue_rotate", "hue_rotate", "Rotate hue", &[("angle", "f32")], &["hue"]),
    e(
        "directional_blur",
        "directional_blur",
        "Motion blur in a direction",
        &[("strength", "f32"), ("angle", "f32"), ("samples", "u32")],
        &["motion_blur"],
    ),
    e(
        "grain",
        "grain",
        "Film grain overlay",
        &[("intensity", "f32"), ("size", "f32")],
        &["film_grain", "noise"],
    ),
    e(
        "temperature",
        "temperature",
        "Cool/warm color shift",
        &[("warmth", "f32")],
        &["warmth"],
    ),
    e(
        "tint",
        "tint",
        "Solid color tint blend",
        &[("color", "Color"), ("amount", "f32")],
        &[],
    ),
    e(
        "vignette",
        "vignette",
        "Edge darkening vignette",
        &[("intensity", "f32"), ("radius", "f32"), ("feather", "f32")],
        &[],
    ),
    e(
        "chromatic_aberration",
        "chromatic_aberration",
        "RGB channel offset",
        &[("strength", "f32")],
        &["chromatic", "ca"],
    ),
    e("pixelate", "pixelate", "Blocky pixelation", &[("block_size", "f32")], &[]),
    e("sharpen", "sharpen", "Unsharp mask sharpen", &[("strength", "f32")], &[]),
    e(
        "glow",
        "glow",
        "Bright-area glow / bloom",
        &[("intensity", "f32"), ("threshold", "f32")],
        &["bloom"],
    ),
    e(
        "warp",
        "warp",
        "Sine-wave displacement distortion",
        &[
            ("amplitude", "f32"),
            ("frequency", "f32"),
            ("direction", "f32"),
            ("speed", "f32"),
        ],
        &["wave_distort"],
    ),
    e(
        "swirl",
        "swirl",
        "Rotational twist around center",
        &[("radius", "f32"), ("angle", "f32")],
        &[],
    ),
    e(
        "ripple",
        "ripple",
        "Concentric waves from center",
        &[("amplitude", "f32"), ("frequency", "f32"), ("speed", "f32")],
        &[],
    ),
    e(
        "glass",
        "glass",
        "Frosted-glass refraction",
        &[("scale", "f32"), ("intensity", "f32")],
        &["frosted_glass"],
    ),
    e(
        "shake",
        "shake",
        "Block-based jitter displacement",
        &[("intensity", "f32"), ("speed", "f32")],
        &["jitter"],
    ),
    e(
        "duotone",
        "duotone",
        "Two-color luminance grade",
        &[("shadow", "Color"), ("highlight", "Color")],
        &[],
    ),
    e("posterize", "posterize", "Reduce color levels", &[("levels", "f32")], &[]),
    e("threshold", "threshold", "Black/white luminance cutoff", &[("level", "f32")], &[]),
    e(
        "scanlines",
        "scanlines",
        "CRT scanline overlay",
        &[("intensity", "f32"), ("count", "f32")],
        &["crt"],
    ),
    e(
        "glitch",
        "glitch",
        "RGB split with block displacement",
        &[("intensity", "f32"), ("speed", "f32")],
        &[],
    ),
    e(
        "edge_detect",
        "edge_detect",
        "Sobel edge outline",
        &[("intensity", "f32")],
        &["edges"],
    ),
    e("zoom_blur", "zoom_blur", "Radial motion blur from center", &[("strength", "f32")], &[]),
    e(
        "kaleidoscope",
        "kaleidoscope",
        "Mirror symmetry segments",
        &[("segments", "f32")],
        &["kaleido"],
    ),
    e("thermal", "thermal", "Heat-map color spectrum", &[], &["heatmap"]),
    e(
        "liquid",
        "liquid",
        "Bidirectional sine/cosine coordinate warp",
        &[("frequency", "f32"), ("amplitude", "f32")],
        &[],
    ),
    e(
        "acid",
        "acid",
        "Fluorescent acid melt distortion",
        &[("color", "Color"), ("speed", "f32")],
        &[],
    ),
    e("halftone", "halftone", "Pop-art dot grid", &[("dot_size", "f32")], &[]),
    e(
        "ascii",
        "ascii",
        "Terminal ASCII art effect",
        &[("scale", "f32"), ("color", "Color")],
        &[],
    ),
    e("sketch", "sketch", "Charcoal pencil sketch", &[("contrast", "f32")], &["pencil"]),
    e("mirror", "mirror", "Quad-mirror symmetry", &[], &["quad_mirror"]),
    e(
        "tritone",
        "tri_tone",
        "Three-color luminance grade",
        &[
            ("shadow", "Color"),
            ("mid", "Color"),
            ("highlight", "Color"),
            ("midpoint", "f32"),
        ],
        &["tri_tone"],
    ),
    e(
        "vhs",
        "vhs",
        "Combined CRT glitch",
        &[("distortion", "f32"), ("scanline", "f32")],
        &["crt_glitch"],
    ),
    e(
        "neon_edge",
        "neon_edge",
        "Edge detect with colored glow",
        &[("glow_color", "Color"), ("threshold", "f32")],
        &["neon_edge_detect"],
    ),
    e(
        "fisheye",
        "fisheye",
        "Radial lens magnification",
        &[("strength", "f32")],
        &["fish_eye"],
    ),
    e(
        "bulge",
        "bulge",
        "Center bulge/pinch distortion",
        &[("strength", "f32"), ("radius", "f32")],
        &[],
    ),
    e(
        "barrel",
        "barrel",
        "Barrel lens distortion",
        &[("curvature", "f32")],
        &["pincushion"],
    ),
    e(
        "lens_flare",
        "lens_flare",
        "Additive lens flare",
        &[("intensity", "f32"), ("color", "Color")],
        &["flare"],
    ),
    e("noir", "noir", "High-contrast monochrome", &[("contrast", "f32")], &["mono"]),
    e(
        "cyberpunk",
        "cyberpunk",
        "Magenta/cyan color grade",
        &[("intensity", "f32")],
        &[],
    ),
    e("vintage", "vintage", "Faded warm film grade", &[("amount", "f32")], &["retro"]),
    e("gameboy", "gameboy", "4-tone green palette", &[], &[]),
    e(
        "bleach_bypass",
        "bleach_bypass",
        "Desaturated high-contrast film",
        &[("amount", "f32")],
        &["bleach"],
    ),
    e(
        "teal_orange",
        "teal_orange",
        "Cinematic teal/orange grade",
        &[("amount", "f32")],
        &["cinematic"],
    ),
    e(
        "plasma",
        "plasma",
        "Animated color plasma",
        &[("scale", "f32"), ("speed", "f32")],
        &[],
    ),
    e(
        "hologram",
        "hologram",
        "Chromatic scanline hologram",
        &[("intensity", "f32")],
        &["holo"],
    ),
    e(
        "emboss",
        "emboss",
        "Directional emboss",
        &[("angle", "f32"), ("strength", "f32")],
        &[],
    ),
    e(
        "toon",
        "toon",
        "Cel-shaded posterize with outline",
        &[("levels", "f32"), ("edge", "f32")],
        &["cel"],
    ),
    e(
        "oil_paint",
        "oil_paint",
        "Quantized oil-paint blocks",
        &[("block_size", "f32"), ("levels", "f32")],
        &["oil"],
    ),
    e(
        "light_leak",
        "light_leak",
        "Warm radial light leak",
        &[("intensity", "f32"), ("color", "Color")],
        &["leak"],
    ),
    e(
        "aurora",
        "aurora",
        "Animated aurora bands",
        &[("intensity", "f32"), ("speed", "f32")],
        &[],
    ),
    e(
        "sunrays",
        "sunrays",
        "Radial light rays",
        &[("intensity", "f32"), ("angle", "f32"), ("speed", "f32")],
        &["rays", "godrays"],
    ),
    e(
        "skin_smooth",
        "skin_smooth",
        "Edge-preserving portrait smoothing",
        &[("intensity", "f32"), ("radius", "f32")],
        &["beauty_smooth"],
    ),
    e(
        "beauty_glow",
        "beauty_glow",
        "Soft diffuse glow on bright areas",
        &[("intensity", "f32"), ("threshold", "f32")],
        &[],
    ),
    e("soft_focus", "soft_focus", "Dreamy diffusion blur", &[("amount", "f32")], &[]),
    e("vibrance", "vibrance", "Selective saturation boost", &[("amount", "f32")], &[]),
    e(
        "fog",
        "fog",
        "Ground-hugging atmospheric mist",
        &[("intensity", "f32"), ("color", "Color")],
        &[],
    ),
    e(
        "mist",
        "mist",
        "Animated drifting haze",
        &[("intensity", "f32"), ("speed", "f32")],
        &[],
    ),
    e(
        "rain",
        "rain",
        "Animated slanted rain",
        &[("intensity", "f32"), ("speed", "f32")],
        &[],
    ),
    e(
        "snow",
        "snow",
        "Animated drifting snow",
        &[("intensity", "f32"), ("speed", "f32"), ("size", "f32")],
        &[],
    ),
    e(
        "dust",
        "dust",
        "Floating bokeh dust motes",
        &[("intensity", "f32"), ("speed", "f32")],
        &[],
    ),
    e(
        "bokeh_dots",
        "bokeh_dots",
        "Decorative out-of-focus circles",
        &[("intensity", "f32"), ("size", "f32"), ("color", "Color")],
        &["bokeh"],
    ),
    e(
        "film_look",
        "film_look",
        "Film-stock emulation",
        &[("amount", "f32"), ("grain", "f32")],
        &[],
    ),
    e(
        "orange_teal",
        "orange_teal",
        "Split-tone teal/orange grade",
        &[("amount", "f32")],
        &["orange_and_teal"],
    ),
    e(
        "cross_process",
        "cross_process",
        "Lomo cross-process grade",
        &[("amount", "f32")],
        &["crossprocess", "lomo"],
    ),
    e("polaroid", "polaroid", "Instant-film warm wash", &[("amount", "f32")], &[]),
    e("faded_film", "faded_film", "Matte low-contrast wash", &[("amount", "f32")], &[]),
    e(
        "midnight",
        "midnight",
        "Cool blue-night grade",
        &[("amount", "f32")],
        &["night"],
    ),
    e(
        "watercolor",
        "watercolor",
        "Soft quantized washes",
        &[("intensity", "f32"), ("edge", "f32")],
        &[],
    ),
    e(
        "crosshatch",
        "crosshatch",
        "Progressive ink hatching",
        &[("intensity", "f32"), ("density", "f32")],
        &[],
    ),
    e(
        "pointillism",
        "pointillism",
        "Painted-dot stipple",
        &[("dot_size", "f32"), ("intensity", "f32")],
        &[],
    ),
    e("mosaic", "mosaic", "Square tile grid", &[("tile_size", "f32")], &[]),
    e(
        "ink",
        "ink",
        "Bold black ink outline",
        &[("threshold", "f32"), ("intensity", "f32")],
        &[],
    ),
    e(
        "pencil_color",
        "pencil_color",
        "Colored-pencil sketch",
        &[("intensity", "f32")],
        &["colored_pencil"],
    ),
    e(
        "shader",
        "sksl_shader",
        "Custom SkSL runtime shader",
        &[("code", "String"), ("uniforms", "Map")],
        &[],
    ),
];

pub const TRANSITION_CATALOG: &[TransitionCatalogEntry] = &[
    t("fade", "Cross-dissolve between scenes", &[], &[]),
    t("slide_left", "Slide outgoing scene left", &[], &["slide-left"]),
    t("slide_right", "Slide outgoing scene right", &[], &["slide-right"]),
    t("wipe_left", "Wipe revealing new scene from right", &[], &["wipe-left"]),
    t("wipe_right", "Wipe revealing new scene from left", &[], &["wipe-right"]),
    t("circle_open", "Circular reveal from center", &[], &["circle-open"]),
    t(
        "wave",
        "Wave distortion with amplitude and frequency",
        &[("amplitude", "f32"), ("frequency", "f32")],
        &[],
    ),
    t("glitch", "Glitch/scanline distortion", &[("intensity", "f32")], &[]),
    t(
        "iris",
        "Iris/circular wipe transition",
        &[("start_radius", "f32"), ("end_radius", "f32")],
        &[],
    ),
    t("spiral", "Spiral transition", &[("rotations", "f32")], &[]),
    t("zoom_in", "Zoom in from center", &[], &["zoom-in", "zoomin"]),
    t("radial_wipe", "Radial clockwise wipe", &[], &["radial-wipe", "radialwipe"]),
    t(
        "pixelate_dissolve",
        "Pixel grid dissolve",
        &[],
        &["pixelate", "pixelate-dissolve", "pixelate_dissolve"],
    ),
    t("cross_zoom", "Cross zoom between scenes", &[], &["cross-zoom", "crosszoom"]),
    t("wind", "Horizontal sweep with shear", &[], &[]),
    t("burn", "Heat-edge burn reveal", &[], &[]),
    t("dissolve", "Dreamy blurred dissolve", &[], &[]),
    t("flip", "3D card flip", &[], &[]),
    t("clock_wipe", "Radial sweep from 12 o'clock", &[], &["clock-wipe", "clockwipe"]),
    t("cube", "3D cube rotation", &[], &[]),
];

pub const EASING_CATALOG: &[EasingCatalogEntry] = &[
    ease("linear", "Constant speed, no acceleration"),
    ease("ease_in", "Start slow, accelerate"),
    ease("ease_out", "Start fast, decelerate"),
    ease("ease_in_out", "Slow start and end, fast middle"),
    ease("bounce_out", "Bounce at the end"),
    ease("bounce_in", "Bounce at the start"),
    ease("bounce_in_out", "Bounce at both ends"),
    ease("elastic_out", "Elastic overshoot at end"),
    ease("elastic_in", "Elastic anticipation at start"),
    ease("elastic_in_out", "Elastic at both ends"),
    ease("back_out", "Slight overshoot"),
    ease("back_in", "Slight anticipation"),
    ease("back_in_out", "Anticipation and overshoot"),
    ease("hold", "Hold value until end of segment"),
    ease("cubic_bezier", "Custom cubic bezier: cubic_bezier(x1,y1,x2,y2)"),
    ease("expr", "Per-frame math expression: expr(sin(t*pi))"),
];

/// Rhai lifecycle / authoring helpers not represented as scene-graph nodes.
pub const RHAI_FUNCTION_CATALOG: &[RhaiFunctionEntry] = &[
    rhai_fn("add_loop", "Loop a scene N times on the movie timeline (JSON: Scene.loop_count)", &[]),
    rhai_fn("add_freeze", "Hold a scene frame for an extra duration (JSON: Scene.post_hold)", &[]),
    rhai_fn("save_project", "Serialize the live Director graph to versioned JSON", &[]),
    rhai_fn("load_project", "Load a versioned project JSON into a MovieHandle", &[]),
    rhai_fn(
        "apply_effect",
        "Attach a visual effect to a node by catalog name (method on NodeHandle; optional second argument is a value or parameter map)",
        &[("name", "String"), ("args", "ValueOrMap")],
    ),
    rhai_fn("compile", "Build the dependency graph before export", &[]),
    rhai_fn(
        "invalidate_entity",
        "Mark nodes depending on an entity as dirty and return affected node IDs",
        &[("entity_id", "String")],
    ),
    rhai_fn(
        "invalidate_audio_track",
        "Mark nodes depending on an audio track as dirty and return affected node IDs",
        &[("track_id", "Integer")],
    ),
    rhai_fn(
        "invalidate_node",
        "Mark nodes depending on a node as dirty and return affected node IDs",
        &[("node_id", "Integer")],
    ),
    rhai_fn(
        "compiler_inputs_for_node",
        "List the dependency inputs declared by a node",
        &[("node_id", "Integer")],
    ),
    rhai_fn(
        "compiler_dependents_of_entity",
        "List node IDs that depend on the given entity",
        &[("entity_id", "String")],
    ),
    rhai_fn(
        "compiler_dependents_of_audio_track",
        "List node IDs that depend on the given audio track",
        &[("track_id", "Integer")],
    ),
    rhai_fn(
        "compiler_dependents_of_node",
        "List node IDs that depend on the given node",
        &[("node_id", "Integer")],
    ),
    rhai_fn("compiler_graph_json", "Dump the full dependency graph as a JSON string for debugging", &[]),
    rhai_fn(
        "compiler_affected_nodes",
        "Return the transitive set of node IDs affected by the given dependency inputs without marking anything dirty",
        &[("inputs", "Array")],
    ),
    // --- Plan 07 L1 motion stdlib (methods on NodeHandle) ---
    rhai_fn("fade_in", "L1: opacity 0→1 (Node method; optional delay)", &[("dur", "Float"), ("delay", "Float?")]),
    rhai_fn("fade_out", "L1: opacity 1→0 (Node method; optional delay)", &[("dur", "Float"), ("delay", "Float?")]),
    rhai_fn("pop_in", "L1: scale+opacity bounce entrance (Node method)", &[("dur", "Float"), ("delay", "Float?")]),
    rhai_fn("fly_in_up", "L1: translate_y + fade entrance from below", &[("dur", "Float"), ("delay", "Float?"), ("px", "Float?")]),
    rhai_fn("fly_in_down", "L1: translate_y + fade entrance from above", &[("dur", "Float"), ("delay", "Float?"), ("px", "Float?")]),
    rhai_fn("fly_in_left", "L1: translate_x + fade entrance from right", &[("dur", "Float"), ("delay", "Float?"), ("px", "Float?")]),
    rhai_fn("fly_in_right", "L1: translate_x + fade entrance from left", &[("dur", "Float"), ("delay", "Float?"), ("px", "Float?")]),
    rhai_fn("fly_out_up", "L1: translate_y + fade exit upward", &[("dur", "Float"), ("delay", "Float?"), ("px", "Float?")]),
    rhai_fn("fly_out_down", "L1: translate_y + fade exit downward", &[("dur", "Float"), ("delay", "Float?"), ("px", "Float?")]),
    rhai_fn("fly_out_left", "L1: translate_x + fade exit left", &[("dur", "Float"), ("delay", "Float?"), ("px", "Float?")]),
    rhai_fn("fly_out_right", "L1: translate_x + fade exit right", &[("dur", "Float"), ("delay", "Float?"), ("px", "Float?")]),
    rhai_fn("pulse", "L1: single scale pulse", &[("dur", "Float"), ("delay", "Float?"), ("peak", "Float?")]),
    rhai_fn(
        "pulse_on_beat",
        "L1: scale pulses locked to BPM for a duration",
        &[("bpm", "Float"), ("duration", "Float"), ("peak", "Float?")],
    ),
    rhai_fn(
        "typewriter",
        "L1: TextNode glyph opacity stagger reveal (Text only)",
        &[("dur", "Float"), ("delay", "Float?")],
    ),
    rhai_fn(
        "ken_burns",
        "L1: slow zoom + drift (images/boxes)",
        &[("dur", "Float"), ("zoom", "Float?"), ("drift_x", "Float?"), ("drift_y", "Float?")],
    ),
    // --- Plan 07 L2 ---
    rhai_fn(
        "add_smart_lower_third",
        "L2: content-reactive name/title lower third (Scene method)",
        &[("props", "Map")],
    ),
    rhai_fn(
        "register_composition",
        "L2/L3: register a named composition builder with meta + default_props",
        &[("name", "String"), ("meta", "Map"), ("builder", "Fn")],
    ),
];

#[derive(Debug, Clone, Serialize)]
pub struct ApiManifest {
    pub version: u32,
    pub nodes: Vec<NodeCatalogEntry>,
    pub effects: Vec<EffectCatalogEntry>,
    pub transitions: Vec<TransitionCatalogEntry>,
    pub easings: Vec<EasingCatalogEntry>,
    pub rhai_functions: Vec<RhaiFunctionEntry>,
}

/// Build the full API manifest from the static catalog.
pub fn api_manifest() -> ApiManifest {
    ApiManifest {
        version: API_MANIFEST_VERSION,
        nodes: NODE_CATALOG.to_vec(),
        effects: EFFECT_CATALOG.to_vec(),
        transitions: TRANSITION_CATALOG.to_vec(),
        easings: EASING_CATALOG.to_vec(),
        rhai_functions: RHAI_FUNCTION_CATALOG.to_vec(),
    }
}

/// JSON manifest for agents, docs, and schema drift checks.
pub fn api_manifest_json() -> String {
    serde_json::to_string_pretty(&api_manifest()).expect("ApiManifest must serialize")
}

/// Primary effect names accepted by `apply_effect` (one per catalog entry).
pub fn catalog_effect_names() -> Vec<&'static str> {
    EFFECT_CATALOG.iter().map(|e| e.name).collect()
}

/// Catalog entry for a primary effect name or a documented alias.
pub fn find_effect(name: &str) -> Option<&'static EffectCatalogEntry> {
    EFFECT_CATALOG.iter().find(|entry| effect_name_matches(entry, name))
}

/// Catalog entry for a primary transition name or a documented alias.
pub fn find_transition(name: &str) -> Option<&'static TransitionCatalogEntry> {
    TRANSITION_CATALOG
        .iter()
        .find(|entry| entry.name == name || entry.aliases.contains(&name))
}

fn effect_name_matches(entry: &EffectCatalogEntry, name: &str) -> bool {
    entry.name == name || entry.aliases.contains(&name)
}

fn effect_labels() -> Vec<&'static str> {
    let mut labels = Vec::new();
    for entry in EFFECT_CATALOG {
        labels.push(entry.name);
        labels.extend(entry.aliases.iter().copied());
    }
    labels
}

fn transition_labels() -> Vec<&'static str> {
    let mut labels = Vec::new();
    for entry in TRANSITION_CATALOG {
        labels.push(entry.name);
        labels.extend(entry.aliases.iter().copied());
    }
    labels
}

/// Runtime error text for an effect name that is not in the catalog.
pub fn unknown_effect_message(name: &str) -> String {
    unknown_name_message("effect", name, &effect_labels())
}

/// Runtime error text for a transition name that is not in the catalog.
pub fn unknown_transition_message(name: &str) -> String {
    unknown_name_message("transition", name, &transition_labels())
}

/// Runtime error text listing parameter names declared for an effect.
pub fn unknown_effect_params_message(
    name: &str,
    unknown: &[String],
    entry: &EffectCatalogEntry,
) -> String {
    let valid: Vec<&str> = entry.params.iter().map(|(param, _)| *param).collect();
    let valid_list = if valid.is_empty() {
        "(none)".to_string()
    } else {
        valid.join(", ")
    };
    format!(
        "unknown parameter '{}' for effect '{name}'; valid: {valid_list}",
        unknown.join("', '")
    )
}

fn unknown_name_message(kind: &str, name: &str, labels: &[&str]) -> String {
    let close = close_names(name, labels);
    if close.is_empty() {
        format!("unknown {kind} '{name}'; valid: {}", labels.join(", "))
    } else {
        format!(
            "unknown {kind} '{name}'; close matches: {}; valid: {}",
            close.join(", "),
            labels.join(", ")
        )
    }
}

fn close_names<'a>(query: &str, names: &[&'a str]) -> Vec<&'a str> {
    let query_l = query.to_ascii_lowercase();
    let mut scored: Vec<(usize, &'a str)> = names
        .iter()
        .copied()
        .filter(|name| *name != query)
        .filter_map(|name| {
            let name_l = name.to_ascii_lowercase();
            let dist = edit_distance(&query_l, &name_l);
            let contains = name_l.len() >= 4
                && (name_l.contains(&query_l) || query_l.contains(&name_l));
            let near = dist <= 2 || (dist <= 3 && query_l.len() >= 6) || contains;
            near.then_some((dist, name))
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(b.1)));
    scored.dedup_by(|a, b| a.1 == b.1);
    scored.into_iter().take(5).map(|(_, name)| name).collect()
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// JSON schema tags for effects (one per catalog entry).
pub fn catalog_effect_schema_tags() -> Vec<&'static str> {
    EFFECT_CATALOG.iter().map(|e| e.schema_tag).collect()
}

/// PascalCase `EffectType` tags that project JSON can serialize (Plan 05 parity).
///
/// Catalog Rhai names are snake_case and include ColorMatrix presets
/// (`grayscale`, `sepia`, …) that lower to `ColorMatrix` / other runtime types.
/// This list is the runtime enum surface that must stay in lockstep with
/// `EffectType::type_tag` / `project::effect_type_to_json`.
pub fn project_effect_type_tags() -> &'static [&'static str] {
    &[
        "Blur",
        "ColorMatrix",
        "RuntimeShader",
        "DropShadow",
        "DirectionalBlur",
        "FilmGrain",
        "Vignette",
        "ChromaticAberration",
        "Pixelate",
        "Sharpen",
        "Glow",
        "Warp",
        "Swirl",
        "Ripple",
        "Glass",
        "Shake",
        "Duotone",
        "Posterize",
        "Threshold",
        "Scanlines",
        "Glitch",
        "EdgeDetect",
        "ZoomBlur",
        "Kaleidoscope",
        "Thermal",
        "Liquid",
        "Acid",
        "Halftone",
        "Ascii",
        "Sketch",
        "Mirror",
        "TriTone",
        "Vhs",
        "NeonEdge",
        "Fisheye",
        "Bulge",
        "Barrel",
        "LensFlare",
        "Noir",
        "Cyberpunk",
        "Vintage",
        "Gameboy",
        "BleachBypass",
        "TealOrange",
        "Plasma",
        "Hologram",
        "Emboss",
        "Toon",
        "OilPaint",
        "LightLeak",
        "Aurora",
        "SunRays",
        "SkinSmooth",
        "BeautyGlow",
        "SoftFocus",
        "Vibrance",
        "Fog",
        "Mist",
        "Rain",
        "Snow",
        "Dust",
        "BokehDots",
        "FilmLook",
        "OrangeTeal",
        "CrossProcess",
        "Polaroid",
        "FadedFilm",
        "Midnight",
        "Watercolor",
        "Crosshatch",
        "Pointillism",
        "Mosaic",
        "Ink",
        "PencilColor",
    ]
}

/// Primary transition type strings.
pub fn catalog_transition_names() -> Vec<&'static str> {
    TRANSITION_CATALOG.iter().map(|t| t.name).collect()
}

/// `NodeKind` serde tags for catalog entries exported to `MovieRequest` JSON.
pub fn catalog_node_schema_tags() -> Vec<&'static str> {
    NODE_CATALOG
        .iter()
        .filter(|n| n.json_schema)
        .map(|n| n.schema_tag)
        .collect()
}

const fn node(
    name: &'static str,
    schema_tag: &'static str,
    description: &'static str,
    supports_children: bool,
    animatable_properties: &'static [&'static str],
    features: &'static [&'static str],
    json_schema: bool,
) -> NodeCatalogEntry {
    NodeCatalogEntry {
        name,
        schema_tag,
        description,
        supports_children,
        animatable_properties,
        features,
        json_schema,
    }
}

const fn e(
    name: &'static str,
    schema_tag: &'static str,
    description: &'static str,
    params: &'static [(&'static str, &'static str)],
    aliases: &'static [&'static str],
) -> EffectCatalogEntry {
    EffectCatalogEntry {
        name,
        schema_tag,
        description,
        params,
        aliases,
    }
}

const fn t(
    name: &'static str,
    description: &'static str,
    params: &'static [(&'static str, &'static str)],
    aliases: &'static [&'static str],
) -> TransitionCatalogEntry {
    TransitionCatalogEntry {
        name,
        description,
        params,
        aliases,
    }
}

const fn ease(name: &'static str, description: &'static str) -> EasingCatalogEntry {
    EasingCatalogEntry { name, description }
}

const fn rhai_fn(
    name: &'static str,
    description: &'static str,
    params: &'static [(&'static str, &'static str)],
) -> RhaiFunctionEntry {
    RhaiFunctionEntry { name, description, params }
}