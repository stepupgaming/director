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
        "Wgpu",
        "",
        "GPU compute node with custom WGSL shaders (Rhai-only; not in MovieRequest JSON)",
        false,
        &["opacity"],
        &["wgpu_gpu_compute"],
        false,
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
    RhaiFunctionEntry {
        name: "add_loop",
        description: "Loop a scene N times on the movie timeline (JSON: Scene.loop_count)",
    },
    RhaiFunctionEntry {
        name: "add_freeze",
        description: "Hold a scene frame for an extra duration (JSON: Scene.post_hold)",
    },
    RhaiFunctionEntry {
        name: "save_project",
        description: "Serialize the live Director graph to versioned JSON",
    },
    RhaiFunctionEntry {
        name: "load_project",
        description: "Load a versioned project JSON into a MovieHandle",
    },
    RhaiFunctionEntry {
        name: "apply_effect",
        description: "Attach a visual effect to a node by catalog name",
    },
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

/// JSON schema tags for effects (one per catalog entry).
pub fn catalog_effect_schema_tags() -> Vec<&'static str> {
    EFFECT_CATALOG.iter().map(|e| e.schema_tag).collect()
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