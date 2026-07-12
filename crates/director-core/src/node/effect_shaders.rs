//! # Effect Shader Builders
//!
//! SkSL runtime-shader implementations for the extended `EffectType` variants.
//!
//! Each builder compiles its SkSL once (cached in the shared `shader_cache`)
//! and applies the result as an Skia `ImageFilter`. The automatic uniforms
//! `u_resolution` and `u_time` are injected only when a shader declares them.
//!
//! See `crate::node::effect` for the `EffectType` enum and `build_effect_filter`.

use crate::types::Color;
use skia_safe::{image_filters, runtime_effect::RuntimeShaderBuilder, RuntimeEffect};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::error;

/// Compile (once) and apply a cached runtime shader, invoking `configure` to set uniforms.
///
/// Returns `current_filter` unchanged when no shader cache is supplied or the shader
/// fails to compile.
fn cached_runtime_shader(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    cache_key: &str,
    sksl: &str,
    label: &str,
    current_filter: Option<skia_safe::ImageFilter>,
    configure: impl FnOnce(&mut RuntimeShaderBuilder),
) -> Option<skia_safe::ImageFilter> {
    let cache_arc = match shader_cache {
        Some(c) => c,
        None => return current_filter,
    };
    let mut cache = cache_arc.lock().unwrap();
    if !cache.contains_key(cache_key) {
        match RuntimeEffect::make_for_shader(sksl, None) {
            Ok(effect) => {
                cache.insert(cache_key.to_string(), effect);
            }
            Err(e) => {
                error!("{} shader compilation error: {}", label, e);
                return current_filter;
            }
        }
    }
    let effect = match cache.get(cache_key) {
        Some(e) => e.clone(),
        None => return current_filter,
    };
    drop(cache);
    let mut builder = RuntimeShaderBuilder::new(effect);
    configure(&mut builder);
    image_filters::runtime_shader(&builder, "image", current_filter)
}

/// Warp: sine-wave displacement distortion.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_warp(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    amplitude: f32,
    frequency: f32,
    direction: f32,
    speed: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_time;
uniform float u_amplitude;
uniform float u_frequency;
uniform float u_direction;
uniform float u_speed;

half4 main(float2 pos) {
    float rad = radians(u_direction);
    float2 wave_dir = float2(cos(rad), sin(rad));
    float2 perp = float2(-sin(rad), cos(rad));
    float wave = sin(dot(pos, wave_dir) * u_frequency + u_time * u_speed) * u_amplitude;
    return image.eval(pos + perp * wave);
}
"#;
    cached_runtime_shader(shader_cache, "__warp__", sksl, "Warp", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_time", &[time]);
        let _ = b.set_uniform_float("u_amplitude", &[amplitude]);
        let _ = b.set_uniform_float("u_frequency", &[frequency]);
        let _ = b.set_uniform_float("u_direction", &[direction]);
        let _ = b.set_uniform_float("u_speed", &[speed]);
    })
}

/// Swirl: rotational twist around the center.
pub(crate) fn build_swirl(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    radius: f32,
    angle: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_radius;
uniform float u_angle;

half4 main(float2 pos) {
    float2 center = u_resolution * 0.5;
    float2 d = pos - center;
    float dist = length(d);
    float t = 1.0 - smoothstep(0.0, u_radius, dist);
    float a = atan(d.y, d.x) + radians(u_angle) * t;
    float2 new_pos = center + float2(cos(a), sin(a)) * dist;
    return image.eval(new_pos);
}
"#;
    cached_runtime_shader(shader_cache, "__swirl__", sksl, "Swirl", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_radius", &[radius]);
        let _ = b.set_uniform_float("u_angle", &[angle]);
    })
}

/// Ripple: concentric waves from the center.
pub(crate) fn build_ripple(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    amplitude: f32,
    frequency: f32,
    speed: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_time;
uniform float u_amplitude;
uniform float u_frequency;
uniform float u_speed;

half4 main(float2 pos) {
    float2 center = u_resolution * 0.5;
    float2 d = pos - center;
    float dist = length(d);
    float wave = sin(dist * u_frequency - u_time * u_speed) * u_amplitude;
    float2 dir = (dist > 0.001) ? normalize(d) : float2(0.0);
    return image.eval(pos + dir * wave);
}
"#;
    cached_runtime_shader(shader_cache, "__ripple__", sksl, "Ripple", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_time", &[time]);
        let _ = b.set_uniform_float("u_amplitude", &[amplitude]);
        let _ = b.set_uniform_float("u_frequency", &[frequency]);
        let _ = b.set_uniform_float("u_speed", &[speed]);
    })
}

/// Glass: frosted-glass refraction combined with a small blur.
pub(crate) fn build_glass(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    scale: f32,
    intensity: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_scale;
uniform float u_intensity;

float hash(float2 p) {
    return fract(sin(dot(p, float2(127.1, 311.7))) * 43758.5453);
}

half4 main(float2 pos) {
    float2 n = float2(hash(pos * u_scale), hash(pos * u_scale + 7.3)) - 0.5;
    float2 offset = n * u_intensity;
    float r = u_intensity * 2.0;
    half4 color = half4(0.0);
    color += image.eval(pos + offset + float2(-r, -r));
    color += image.eval(pos + offset + float2(0.0, -r));
    color += image.eval(pos + offset + float2(r, -r));
    color += image.eval(pos + offset + float2(-r, 0.0));
    color += image.eval(pos + offset);
    color += image.eval(pos + offset + float2(r, 0.0));
    color += image.eval(pos + offset + float2(-r, r));
    color += image.eval(pos + offset + float2(0.0, r));
    color += image.eval(pos + offset + float2(r, r));
    return color / 9.0;
}
"#;
    cached_runtime_shader(shader_cache, "__glass__", sksl, "Glass", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_scale", &[scale]);
        let _ = b.set_uniform_float("u_intensity", &[intensity]);
    })
}

/// Shake: block-based jitter displacement driven by time.
pub(crate) fn build_shake(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
    speed: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_time;
uniform float u_intensity;
uniform float u_speed;

float hash(float2 p) {
    return fract(sin(dot(p, float2(127.1, 311.7))) * 43758.5453);
}

half4 main(float2 pos) {
    float t = floor(u_time * u_speed);
    float2 block = floor(pos / max(u_intensity, 1.0));
    float ox = (hash(block + t) - 0.5) * u_intensity * 2.0;
    float oy = (hash(block + t + 31.0) - 0.5) * u_intensity * 2.0;
    return image.eval(pos + float2(ox, oy));
}
"#;
    cached_runtime_shader(shader_cache, "__shake__", sksl, "Shake", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_time", &[time]);
        let _ = b.set_uniform_float("u_intensity", &[intensity]);
        let _ = b.set_uniform_float("u_speed", &[speed]);
    })
}

/// Duotone: luminance-mapped two-color grade.
pub(crate) fn build_duotone(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    _resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    shadow: Color,
    highlight: Color,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float3 u_shadow;
uniform float3 u_highlight;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float lum = dot(color.rgb, half3(0.2126, 0.7152, 0.0722));
    half3 mapped = mix(half3(u_shadow), half3(u_highlight), half(lum));
    return half4(mapped, color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__duotone__", sksl, "Duotone", current_filter, |b| {
        let _ = b.set_uniform_float("u_shadow", &[shadow.r, shadow.g, shadow.b]);
        let _ = b.set_uniform_float("u_highlight", &[highlight.r, highlight.g, highlight.b]);
    })
}
/// Posterize: reduce color levels.
pub(crate) fn build_posterize(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    _resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    levels: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float u_levels;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    half3 c = floor(color.rgb * u_levels + 0.5) / u_levels;
    return half4(c, color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__posterize__", sksl, "Posterize", current_filter, |b| {
        let _ = b.set_uniform_float("u_levels", &[levels]);
    })
}

/// Threshold: black/white luminance cutoff.
pub(crate) fn build_threshold(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    _resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    level: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float u_level;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float lum = dot(color.rgb, half3(0.2126, 0.7152, 0.0722));
    half v = (lum >= u_level) ? half(1.0) : half(0.0);
    return half4(v, v, v, color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__threshold__", sksl, "Threshold", current_filter, |b| {
        let _ = b.set_uniform_float("u_level", &[level]);
    })
}

/// Scanlines: CRT-style scanline overlay.
pub(crate) fn build_scanlines(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
    count: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_time;
uniform float u_intensity;
uniform float u_count;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float y = pos.y + u_time * 50.0;
    float line = sin(y * u_count * 3.14159) * 0.5 + 0.5;
    float mask = mix(1.0, line, u_intensity);
    return half4(color.rgb * mask, color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__scanlines__", sksl, "Scanlines", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_time", &[time]);
        let _ = b.set_uniform_float("u_intensity", &[intensity]);
        let _ = b.set_uniform_float("u_count", &[count]);
    })
}

/// Glitch: RGB split with block displacement.
pub(crate) fn build_glitch(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
    speed: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_time;
uniform float u_intensity;
uniform float u_speed;

float hash(float2 p) {
    return fract(sin(dot(p, float2(127.1, 311.7))) * 43758.5453);
}

half4 main(float2 pos) {
    float t = floor(u_time * u_speed);
    float block_y = floor(pos.y / 20.0);
    float2 offset = float2(0.0);
    if (hash(float2(block_y, t)) > 0.7) {
        offset.x = (hash(float2(block_y + 1.0, t)) - 0.5) * u_intensity * 16.0;
    }
    float r = image.eval(pos + offset + float2(u_intensity * 4.0, 0.0)).r;
    float g = image.eval(pos + offset).g;
    float b = image.eval(pos + offset - float2(u_intensity * 4.0, 0.0)).b;
    float a = image.eval(pos + offset).a;
    return half4(r, g, b, a);
}
"#;
    cached_runtime_shader(shader_cache, "__glitch__", sksl, "Glitch", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_time", &[time]);
        let _ = b.set_uniform_float("u_intensity", &[intensity]);
        let _ = b.set_uniform_float("u_speed", &[speed]);
    })
}

/// Edge detect: Sobel outline on luminance.
pub(crate) fn build_edge_detect(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_intensity;

float luma(float3 c) {
    return dot(c, float3(0.2126, 0.7152, 0.0722));
}

half4 main(float2 pos) {
    // Resolution-relative offset so edges are consistent across canvas sizes.
    float2 px = 1.0 / u_resolution;
    float2 o = px * 1.5;
    float tl = luma(float3(image.eval(pos + float2(-o.x, -o.y)).rgb));
    float t  = luma(float3(image.eval(pos + float2(0.0,  -o.y)).rgb));
    float tr = luma(float3(image.eval(pos + float2( o.x, -o.y)).rgb));
    float l  = luma(float3(image.eval(pos + float2(-o.x, 0.0)).rgb));
    float r  = luma(float3(image.eval(pos + float2( o.x, 0.0)).rgb));
    float bl = luma(float3(image.eval(pos + float2(-o.x,  o.y)).rgb));
    float b  = luma(float3(image.eval(pos + float2(0.0,   o.y)).rgb));
    float br = luma(float3(image.eval(pos + float2( o.x,  o.y)).rgb));
    float gx = -tl - 2.0 * l - bl + tr + 2.0 * r + br;
    float gy = -tl - 2.0 * t - tr + bl + 2.0 * b + br;
    float g = sqrt(gx * gx + gy * gy) * u_intensity;
    g = min(g, 1.0);
    half4 color = image.eval(pos);
    return half4(half3(g), color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__edge_detect__", sksl, "EdgeDetect", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_intensity", &[intensity]);
    })
}

/// Zoom blur: radial motion blur from the center.
pub(crate) fn build_zoom_blur(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    strength: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_strength;

half4 main(float2 pos) {
    float2 center = u_resolution * 0.5;
    float2 dir = pos - center;
    half4 color = half4(0.0);
    for (int i = 0; i < 16; i++) {
        float t = float(i) / 16.0;
        color += image.eval(pos - dir * t * u_strength);
    }
    return color / 16.0;
}
"#;
    cached_runtime_shader(shader_cache, "__zoom_blur__", sksl, "ZoomBlur", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_strength", &[strength]);
    })
}

/// Kaleidoscope: mirror symmetry segments.
pub(crate) fn build_kaleidoscope(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    segments: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_segments;

half4 main(float2 pos) {
    float2 center = u_resolution * 0.5;
    float2 d = pos - center;
    float r = length(d);
    float a = atan(d.y, d.x);
    float seg = 6.2831853 / u_segments;
    a = mod(a, seg);
    a = abs(a - seg * 0.5);
    float2 new_pos = center + float2(cos(a), sin(a)) * r;
    return image.eval(new_pos);
}
"#;
    cached_runtime_shader(shader_cache, "__kaleidoscope__", sksl, "Kaleidoscope", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_segments", &[segments]);
    })
}

// ============================================================================
// New shader library — ported from shader-color-panels WebGL GLSL to SkSL.
// GLSL→SkSL conversion: texture2D(u_image, uv) → image.eval(pos),
//   void main() → half4 main(float2 pos), UV normalized via pos / u_resolution.
// ============================================================================

/// Thermal: heat map color spectrum (luminance → blue→cyan→yellow→red→white).
pub(crate) fn build_thermal(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    _resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float luma = dot(color.rgb, half3(0.2126, 0.7152, 0.0722));

    half3 heat;
    if (luma < 0.15) {
        heat = mix(half3(0.0, 0.0, 0.2), half3(0.0, 0.0, 1.0), luma / 0.15);
    } else if (luma < 0.4) {
        heat = mix(half3(0.0, 0.0, 1.0), half3(0.0, 0.9, 0.9), (luma - 0.15) / 0.25);
    } else if (luma < 0.65) {
        heat = mix(half3(0.0, 0.9, 0.9), half3(0.9, 0.9, 0.0), (luma - 0.4) / 0.25);
    } else if (luma < 0.9) {
        heat = mix(half3(0.9, 0.9, 0.0), half3(1.0, 0.2, 0.0), (luma - 0.65) / 0.25);
    } else {
        heat = mix(half3(1.0, 0.2, 0.0), half3(1.0, 1.0, 1.0), (luma - 0.9) / 0.1);
    }
    return half4(heat, color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__thermal__", sksl, "Thermal", current_filter, |_| {})
}

/// Liquid: bidirectional sine/cosine coordinate warp.
pub(crate) fn build_liquid(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    frequency: f32,
    amplitude: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_time;
uniform float u_frequency;
uniform float u_amplitude;

half4 main(float2 pos) {
    float2 uv = pos / u_resolution;
    uv.x += sin(uv.y * u_frequency + u_time * 1.5) * u_amplitude;
    uv.y += cos(uv.x * (u_frequency * 0.8) + u_time * 1.2) * u_amplitude;
    return image.eval(uv * u_resolution);
}
"#;
    cached_runtime_shader(shader_cache, "__liquid__", sksl, "Liquid", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_time", &[time]);
        let _ = b.set_uniform_float("u_frequency", &[frequency]);
        let _ = b.set_uniform_float("u_amplitude", &[amplitude]);
    })
}

/// Acid: fluorescent acid melt (wave distortion + color morph).
pub(crate) fn build_acid(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    acid_color: Color,
    speed: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_time;
uniform float u_speed;
uniform float3 u_acid_color;

half4 main(float2 pos) {
    float2 uv = pos / u_resolution;
    uv.x += sin(uv.y * 12.0 + u_time * u_speed) * 0.012;
    uv.y += cos(uv.x * 10.0 + u_time * (u_speed * 0.8)) * 0.012;

    half4 color = image.eval(uv * u_resolution);
    float luma = dot(color.rgb, half3(0.2126, 0.7152, 0.0722));

    half3 wave_color = mix(half3(u_acid_color), half3(0.0, 1.0, 0.8),
        half(sin(luma * 8.0 + u_time * u_speed) * 0.5 + 0.5));
    half3 final_color = mix(color.rgb, wave_color, half(0.45));
    return half4(final_color, color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__acid__", sksl, "Acid", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_time", &[time]);
        let _ = b.set_uniform_float("u_speed", &[speed]);
        let _ = b.set_uniform_float("u_acid_color", &[acid_color.r, acid_color.g, acid_color.b]);
    })
}

/// Halftone: pop-art dot grid (luminance → dot size mapping).
pub(crate) fn build_halftone(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    dot_size: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_dot_size;

half4 main(float2 pos) {
    float2 uv = pos / u_resolution;

    float2 cell_coords = fract(uv * u_dot_size) - 0.5;
    float dist = length(cell_coords);

    float2 cell_center = (floor(uv * u_dot_size) + 0.5) / u_dot_size;
    half4 color = image.eval(cell_center * u_resolution);
    float luma = dot(color.rgb, half3(0.2126, 0.7152, 0.0722));

    float threshold = 0.5 * (1.0 - luma);
    float dot_mask = smoothstep(threshold + 0.06, threshold - 0.06, dist);

    half3 dot_color = color.rgb * 1.4;
    half3 bg_color = half3(0.06, 0.06, 0.09);
    half3 final_color = mix(bg_color, dot_color, half(dot_mask));
    return half4(final_color, color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__halftone__", sksl, "Halftone", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_dot_size", &[dot_size]);
    })
}

/// Ascii: terminal ASCII art effect.
pub(crate) fn build_ascii(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    scale: f32,
    ascii_color: Color,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_scale;
uniform float3 u_ascii_color;
uniform float u_time;

float hash(float2 p) {
    return fract(sin(dot(p, float2(12.9898, 78.233))) * 43758.5453);
}

half4 main(float2 pos) {
    float2 uv = pos / u_resolution;
    float aspect = u_resolution.x / u_resolution.y;
    float2 size = float2(u_scale, u_scale / aspect);
    float2 cell_uv = floor(uv * size) / size;
    float2 local_uv = fract(uv * size);

    half4 tex = image.eval(cell_uv * u_resolution);
    float luma = dot(tex.rgb, half3(0.2126, 0.7152, 0.0722));

    float is_one = step(0.5, hash(floor(uv * size) + floor(u_time * 1.5)));

    float char_mask = 0.0;
    if (is_one > 0.5) {
        char_mask = step(0.4, local_uv.x) * step(local_uv.x, 0.6) * step(0.15, local_uv.y) * step(local_uv.y, 0.85);
    } else {
        float dist_to_center = length(local_uv - 0.5);
        char_mask = step(0.2, dist_to_center) * step(dist_to_center, 0.4);
    }

    char_mask *= step(0.15, luma);

    half3 final_color = half3(u_ascii_color) * half(char_mask * (luma * 1.3));
    final_color += half3(u_ascii_color) * half(luma * 0.08);
    return half4(final_color, tex.a);
}
"#;
    cached_runtime_shader(shader_cache, "__ascii__", sksl, "Ascii", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_scale", &[scale]);
        let _ = b.set_uniform_float("u_ascii_color", &[ascii_color.r, ascii_color.g, ascii_color.b]);
        let _ = b.set_uniform_float("u_time", &[time]);
    })
}

/// Sketch: charcoal pencil sketch (edge detection + hatching + paper texture).
pub(crate) fn build_sketch(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    contrast: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_contrast;
uniform float u_time;

float hash(float2 p) {
    return fract(sin(dot(p, float2(12.9898, 78.233))) * 43758.5453);
}

half4 main(float2 pos) {
    float2 uv = pos / u_resolution;
    half4 original = image.eval(pos);

    float offset = 1.0 / 400.0;
    float left   = dot(float3(image.eval(pos - float2(offset * u_resolution.x, 0.0)).rgb), float3(0.2126, 0.7152, 0.0722));
    float right  = dot(float3(image.eval(pos + float2(offset * u_resolution.x, 0.0)).rgb), float3(0.2126, 0.7152, 0.0722));
    float top    = dot(float3(image.eval(pos - float2(0.0, offset * u_resolution.y)).rgb), float3(0.2126, 0.7152, 0.0722));
    float bottom = dot(float3(image.eval(pos + float2(0.0, offset * u_resolution.y)).rgb), float3(0.2126, 0.7152, 0.0722));

    float edge = abs(left - right) + abs(top - bottom);

    float hatch = sin((uv.x + uv.y) * 450.0 + hash(uv + floor(u_time * 8.0)) * 10.0) * 0.5 + 0.5;
    float luma = dot(original.rgb, half3(0.2126, 0.7152, 0.0722));

    float paper = 0.95 + 0.05 * hash(uv);
    float line_stroke = 1.0 - clamp(edge * u_contrast * 2.5, 0.0, 1.0);

    float shade = mix(1.0, hatch, clamp((0.7 - luma) * 1.5, 0.0, 1.0));
    half3 final_color = half3(paper * line_stroke * shade);
    return half4(final_color, original.a);
}
"#;
    cached_runtime_shader(shader_cache, "__sketch__", sksl, "Sketch", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_contrast", &[contrast]);
        let _ = b.set_uniform_float("u_time", &[time]);
    })
}

/// Mirror: quad-mirror symmetry (4-way reflection).
pub(crate) fn build_mirror(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;

half4 main(float2 pos) {
    float2 uv = pos / u_resolution;
    if (uv.x > 0.5) {
        uv.x = 1.0 - uv.x;
    }
    if (uv.y > 0.5) {
        uv.y = 1.0 - uv.y;
    }
    return image.eval(uv * u_resolution);
}
"#;
    cached_runtime_shader(shader_cache, "__mirror__", sksl, "Mirror", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
    })
}

/// TriTone: three-color luminance grade with adjustable midpoint.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_tritone(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    _resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    shadow: Color,
    mid: Color,
    highlight: Color,
    midpoint: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float3 u_color_low;
uniform float3 u_color_mid;
uniform float3 u_color_high;
uniform float u_midpoint;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float luma = dot(color.rgb, half3(0.2126, 0.7152, 0.0722));

    half3 final_color;
    if (luma < u_midpoint) {
        final_color = mix(half3(u_color_low), half3(u_color_mid),
            half(luma / max(u_midpoint, 0.01)));
    } else {
        final_color = mix(half3(u_color_mid), half3(u_color_high),
            half((luma - u_midpoint) / max(1.0 - u_midpoint, 0.01)));
    }
    return half4(final_color, color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__tritone__", sksl, "TriTone", current_filter, |b| {
        let _ = b.set_uniform_float("u_color_low", &[shadow.r, shadow.g, shadow.b]);
        let _ = b.set_uniform_float("u_color_mid", &[mid.r, mid.g, mid.b]);
        let _ = b.set_uniform_float("u_color_high", &[highlight.r, highlight.g, highlight.b]);
        let _ = b.set_uniform_float("u_midpoint", &[midpoint]);
    })
}

/// VHS: combined CRT glitch (jitter + scanlines + noise + RGB shift).
pub(crate) fn build_vhs(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    distortion: f32,
    scanline_intensity: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_time;
uniform float u_distortion;
uniform float u_scanline;

float hash(float2 p) {
    return fract(sin(dot(p, float2(12.9898, 78.233))) * 43758.5453);
}

half4 main(float2 pos) {
    float2 uv = pos / u_resolution;

    float wave = sin(uv.y * 30.0 + u_time * 4.0) * 0.1;
    float glitch_chance = step(0.96, sin(uv.y * 4.0 + u_time * 6.0));
    float displacement = glitch_chance * u_distortion * (sin(u_time * 25.0) + wave * 0.5);
    uv.x += displacement;

    half4 color = image.eval(uv * u_resolution);

    if (glitch_chance > 0.0) {
        color.r = image.eval((uv + float2(0.015 * u_distortion, 0.0)) * u_resolution).r;
        color.b = image.eval((uv - float2(0.015 * u_distortion, 0.0)) * u_resolution).b;
    }

    float scanline = sin(uv.y * 500.0) * u_scanline;
    color.rgb -= half3(scanline * 0.07);

    float grain = hash(uv + fract(u_time)) * 0.07;
    color.rgb += half3(grain * (0.5 + 0.5 * sin(u_time)));

    return color;
}
"#;
    cached_runtime_shader(shader_cache, "__vhs__", sksl, "VHS", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_time", &[time]);
        let _ = b.set_uniform_float("u_distortion", &[distortion]);
        let _ = b.set_uniform_float("u_scanline", &[scanline_intensity]);
    })
}

/// NeonEdge: edge detect with configurable glow color and darkened background.
pub(crate) fn build_neon_edge(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    glow_color: Color,
    threshold: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float3 u_edge_color;
uniform float u_threshold;

half4 main(float2 pos) {
    float2 uv = pos / u_resolution;
    float offset = 1.0 / 350.0;

    half4 center = image.eval(pos);
    float l_center = dot(center.rgb, half3(0.2126, 0.7152, 0.0722));
    float l_left   = dot(image.eval(pos - float2(offset * u_resolution.x, 0.0)).rgb, half3(0.2126, 0.7152, 0.0722));
    float l_right  = dot(image.eval(pos + float2(offset * u_resolution.x, 0.0)).rgb, half3(0.2126, 0.7152, 0.0722));
    float l_top    = dot(image.eval(pos - float2(0.0, offset * u_resolution.y)).rgb, half3(0.2126, 0.7152, 0.0722));
    float l_bottom = dot(image.eval(pos + float2(0.0, offset * u_resolution.y)).rgb, half3(0.2126, 0.7152, 0.0722));

    float edge_val = abs(l_left + l_right + l_top + l_bottom - 4.0 * l_center);
    edge_val = smoothstep(u_threshold, u_threshold + 0.08, edge_val) * 4.0;

    half3 bg = center.rgb * 0.18;
    half3 final_color = mix(bg, half3(u_edge_color), half(clamp(edge_val, 0.0, 1.0)));
    return half4(final_color, center.a);
}
"#;
    cached_runtime_shader(shader_cache, "__neon_edge__", sksl, "NeonEdge", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_edge_color", &[glow_color.r, glow_color.g, glow_color.b]);
        let _ = b.set_uniform_float("u_threshold", &[threshold]);
    })
}

// ============================================================================
// Batch 2 — lens/distortion, color-grade presets, stylized, and atmosphere.
// Same SkSL→Skia conventions as the shader-color-panels library above.
// ============================================================================

/// Fisheye: radial magnification that warps toward (strength > 0) or away
/// from (strength < 0) the center.
pub(crate) fn build_fisheye(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    strength: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_strength;

half4 main(float2 pos) {
    float2 center = u_resolution * 0.5;
    float2 d = pos - center;
    float maxr = length(center);
    float r = length(d);
    float t = clamp(r / max(maxr, 0.001), 0.0, 1.0);
    float k = 1.0 - u_strength * (1.0 - t * t);
    float2 new_pos = center + d * k;
    return image.eval(new_pos);
}
"#;
    cached_runtime_shader(shader_cache, "__fisheye__", sksl, "Fisheye", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_strength", &[strength]);
    })
}

/// Bulge: localized magnification (strength > 0) or pinch (strength < 0) inside
/// a circular radius around the center.
pub(crate) fn build_bulge(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    strength: f32,
    radius: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_strength;
uniform float u_radius;

half4 main(float2 pos) {
    float2 center = u_resolution * 0.5;
    float2 d = pos - center;
    float dist = length(d);
    float radius = max(u_radius, 1.0);
    if (dist < radius) {
        float pct = dist / radius;
        float theta = atan(d.y, d.x);
        float r = pow(pct, max(1.0 - u_strength, 0.05)) * radius;
        d = float2(cos(theta), sin(theta)) * r;
    }
    return image.eval(center + d);
}
"#;
    cached_runtime_shader(shader_cache, "__bulge__", sksl, "Bulge", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_strength", &[strength]);
        let _ = b.set_uniform_float("u_radius", &[radius]);
    })
}

/// Barrel: barrel/pincushion lens distortion driven by a single curvature term.
pub(crate) fn build_barrel(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    curvature: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_curvature;

half4 main(float2 pos) {
    float2 uv = pos / u_resolution;
    float2 c = uv - 0.5;
    float r2 = dot(c, c);
    float f = 1.0 + u_curvature * r2;
    float2 new_uv = clamp(0.5 + c * f, 0.0, 1.0);
    return image.eval(new_uv * u_resolution);
}
"#;
    cached_runtime_shader(shader_cache, "__barrel__", sksl, "Barrel", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_curvature", &[curvature]);
    })
}

/// LensFlare: additive radial glow plus a soft horizontal streak.
pub(crate) fn build_lens_flare(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
    color: Color,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_intensity;
uniform float3 u_color;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float2 uv = pos / u_resolution;
    float2 c = uv - 0.5;
    float dist = length(c);
    float glow = smoothstep(0.5, 0.0, dist) * u_intensity;
    float streak = smoothstep(0.012, 0.0, abs(c.y)) * (1.0 - smoothstep(0.0, 0.5, abs(c.x))) * u_intensity * 0.5;
    half3 flare = half3(u_color) * (glow + streak);
    return half4(min(color.rgb + flare, 1.0), color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__lens_flare__", sksl, "LensFlare", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_intensity", &[intensity]);
        let _ = b.set_uniform_float("u_color", &[color.r, color.g, color.b]);
    })
}

/// Noir: high-contrast desaturated monochrome.
pub(crate) fn build_noir(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    _resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    contrast: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float u_contrast;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float luma = dot(color.rgb, half3(0.2126, 0.7152, 0.0722));
    luma = clamp((luma - 0.5) * u_contrast + 0.5, 0.0, 1.0);
    return half4(half3(luma), color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__noir__", sksl, "Noir", current_filter, |b| {
        let _ = b.set_uniform_float("u_contrast", &[contrast]);
    })
}

/// Cyberpunk: magenta-shadow / cyan-highlight color grade.
pub(crate) fn build_cyberpunk(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    _resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float u_intensity;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float luma = dot(color.rgb, half3(0.2126, 0.7152, 0.0722));
    half3 shadow = color.rgb * half3(0.1, 0.4, 0.8);
    half3 high = color.rgb * half3(1.0, 0.3, 0.8);
    half3 graded = mix(shadow, high, half(luma));
    return half4(mix(color.rgb, graded, half(u_intensity)), color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__cyberpunk__", sksl, "Cyberpunk", current_filter, |b| {
        let _ = b.set_uniform_float("u_intensity", &[intensity]);
    })
}

/// Vintage: faded warm film grade with gentle contrast lift.
pub(crate) fn build_vintage(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    _resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    amount: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float u_amount;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    half3 faded = mix(color.rgb, half3(0.9, 0.85, 0.75), 0.3);
    half3 graded = faded * half3(1.05, 1.0, 0.9);
    graded = (graded - 0.5) * 1.05 + 0.5;
    return half4(mix(color.rgb, clamp(graded, 0.0, 1.0), half(u_amount)), color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__vintage__", sksl, "Vintage", current_filter, |b| {
        let _ = b.set_uniform_float("u_amount", &[amount]);
    })
}

/// Gameboy: 4-tone green handheld palette mapped from luminance.
pub(crate) fn build_gameboy(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    _resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float luma = dot(color.rgb, half3(0.2126, 0.7152, 0.0722));
    half3 c0 = half3(0.06, 0.22, 0.11);
    half3 c1 = half3(0.16, 0.40, 0.20);
    half3 c2 = half3(0.35, 0.60, 0.30);
    half3 c3 = half3(0.70, 0.86, 0.55);
    half3 outc;
    if (luma < 0.25) { outc = c0; }
    else if (luma < 0.5) { outc = c1; }
    else if (luma < 0.75) { outc = c2; }
    else { outc = c3; }
    return half4(outc, color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__gameboy__", sksl, "Gameboy", current_filter, |_| {})
}

/// BleachBypass: desaturated, high-contrast silver-retention look.
pub(crate) fn build_bleach_bypass(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    _resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    amount: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float u_amount;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float luma = dot(color.rgb, half3(0.2126, 0.7152, 0.0722));
    half3 low_sat = half3(luma);
    half3 b = mix(color.rgb, low_sat, 0.7);
    b = (b - 0.5) * 1.3 + 0.5;
    return half4(mix(color.rgb, clamp(b, 0.0, 1.0), half(u_amount)), color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__bleach_bypass__", sksl, "BleachBypass", current_filter, |b| {
        let _ = b.set_uniform_float("u_amount", &[amount]);
    })
}

/// TealOrange: cinematic teal-shadow / orange-highlight luminance grade.
pub(crate) fn build_teal_orange(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    _resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    amount: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float u_amount;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float luma = dot(color.rgb, half3(0.2126, 0.7152, 0.0722));
    half3 shadow = half3(0.0, 0.4, 0.5);
    half3 high = half3(1.0, 0.6, 0.3);
    half3 graded = mix(shadow, high, half(clamp(luma, 0.0, 1.0)));
    half3 result = color.rgb * (graded / max(luma, 0.001));
    return half4(mix(color.rgb, clamp(result, 0.0, 1.0), half(u_amount)), color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__teal_orange__", sksl, "TealOrange", current_filter, |b| {
        let _ = b.set_uniform_float("u_amount", &[amount]);
    })
}

/// Plasma: animated color plasma blended over the source.
pub(crate) fn build_plasma(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    scale: f32,
    speed: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_time;
uniform float u_scale;
uniform float u_speed;

half4 main(float2 pos) {
    float2 uv = pos / u_resolution;
    float t = u_time * u_speed;
    float v = sin(uv.x * u_scale + t)
            + sin((uv.y * u_scale + t) * 1.3)
            + sin((uv.x + uv.y) * u_scale * 0.7 + t * 1.1);
    v = v * 0.5 + 0.5;
    half3 col = 0.5 + 0.5 * cos(6.2831853 * (v + float3(0.0, 0.33, 0.67)));
    half4 src = image.eval(pos);
    return half4(mix(src.rgb, col, 0.8), src.a);
}
"#;
    cached_runtime_shader(shader_cache, "__plasma__", sksl, "Plasma", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_time", &[time]);
        let _ = b.set_uniform_float("u_scale", &[scale]);
        let _ = b.set_uniform_float("u_speed", &[speed]);
    })
}

/// Hologram: chromatic split, cyan tint, scanlines, and flicker.
pub(crate) fn build_hologram(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_time;
uniform float u_intensity;

half4 main(float2 pos) {
    float2 uv = pos / u_resolution;
    half4 color = image.eval(pos);
    float shift = 0.004 * u_intensity;
    float r = image.eval(pos + float2(shift, 0.0)).r;
    float b = image.eval(pos - float2(shift, 0.0)).b;
    half3 rgb = half3(r, color.g, b);
    rgb = mix(rgb, rgb * half3(0.4, 1.0, 1.0), 0.5);
    float scan = sin(uv.y * 400.0 + u_time * 5.0) * 0.5 + 0.5;
    rgb *= mix(1.0, scan, 0.3 * u_intensity);
    rgb *= 0.9 + 0.1 * sin(u_time * 10.0);
    return half4(rgb, color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__hologram__", sksl, "Hologram", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_time", &[time]);
        let _ = b.set_uniform_float("u_intensity", &[intensity]);
    })
}

/// Emboss: directional emboss from neighboring samples. Resolution-relative
/// sampling so the emboss strength is independent of canvas size.
pub(crate) fn build_emboss(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    angle: f32,
    strength: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_angle;
uniform float u_strength;

half4 main(float2 pos) {
    float2 dir = float2(cos(radians(u_angle)), sin(radians(u_angle)));
    float2 o = dir * (1.5 / u_resolution.x);
    half4 c = image.eval(pos);
    half4 lo = image.eval(pos - o);
    half4 hi = image.eval(pos + o);
    float dl = dot(hi.rgb - lo.rgb, half3(0.2126, 0.7152, 0.0722));
    float v = 0.5 + dl * u_strength;
    return half4(half3(clamp(v, 0.0, 1.0)), c.a);
}
"#;
    cached_runtime_shader(shader_cache, "__emboss__", sksl, "Emboss", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_angle", &[angle]);
        let _ = b.set_uniform_float("u_strength", &[strength]);
    })
}

/// Toon: cel-shaded posterize with a luminance edge outline.
pub(crate) fn build_toon(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    levels: f32,
    edge: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_levels;
uniform float u_edge;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float2 px = 1.0 / u_resolution;
    float l = dot(color.rgb, half3(0.2126, 0.7152, 0.0722));
    float q = floor(l * u_levels) / u_levels;
    float lx = dot(image.eval(pos + float2(px.x, 0.0)).rgb, half3(0.2126, 0.7152, 0.0722));
    float ly = dot(image.eval(pos + float2(0.0, px.y)).rgb, half3(0.2126, 0.7152, 0.0722));
    float e = abs(l - lx) + abs(l - ly);
    float outline = smoothstep(0.0, u_edge, e);
    half3 base = half3(q);
    half3 outc = mix(base, half3(0.0), half(outline));
    return half4(mix(color.rgb, outc, 0.85), color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__toon__", sksl, "Toon", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_levels", &[levels]);
        let _ = b.set_uniform_float("u_edge", &[edge]);
    })
}

/// OilPaint: quantized color blocks for a painterly feel.
pub(crate) fn build_oil_paint(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    _resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    block_size: f32,
    levels: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float u_block_size;
uniform float u_levels;

half4 main(float2 pos) {
    float2 b = floor(pos / u_block_size) * u_block_size + u_block_size * 0.5;
    half4 c = image.eval(b);
    half3 q = floor(c.rgb * u_levels + 0.5) / u_levels;
    return half4(q, c.a);
}
"#;
    cached_runtime_shader(shader_cache, "__oil_paint__", sksl, "OilPaint", current_filter, |b| {
        let _ = b.set_uniform_float("u_block_size", &[block_size]);
        let _ = b.set_uniform_float("u_levels", &[levels]);
    })
}

/// LightLeak: warm diagonal gradient overlay.
pub(crate) fn build_light_leak(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
    color: Color,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_intensity;
uniform float3 u_color;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float2 uv = pos / u_resolution;
    float g = smoothstep(0.0, 1.0, uv.x * 0.7 + (1.0 - uv.y) * 0.7);
    float leak = pow(g, 2.0) * u_intensity;
    half3 add = half3(u_color) * half(leak);
    return half4(min(color.rgb + add, 1.0), color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__light_leak__", sksl, "LightLeak", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_intensity", &[intensity]);
        let _ = b.set_uniform_float("u_color", &[color.r, color.g, color.b]);
    })
}

/// Aurora: animated vertical light bands.
pub(crate) fn build_aurora(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
    speed: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_time;
uniform float u_intensity;
uniform float u_speed;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float2 uv = pos / u_resolution;
    float t = u_time * u_speed;
    float wave = sin(uv.x * 6.0 + t) * 0.1 + sin(uv.x * 3.0 - t * 1.3) * 0.15;
    float center = 0.6 + wave;
    float band = smoothstep(0.25, 0.0, abs(uv.y - center));
    half3 a = half3(0.1, 0.9, 0.5) * band + half3(0.5, 0.2, 0.9) * band * 0.5;
    return half4(min(color.rgb + a * half(u_intensity), 1.0), color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__aurora__", sksl, "Aurora", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_time", &[time]);
        let _ = b.set_uniform_float("u_intensity", &[intensity]);
        let _ = b.set_uniform_float("u_speed", &[speed]);
    })
}

/// SunRays: radial light rays emanating from the center.
pub(crate) fn build_sunrays(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
    angle: f32,
    speed: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_time;
uniform float u_intensity;
uniform float u_angle;
uniform float u_speed;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float2 uv = pos / u_resolution - 0.5;
    float a = atan(uv.y, uv.x) - radians(u_angle);
    float rays = sin(a * 24.0 + u_time * u_speed * 5.0);
    rays = smoothstep(0.4, 1.0, rays);
    float falloff = 1.0 - smoothstep(0.0, 0.7, length(uv));
    half3 light = half3(1.0, 0.95, 0.8) * rays * falloff * u_intensity;
    return half4(min(color.rgb + light, 1.0), color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__sunrays__", sksl, "SunRays", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_time", &[time]);
        let _ = b.set_uniform_float("u_intensity", &[intensity]);
        let _ = b.set_uniform_float("u_angle", &[angle]);
        let _ = b.set_uniform_float("u_speed", &[speed]);
    })
}

// ============================================================================
// Cinematic library — SkSL moved out of `effect.rs` so every effect goes
// through the shared `cached_runtime_shader` helper (no duplicated
// compile/cache boilerplate). Behavior is preserved unless a quality fix is
// noted in the doc comment.
// ============================================================================

/// Directional blur (motion blur) along an angle. Fixed 16 samples because
/// SkSL requires constant loop bounds.
pub(crate) fn build_directional_blur(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    strength: f32,
    angle: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_strength;
uniform float u_angle;

half4 main(float2 pos) {
    float rad = radians(u_angle);
    float2 dir = float2(cos(rad), sin(rad));
    half4 color = half4(0.0);
    for (int i = 0; i < 16; i++) {
        float t = (float(i) - 7.5) / 8.0;
        float2 offset = dir * u_strength * t;
        color += image.eval(pos + offset);
    }
    return color / 16.0;
}
"#;
    cached_runtime_shader(
        shader_cache, "__directional_blur__", sksl, "DirectionalBlur", current_filter, |b| {
            let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
            let _ = b.set_uniform_float("u_strength", &[strength]);
            let _ = b.set_uniform_float("u_angle", &[angle]);
        },
    )
}

/// Film grain / noise overlay.
pub(crate) fn build_film_grain(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
    size: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_time;
uniform float u_intensity;
uniform float u_size;

float hash(float2 p) {
    return fract(sin(dot(p, float2(127.1, 311.7))) * 43758.5453);
}

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float2 grain_pos = floor(pos / u_size) + floor(u_time * 24.0);
    float grain = hash(grain_pos) * 2.0 - 1.0;
    color.rgb += half3(grain * u_intensity);
    return color;
}
"#;
    cached_runtime_shader(
        shader_cache, "__film_grain__", sksl, "FilmGrain", current_filter, |b| {
            let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
            let _ = b.set_uniform_float("u_time", &[time]);
            let _ = b.set_uniform_float("u_intensity", &[intensity]);
            let _ = b.set_uniform_float("u_size", &[size]);
        },
    )
}

/// Vignette: darken edges of frame.
pub(crate) fn build_vignette(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
    radius: f32,
    feather: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_intensity;
uniform float u_radius;
uniform float u_feather;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float2 center = u_resolution * 0.5;
    float dist = distance(pos, center) / length(center);
    float v = 1.0 - smoothstep(u_radius - u_feather, u_radius + u_feather, dist) * u_intensity;
    return half4(color.rgb * v, color.a);
}
"#;
    cached_runtime_shader(
        shader_cache, "__vignette__", sksl, "Vignette", current_filter, |b| {
            let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
            let _ = b.set_uniform_float("u_intensity", &[intensity]);
            let _ = b.set_uniform_float("u_radius", &[radius]);
            let _ = b.set_uniform_float("u_feather", &[feather]);
        },
    )
}

/// Chromatic aberration: RGB channel offset.
pub(crate) fn build_chromatic_aberration(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    strength: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_strength;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float r = image.eval(pos + float2(u_strength, 0.0)).r;
    float g = color.g;
    float b = image.eval(pos - float2(u_strength, 0.0)).b;
    return half4(r, g, b, color.a);
}
"#;
    cached_runtime_shader(
        shader_cache, "__chromatic_aberration__", sksl, "ChromaticAberration", current_filter,
        |b| {
            let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
            let _ = b.set_uniform_float("u_strength", &[strength]);
        },
    )
}

/// Pixelate: blocky pixelation grid.
pub(crate) fn build_pixelate(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    block_size: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_block_size;

half4 main(float2 pos) {
    float2 block = floor(pos / u_block_size) * u_block_size + u_block_size * 0.5;
    return image.eval(block);
}
"#;
    cached_runtime_shader(
        shader_cache, "__pixelate__", sksl, "Pixelate", current_filter, |b| {
            let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
            let _ = b.set_uniform_float("u_block_size", &[block_size]);
        },
    )
}

/// Sharpen: unsharp mask via 3x3 Laplacian. (A1 fix: resolution-relative
/// sampling so the result is independent of canvas size.)
pub(crate) fn build_sharpen(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    strength: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_strength;

half4 main(float2 pos) {
    half4 center = image.eval(pos);
    float2 px = 1.0 / u_resolution;
    float2 o = px * 1.5;
    half4 sum = image.eval(pos + float2(-o.x, -o.y))
              + image.eval(pos + float2(0.0,  -o.y))
              + image.eval(pos + float2( o.x, -o.y))
              + image.eval(pos + float2(-o.x, 0.0))
              + image.eval(pos + float2( o.x, 0.0))
              + image.eval(pos + float2(-o.x,  o.y))
              + image.eval(pos + float2(0.0,   o.y))
              + image.eval(pos + float2( o.x,  o.y));
    half4 laplacian = center * 9.0 - sum;
    return center + laplacian * u_strength;
}
"#;
    cached_runtime_shader(
        shader_cache, "__sharpen__", sksl, "Sharpen", current_filter, |b| {
            let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
            let _ = b.set_uniform_float("u_strength", &[strength]);
        },
    )
}

/// Glow: additive bright-area bloom. (A3 fix: replace the naive box blur with
/// a luminance-weighted 9-tap kernel for a smoother, cheaper bloom.)
pub(crate) fn build_glow(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
    threshold: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_intensity;
uniform float u_threshold;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    // 9-tap luminance-weighted bloom. Fixed radius (SkSL needs constant loops).
    float2 px = 1.0 / u_resolution;
    float radius = 3.0;
    half3 bloom = half3(0.0);
    half wsum = half(0.0);
    for (int x = -1; x <= 1; x++) {
        for (int y = -1; y <= 1; y++) {
            half2 d = half2(float(x), float(y)) * radius;
            half w = half(exp(-dot(d, d) / 8.0));
            half4 s = image.eval(pos + float2(d) * px);
            half lum = dot(s.rgb, half3(0.2126, 0.7152, 0.0722));
            half mask = smoothstep(half(u_threshold), half(1.0), lum);
            bloom += s.rgb * w * mask;
            wsum += w;
        }
    }
    bloom /= max(wsum, half(0.0001));
    return half4(min(color.rgb + bloom * u_intensity, 1.0), color.a);
}
"#;
    cached_runtime_shader(
        shader_cache, "__glow__", sksl, "Glow", current_filter, |b| {
            let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
            let _ = b.set_uniform_float("u_intensity", &[intensity]);
            let _ = b.set_uniform_float("u_threshold", &[threshold]);
        },
    )
}

// ============================================================================
// Batch 3 — beauty/portrait, atmospheric/weather, cinematic grades, and
// artistic stylize shaders (SkSL runtime shaders via `cached_runtime_shader`).
// ============================================================================

/// SkinSmooth: edge-preserving (bilateral-ish) smoothing for portrait/beauty.
pub(crate) fn build_skin_smooth(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
    radius: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_intensity;
uniform float u_radius;

half4 main(float2 pos) {
    float2 px = 1.0 / u_resolution;
    half4 center = image.eval(pos);
    half3 sum = center.rgb;
    float wsum = 1.0;
    float r = max(u_radius, 1.0);
    for (int x = -2; x <= 2; x++) {
        for (int y = -2; y <= 2; y++) {
            if (x == 0 && y == 0) { continue; }
            float2 off = float2(float(x), float(y)) * r * px;
            half4 s = image.eval(pos + off);
            float sw = exp(-float(x * x + y * y) / 4.0);
            float cd = length(s.rgb - center.rgb);
            float rw = exp(-cd * cd * 12.0);
            float w = sw * rw;
            sum += s.rgb * w;
            wsum += w;
        }
    }
    half3 smoothed = sum / wsum;
    return half4(mix(center.rgb, smoothed, u_intensity), center.a);
}
"#;
    cached_runtime_shader(
        shader_cache, "__skin_smooth__", sksl, "SkinSmooth", current_filter, |b| {
            let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
            let _ = b.set_uniform_float("u_intensity", &[intensity]);
            let _ = b.set_uniform_float("u_radius", &[radius]);
        },
    )
}

/// BeautyGlow: soft, diffuse glow biased toward brighter areas.
pub(crate) fn build_beauty_glow(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
    threshold: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_intensity;
uniform float u_threshold;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float2 px = 1.0 / u_resolution;
    half3 glow = half3(0.0);
    for (int x = -2; x <= 2; x++) {
        for (int y = -2; y <= 2; y++) {
            half4 s = image.eval(pos + float2(float(x), float(y)) * 2.0 * px);
            float lum = dot(s.rgb, half3(0.2126, 0.7152, 0.0722));
            glow += s.rgb * smoothstep(u_threshold, 1.0, lum);
        }
    }
    glow /= 25.0;
    return half4(min(color.rgb + glow * u_intensity, 1.0), color.a);
}
"#;
    cached_runtime_shader(
        shader_cache, "__beauty_glow__", sksl, "BeautyGlow", current_filter, |b| {
            let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
            let _ = b.set_uniform_float("u_intensity", &[intensity]);
            let _ = b.set_uniform_float("u_threshold", &[threshold]);
        },
    )
}

/// SoftFocus: dreamy diffusion (mix of source and a wide Gaussian blur).
pub(crate) fn build_soft_focus(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    amount: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_amount;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float2 px = 1.0 / u_resolution;
    half3 blur = half3(0.0);
    float r = 4.0;
    for (int x = -2; x <= 2; x++) {
        for (int y = -2; y <= 2; y++) {
            blur += image.eval(pos + float2(float(x), float(y)) * r * px).rgb;
        }
    }
    blur /= 25.0;
    return half4(mix(color.rgb, blur, u_amount * 0.6), color.a);
}
"#;
    cached_runtime_shader(
        shader_cache, "__soft_focus__", sksl, "SoftFocus", current_filter, |b| {
            let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
            let _ = b.set_uniform_float("u_amount", &[amount]);
        },
    )
}

/// Vibrance: selective saturation boost (less-saturated colors lifted more).
pub(crate) fn build_vibrance(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    _resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    amount: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float u_amount;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float l = dot(color.rgb, half3(0.2126, 0.7152, 0.0722));
    float mx = max(color.r, max(color.g, color.b));
    float mn = min(color.r, min(color.g, color.b));
    float sat = mx - mn;
    float boost = 1.0 + u_amount * (1.0 - sat);
    half3 v = l + (color.rgb - l) * boost;
    return half4(clamp(v, 0.0, 1.0), color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__vibrance__", sksl, "Vibrance", current_filter, |b| {
        let _ = b.set_uniform_float("u_amount", &[amount]);
    })
}

/// Fog: ground-hugging atmospheric mist blended toward a tint color.
pub(crate) fn build_fog(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
    color: Color,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_intensity;
uniform float3 u_color;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float2 uv = pos / u_resolution;
    float grad = 1.0 - uv.y;
    float n = 0.5 + 0.5 * sin(uv.x * 6.0 + uv.y * 3.0);
    float f = clamp(grad * 0.7 + n * 0.3, 0.0, 1.0) * u_intensity;
    half3 outc = mix(color.rgb, half3(u_color), f);
    return half4(outc, color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__fog__", sksl, "Fog", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_intensity", &[intensity]);
        let _ = b.set_uniform_float("u_color", &[color.r, color.g, color.b]);
    })
}

/// Mist: animated drifting haze bands (uses u_time).
pub(crate) fn build_mist(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
    speed: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_time;
uniform float u_intensity;
uniform float u_speed;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float2 uv = pos / u_resolution;
    float t = u_time * u_speed;
    float drift = sin(uv.x * 4.0 + t) * 0.15 + sin(uv.y * 3.0 - t * 0.7) * 0.15;
    float f = smoothstep(0.2, 1.0, uv.y + drift) * u_intensity * 0.6;
    half3 haze = half3(0.85, 0.9, 0.95);
    return half4(mix(color.rgb, haze, f), color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__mist__", sksl, "Mist", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_time", &[time]);
        let _ = b.set_uniform_float("u_intensity", &[intensity]);
        let _ = b.set_uniform_float("u_speed", &[speed]);
    })
}

/// Rain: animated slanted rain columns (uses u_time).
pub(crate) fn build_rain(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
    speed: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_time;
uniform float u_intensity;
uniform float u_speed;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float2 uv = pos / u_resolution;
    float t = u_time * u_speed;
    float aspect = u_resolution.x / u_resolution.y;
    float cols = 80.0 * aspect;
    float x = uv.x * cols;
    float cx = floor(x);
    float seed = fract(sin(cx * 12.9898) * 43758.5453);
    float spd = 0.6 + seed * 0.6;
    float y = fract(uv.y * 1.5 - t * spd + seed * 10.0);
    float lane = smoothstep(0.42, 0.5, fract(x)) * smoothstep(0.5, 0.58, fract(x));
    float drop = smoothstep(0.0, 0.06, y) * smoothstep(0.32, 0.10, y);
    float a = lane * drop * u_intensity;
    half3 rc = half3(0.75, 0.85, 1.0);
    return half4(min(color.rgb + rc * a, 1.0), color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__rain__", sksl, "Rain", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_time", &[time]);
        let _ = b.set_uniform_float("u_intensity", &[intensity]);
        let _ = b.set_uniform_float("u_speed", &[speed]);
    })
}

/// Snow: animated drifting flakes (uses u_time).
pub(crate) fn build_snow(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
    speed: f32,
    size: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_time;
uniform float u_intensity;
uniform float u_speed;
uniform float u_size;

float hash(float2 p) {
    return fract(sin(dot(p, float2(127.1, 311.7))) * 43758.5453);
}

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float2 uv = pos / u_resolution;
    float aspect = u_resolution.x / u_resolution.y;
    float t = u_time * u_speed;
    float accum = 0.0;
    for (int i = 0; i < 3; i++) {
        float fi = float(i);
        float g = (20.0 + fi * 15.0) * aspect;
        float x = uv.x * g;
        float cx = floor(x);
        float seed = hash(float2(cx, fi));
        float sway = sin(t * (0.5 + seed) + seed * 6.2831) * 0.03;
        float yf = fract(uv.y - t * (0.3 + seed * 0.4) + seed * 5.0);
        float lane = smoothstep(0.5, 0.5 - u_size * 0.02, abs(fract(x) - 0.5));
        float flake = smoothstep(u_size * 0.02, 0.0, abs(yf - 0.5)) * lane;
        accum += flake * (0.6 - fi * 0.15);
    }
    accum = clamp(accum, 0.0, 1.0) * u_intensity;
    return half4(min(color.rgb + half3(accum), 1.0), color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__snow__", sksl, "Snow", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_time", &[time]);
        let _ = b.set_uniform_float("u_intensity", &[intensity]);
        let _ = b.set_uniform_float("u_speed", &[speed]);
        let _ = b.set_uniform_float("u_size", &[size]);
    })
}

/// Dust: floating bokeh dust motes (uses u_time).
pub(crate) fn build_dust(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
    speed: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_time;
uniform float u_intensity;
uniform float u_speed;

float hash(float2 p) {
    return fract(sin(dot(p, float2(127.1, 311.7))) * 43758.5453);
}

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float2 uv = pos / u_resolution;
    float t = u_time * u_speed;
    float accum = 0.0;
    for (int i = 0; i < 4; i++) {
        float fi = float(i);
        float g = 8.0 + fi * 6.0;
        float x = uv.x * g + sin(t * 0.3 + fi) * 0.1;
        float y = uv.y * g + t * (0.1 + fi * 0.05);
        float2 cell = floor(float2(x, y));
        float seed = hash(cell + fi);
        float2 cuv = fract(float2(x, y)) - 0.5;
        float d = length(cuv - (seed - 0.5) * 0.6);
        float r = 0.1 + seed * 0.15;
        accum += smoothstep(r, r * 0.5, d) * (0.4 + seed * 0.6);
    }
    accum = clamp(accum * 0.4 * u_intensity, 0.0, 1.0);
    return half4(min(color.rgb + half3(1.0, 0.95, 0.8) * accum, 1.0), color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__dust__", sksl, "Dust", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_time", &[time]);
        let _ = b.set_uniform_float("u_intensity", &[intensity]);
        let _ = b.set_uniform_float("u_speed", &[speed]);
    })
}

/// BokehDots: decorative out-of-focus colored circles.
pub(crate) fn build_bokeh_dots(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
    size: f32,
    color: Color,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_intensity;
uniform float u_size;
uniform float3 u_color;

float hash(float2 p) {
    return fract(sin(dot(p, float2(127.1, 311.7))) * 43758.5453);
}

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float s = max(u_size, 1.0);
    float2 cell = pos / s;
    float2 id = floor(cell);
    float h = hash(id);
    float2 center = (id + 0.5 + (float2(hash(id + 1.3), hash(id + 2.7)) - 0.5) * 0.4) * s;
    float d = length(pos - center);
    float r = s * (0.3 + h * 0.4);
    float circle = smoothstep(r, r * 0.6, d);
    float bright = 0.3 + h * 0.7;
    half3 add = half3(u_color) * circle * bright * u_intensity;
    return half4(min(color.rgb + add, 1.0), color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__bokeh_dots__", sksl, "BokehDots", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_intensity", &[intensity]);
        let _ = b.set_uniform_float("u_size", &[size]);
        let _ = b.set_uniform_float("u_color", &[color.r, color.g, color.b]);
    })
}

/// FilmLook: generic film-stock emulation (contrast + color + grain).
pub(crate) fn build_film_look(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    amount: f32,
    grain: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_time;
uniform float u_amount;
uniform float u_grain;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float2 uv = pos / u_resolution;
    half3 c = color.rgb;
    c = (c - 0.5) * (1.0 + 0.25 * u_amount) + 0.5;
    c.r += 0.04 * u_amount;
    c.b += 0.02 * u_amount;
    c = mix(c, c * 0.92 + 0.04, u_amount * 0.5);
    float g = (fract(sin(dot(uv * u_resolution + floor(u_time * 24.0),
                 float2(12.9898, 78.233))) * 43758.5453) - 0.5) * u_grain * 0.15;
    c += g;
    return half4(clamp(mix(color.rgb, c, u_amount), 0.0, 1.0), color.a);
}
"#;
    cached_runtime_shader(
        shader_cache, "__film_look__", sksl, "FilmLook", current_filter, |b| {
            let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
            let _ = b.set_uniform_float("u_time", &[time]);
            let _ = b.set_uniform_float("u_amount", &[amount]);
            let _ = b.set_uniform_float("u_grain", &[grain]);
        },
    )
}

/// OrangeTeal: split-tone teal-shadow / orange-highlight grade (alt algorithm).
pub(crate) fn build_orange_teal(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    _resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    amount: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float u_amount;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float l = dot(color.rgb, half3(0.2126, 0.7152, 0.0722));
    half3 teal = half3(0.1, 0.4, 0.5);
    half3 orange = half3(1.0, 0.6, 0.25);
    half3 tint = mix(teal, orange, smoothstep(0.25, 0.85, l));
    half3 graded = color.rgb * tint * 2.0;
    graded = color.rgb + (graded - color.rgb) * u_amount;
    return half4(clamp(graded, 0.0, 1.0), color.a);
}
"#;
    cached_runtime_shader(
        shader_cache, "__orange_teal__", sksl, "OrangeTeal", current_filter, |b| {
            let _ = b.set_uniform_float("u_amount", &[amount]);
        },
    )
}

/// CrossProcess: lomo cross-process (crushed greens, cyan shadows, vignette).
pub(crate) fn build_cross_process(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    amount: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_amount;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    half3 c = color.rgb;
    c.r = pow(c.r, 0.85);
    c.g = pow(c.g, 1.2) * 0.95;
    c.b = pow(c.b, 0.85) * 1.05;
    c = (c - 0.5) * (1.0 + 0.2 * u_amount) + 0.5;
    float2 uv = pos / u_resolution;
    float d = distance(uv, float2(0.5));
    float vig = smoothstep(0.8, 0.2, d);
    c *= mix(1.0, vig, u_amount * 0.7);
    return half4(clamp(mix(color.rgb, c, u_amount), 0.0, 1.0), color.a);
}
"#;
    cached_runtime_shader(
        shader_cache, "__cross_process__", sksl, "CrossProcess", current_filter, |b| {
            let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
            let _ = b.set_uniform_float("u_amount", &[amount]);
        },
    )
}

/// Polaroid: instant-film warm wash with a soft vignette.
pub(crate) fn build_polaroid(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    amount: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_amount;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    half3 c = color.rgb;
    c = mix(c, half3(1.0, 0.95, 0.85), 0.25);
    c = (c - 0.5) * (1.0 - 0.15 * u_amount) + 0.52;
    c.g *= 0.98;
    c.b *= 0.95;
    float2 uv = pos / u_resolution;
    float d = distance(uv, float2(0.5));
    float vig = smoothstep(0.75, 0.35, d);
    c *= mix(1.0, vig, u_amount);
    return half4(clamp(mix(color.rgb, c, u_amount), 0.0, 1.0), color.a);
}
"#;
    cached_runtime_shader(
        shader_cache, "__polaroid__", sksl, "Polaroid", current_filter, |b| {
            let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
            let _ = b.set_uniform_float("u_amount", &[amount]);
        },
    )
}

/// FadedFilm: low-contrast matte washed look with lifted blacks.
pub(crate) fn build_faded_film(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    _resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    amount: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float u_amount;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    half3 c = color.rgb;
    c = (c - 0.5) * (1.0 - 0.3 * u_amount) + 0.5;
    c = mix(c, half3(0.9, 0.88, 0.85), 0.15 * u_amount);
    c = mix(c, c * 0.9 + 0.1, u_amount);
    return half4(clamp(mix(color.rgb, c, u_amount), 0.0, 1.0), color.a);
}
"#;
    cached_runtime_shader(
        shader_cache, "__faded_film__", sksl, "FadedFilm", current_filter, |b| {
            let _ = b.set_uniform_float("u_amount", &[amount]);
        },
    )
}

/// Midnight: cool blue-night grade biased toward shadows.
pub(crate) fn build_midnight(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    _resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    amount: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float u_amount;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    half3 c = color.rgb;
    c.r *= (1.0 - 0.2 * u_amount);
    c.b *= (1.0 + 0.35 * u_amount);
    c.g *= (1.0 + 0.05 * u_amount);
    float l = dot(c, half3(0.2126, 0.7152, 0.0722));
    c = mix(c, c * half3(0.6, 0.8, 1.2), (1.0 - l) * u_amount);
    return half4(clamp(mix(color.rgb, c, u_amount), 0.0, 1.0), color.a);
}
"#;
    cached_runtime_shader(
        shader_cache, "__midnight__", sksl, "Midnight", current_filter, |b| {
            let _ = b.set_uniform_float("u_amount", &[amount]);
        },
    )
}

/// Watercolor: soft quantized washes with a paper edge.
pub(crate) fn build_watercolor(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
    edge: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_intensity;
uniform float u_edge;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float levels = 5.0;
    half3 c = floor(color.rgb * levels + 0.5) / levels;
    float2 uv = pos / u_resolution;
    float paper = 0.95 + 0.05 * sin(uv.x * 120.0) * sin(uv.y * 120.0);
    float2 px = 2.0 / u_resolution;
    half3 blur = (image.eval(pos + float2(-px.x, 0.0)).rgb
                + image.eval(pos + float2(px.x, 0.0)).rgb
                + image.eval(pos + float2(0.0, -px.y)).rgb
                + image.eval(pos + float2(0.0, px.y)).rgb) * 0.25;
    c = mix(c, blur, u_intensity * 0.4 * u_edge);
    c *= paper;
    return half4(clamp(mix(color.rgb, c, u_intensity), 0.0, 1.0), color.a);
}
"#;
    cached_runtime_shader(
        shader_cache, "__watercolor__", sksl, "Watercolor", current_filter, |b| {
            let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
            let _ = b.set_uniform_float("u_intensity", &[intensity]);
            let _ = b.set_uniform_float("u_edge", &[edge]);
        },
    )
}

/// Crosshatch: progressive ink hatching in darker regions.
pub(crate) fn build_crosshatch(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    _resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
    density: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float u_intensity;
uniform float u_density;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float l = dot(color.rgb, half3(0.2126, 0.7152, 0.0722));
    float dark = 1.0 - l;
    float dp = max(u_density, 3.0);
    float ink = 0.0;
    ink += smoothstep(0.2, 0.4, dark) * step(0.5, fract((pos.x + pos.y) / dp));
    ink += smoothstep(0.45, 0.6, dark) * step(0.5, fract((pos.x - pos.y) / dp));
    ink += smoothstep(0.7, 0.85, dark) * step(0.5, fract(pos.x / dp));
    ink = clamp(ink, 0.0, 1.0);
    half3 outc = mix(half3(1.0), half3(0.05), ink);
    return half4(mix(half3(1.0), outc, u_intensity), color.a);
}
"#;
    cached_runtime_shader(
        shader_cache, "__crosshatch__", sksl, "Crosshatch", current_filter, |b| {
            let _ = b.set_uniform_float("u_intensity", &[intensity]);
            let _ = b.set_uniform_float("u_density", &[density]);
        },
    )
}

/// Pointillism: painted-dot stipple on a paper background.
pub(crate) fn build_pointillism(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    _resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    dot_size: f32,
    intensity: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float u_dot_size;
uniform float u_intensity;

half4 main(float2 pos) {
    float s = max(u_dot_size, 1.0);
    float2 cell = floor(pos / s);
    float2 center = (cell + 0.5) * s;
    half4 c = image.eval(center);
    float d = length(pos - center);
    float r = s * 0.55;
    float m = smoothstep(r, r * 0.5, d);
    half3 outc = c.rgb * m + half3(0.95) * (1.0 - m);
    return half4(mix(c.rgb, outc, u_intensity), c.a);
}
"#;
    cached_runtime_shader(
        shader_cache, "__pointillism__", sksl, "Pointillism", current_filter, |b| {
            let _ = b.set_uniform_float("u_dot_size", &[dot_size]);
            let _ = b.set_uniform_float("u_intensity", &[intensity]);
        },
    )
}

/// Mosaic: square tile grid with dark grout between tiles.
pub(crate) fn build_mosaic(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    _resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    tile_size: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float u_tile_size;

half4 main(float2 pos) {
    float s = max(u_tile_size, 1.0);
    float2 cell = floor(pos / s);
    float2 center = (cell + 0.5) * s;
    half4 c = image.eval(center);
    float2 local = (pos - cell * s) / s;
    float grout = smoothstep(0.0, 0.08, min(local.x, 1.0 - local.x))
                * smoothstep(0.0, 0.08, min(local.y, 1.0 - local.y));
    half3 outc = mix(half3(0.2), c.rgb, grout);
    return half4(outc, c.a);
}
"#;
    cached_runtime_shader(
        shader_cache, "__mosaic__", sksl, "Mosaic", current_filter, |b| {
            let _ = b.set_uniform_float("u_tile_size", &[tile_size]);
        },
    )
}

/// Ink: bold black ink outline on white (Sobel-style edge + shadow fill).
pub(crate) fn build_ink(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    threshold: f32,
    intensity: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_threshold;
uniform float u_intensity;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float2 px = 1.0 / u_resolution;
    float l = dot(color.rgb, half3(0.2126, 0.7152, 0.0722));
    float lx = dot(image.eval(pos + float2(px.x, 0.0)).rgb, half3(0.2126, 0.7152, 0.0722));
    float ly = dot(image.eval(pos + float2(0.0, px.y)).rgb, half3(0.2126, 0.7152, 0.0722));
    float e = abs(l - lx) + abs(l - ly);
    float edge = smoothstep(u_threshold, u_threshold + 0.1, e);
    float fill = smoothstep(0.6, 0.2, l) * (1.0 - edge);
    float ink = clamp(edge + fill * 0.8, 0.0, 1.0) * u_intensity;
    half3 outc = mix(half3(1.0), half3(0.05), ink);
    return half4(outc, color.a);
}
"#;
    cached_runtime_shader(shader_cache, "__ink__", sksl, "Ink", current_filter, |b| {
        let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
        let _ = b.set_uniform_float("u_threshold", &[threshold]);
        let _ = b.set_uniform_float("u_intensity", &[intensity]);
    })
}

/// PencilColor: colored-pencil sketch keeping the source hue.
pub(crate) fn build_pencil_color(
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    _time: f32,
    current_filter: Option<skia_safe::ImageFilter>,
    intensity: f32,
) -> Option<skia_safe::ImageFilter> {
    let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_intensity;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float2 px = 1.0 / u_resolution;
    float l = dot(color.rgb, half3(0.2126, 0.7152, 0.0722));
    float tl = dot(image.eval(pos + float2(-px.x, -px.y)).rgb, half3(0.2126, 0.7152, 0.0722));
    float tr = dot(image.eval(pos + float2(px.x, -px.y)).rgb, half3(0.2126, 0.7152, 0.0722));
    float bl = dot(image.eval(pos + float2(-px.x, px.y)).rgb, half3(0.2126, 0.7152, 0.0722));
    float br = dot(image.eval(pos + float2(px.x, px.y)).rgb, half3(0.2126, 0.7152, 0.0722));
    float edge = abs(tl + tr - bl - br) * 0.5;
    float stroke = smoothstep(0.1, 0.3, edge);
    half3 base = color.rgb * 0.9;
    half3 col = base * (1.0 - stroke * 0.6);
    float grain = 0.95 + 0.05 * sin(pos.x * 0.5) * sin(pos.y * 0.5);
    col *= grain;
    return half4(mix(color.rgb, col, u_intensity), color.a);
}
"#;
    cached_runtime_shader(
        shader_cache, "__pencil_color__", sksl, "PencilColor", current_filter, |b| {
            let _ = b.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
            let _ = b.set_uniform_float("u_intensity", &[intensity]);
        },
    )
}
