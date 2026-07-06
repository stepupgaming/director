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
    float tl = luma(float3(image.eval(pos + float2(-1.0, -1.0)).rgb));
    float t  = luma(float3(image.eval(pos + float2(0.0, -1.0)).rgb));
    float tr = luma(float3(image.eval(pos + float2(1.0, -1.0)).rgb));
    float l  = luma(float3(image.eval(pos + float2(-1.0, 0.0)).rgb));
    float r  = luma(float3(image.eval(pos + float2(1.0, 0.0)).rgb));
    float bl = luma(float3(image.eval(pos + float2(-1.0, 1.0)).rgb));
    float b  = luma(float3(image.eval(pos + float2(0.0, 1.0)).rgb));
    float br = luma(float3(image.eval(pos + float2(1.0, 1.0)).rgb));
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
    float luma = dot(color.rgb, half3(0.299, 0.587, 0.114));

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
    float luma = dot(color.rgb, half3(0.299, 0.587, 0.114));

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
    float luma = dot(color.rgb, half3(0.299, 0.587, 0.114));

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
    float luma = dot(tex.rgb, half3(0.299, 0.587, 0.114));

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
    float left   = dot(float3(image.eval(pos - float2(offset * u_resolution.x, 0.0)).rgb), float3(0.3, 0.59, 0.11));
    float right  = dot(float3(image.eval(pos + float2(offset * u_resolution.x, 0.0)).rgb), float3(0.3, 0.59, 0.11));
    float top    = dot(float3(image.eval(pos - float2(0.0, offset * u_resolution.y)).rgb), float3(0.3, 0.59, 0.11));
    float bottom = dot(float3(image.eval(pos + float2(0.0, offset * u_resolution.y)).rgb), float3(0.3, 0.59, 0.11));

    float edge = abs(left - right) + abs(top - bottom);

    float hatch = sin((uv.x + uv.y) * 450.0 + hash(uv + floor(u_time * 8.0)) * 10.0) * 0.5 + 0.5;
    float luma = dot(original.rgb, half3(0.3, 0.59, 0.11));

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
    float luma = dot(color.rgb, half3(0.299, 0.587, 0.114));

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
    float l_center = dot(center.rgb, half3(0.299, 0.587, 0.114));
    float l_left   = dot(image.eval(pos - float2(offset * u_resolution.x, 0.0)).rgb, half3(0.299, 0.587, 0.114));
    float l_right  = dot(image.eval(pos + float2(offset * u_resolution.x, 0.0)).rgb, half3(0.299, 0.587, 0.114));
    float l_top    = dot(image.eval(pos - float2(0.0, offset * u_resolution.y)).rgb, half3(0.299, 0.587, 0.114));
    float l_bottom = dot(image.eval(pos + float2(0.0, offset * u_resolution.y)).rgb, half3(0.299, 0.587, 0.114));

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
