use crate::animation::{Animated, TweenableVector};
use crate::element::Element;
use crate::errors::RenderError;
use crate::node::parse_easing;
use crate::types::Color;
use skia_safe::{
    color_filters, image_filters, runtime_effect::RuntimeShaderBuilder, Canvas, ColorMatrix, Paint,
    Rect, RuntimeEffect, TileMode,
};
use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use taffy::style::Style;
use tracing::error;

/// Represents an animatable uniform value for Runtime Shaders.
#[derive(Debug, Clone)]
pub enum ShaderUniform {
    Float(Animated<f32>),
    Vec(Animated<TweenableVector>),
}

impl ShaderUniform {
    pub fn update(&mut self, time: f64) {
        match self {
            ShaderUniform::Float(a) => a.update(time),
            ShaderUniform::Vec(a) => a.update(time),
        }
    }
}

/// Available visual effects that can be applied to nodes.
#[derive(Debug, Clone)]
pub enum EffectType {
    /// Gaussian Blur with animated sigma (radius).
    Blur(Animated<f32>),
    /// 4x5 Color Matrix transform (e.g. Grayscale, Sepia).
    ColorMatrix(Vec<f32>),
    /// Custom SkSL shader with animated uniforms.
    RuntimeShader {
        sksl: String,
        uniforms: HashMap<String, ShaderUniform>,
    },
    /// Drop Shadow effect.
    DropShadow {
        blur: Animated<f32>,
        offset_x: Animated<f32>,
        offset_y: Animated<f32>,
        color: Animated<Color>,
    },
    /// Directional blur (motion blur) along an angle.
    DirectionalBlur {
        /// Blur strength (pixel distance)
        strength: Animated<f32>,
        /// Direction angle in degrees (0 = right, 90 = down)
        angle: Animated<f32>,
        /// Number of samples (quality vs performance, clamped 4-64)
        samples: u32,
    },
    /// Film grain / noise overlay for cinematic look.
    FilmGrain {
        /// Grain intensity (0.0 - 1.0)
        intensity: Animated<f32>,
        /// Grain size/scale (pixels)
        size: Animated<f32>,
    },
    /// Vignette: darkens edges of frame.
    Vignette {
        intensity: Animated<f32>,
        radius: Animated<f32>,
        feather: Animated<f32>,
    },
    /// Chromatic Aberration: RGB channel offset.
    ChromaticAberration(Animated<f32>),
    /// Pixelate: blocky pixelation grid.
    Pixelate(Animated<f32>),
    /// Sharpen: unsharp mask via 3x3 Laplacian.
    Sharpen(Animated<f32>),
    /// Glow: additive bright-area glow.
    Glow {
        intensity: Animated<f32>,
        threshold: Animated<f32>,
    },
    /// Warp: sine-wave displacement distortion.
    Warp {
        amplitude: Animated<f32>,
        frequency: Animated<f32>,
        direction: Animated<f32>,
        speed: Animated<f32>,
    },
    /// Swirl: rotational twist around the center.
    Swirl {
        radius: Animated<f32>,
        angle: Animated<f32>,
    },
    /// Ripple: concentric waves from the center.
    Ripple {
        amplitude: Animated<f32>,
        frequency: Animated<f32>,
        speed: Animated<f32>,
    },
    /// Glass: frosted-glass refraction and blur.
    Glass {
        scale: Animated<f32>,
        intensity: Animated<f32>,
    },
    /// Shake: block-based jitter displacement.
    Shake {
        intensity: Animated<f32>,
        speed: Animated<f32>,
    },
    /// Duotone: luminance-mapped two-color grade.
    Duotone {
        shadow: Color,
        highlight: Color,
    },
    /// Posterize: reduce color levels.
    Posterize(Animated<f32>),
    /// Threshold: black/white luminance cutoff.
    Threshold(Animated<f32>),
    /// Scanlines: CRT-style scanline overlay.
    Scanlines {
        intensity: Animated<f32>,
        count: Animated<f32>,
    },
    /// Glitch: RGB split with block displacement.
    Glitch {
        intensity: Animated<f32>,
        speed: Animated<f32>,
    },
    /// Edge detect: Sobel outline.
    EdgeDetect(Animated<f32>),
    /// Zoom blur: radial motion blur from the center.
    ZoomBlur(Animated<f32>),
    /// Kaleidoscope: mirror symmetry segments.
    Kaleidoscope(Animated<f32>),
    /// Thermal: heat map color spectrum (luminance → blue→cyan→yellow→red→white).
    Thermal,
    /// Liquid: bidirectional sine/cosine coordinate warp.
    Liquid {
        frequency: Animated<f32>,
        amplitude: Animated<f32>,
    },
    /// Acid: fluorescent acid melt (wave distortion + color morph).
    Acid {
        color: Color,
        speed: Animated<f32>,
    },
    /// Halftone: pop-art dot grid (luminance → dot size mapping).
    Halftone(Animated<f32>),
    /// Ascii: terminal ASCII art effect.
    Ascii {
        scale: Animated<f32>,
        color: Color,
    },
    /// Sketch: charcoal pencil sketch (edge detection + hatching + paper texture).
    Sketch(Animated<f32>),
    /// Mirror: quad-mirror symmetry (4-way reflection).
    Mirror,
    /// TriTone: three-color luminance grade with adjustable midpoint.
    TriTone {
        shadow: Color,
        mid: Color,
        highlight: Color,
        midpoint: Animated<f32>,
    },
    /// VHS: combined CRT glitch (jitter + scanlines + noise + RGB shift).
    Vhs {
        distortion: Animated<f32>,
        scanline: Animated<f32>,
    },
    /// NeonEdge: edge detect with configurable glow color and darkened background.
    NeonEdge {
        glow_color: Color,
        threshold: Animated<f32>,
    },
}

impl EffectType {
    pub fn update(&mut self, time: f64) {
        match self {
            EffectType::Blur(a) => a.update(time),
            EffectType::ColorMatrix(_) => {}
            EffectType::RuntimeShader { uniforms, .. } => {
                for val in uniforms.values_mut() {
                    val.update(time);
                }
            }
            EffectType::DropShadow {
                blur,
                offset_x,
                offset_y,
                color,
            } => {
                blur.update(time);
                offset_x.update(time);
                offset_y.update(time);
                color.update(time);
            }
            EffectType::DirectionalBlur {
                strength, angle, ..
            } => {
                strength.update(time);
                angle.update(time);
            }
            EffectType::FilmGrain { intensity, size } => {
                intensity.update(time);
                size.update(time);
            }
            EffectType::Vignette {
                intensity, radius, feather,
            } => {
                intensity.update(time);
                radius.update(time);
                feather.update(time);
            }
            EffectType::ChromaticAberration(a) => a.update(time),
            EffectType::Pixelate(a) => a.update(time),
            EffectType::Sharpen(a) => a.update(time),
            EffectType::Glow {
                intensity, threshold,
            } => {
                intensity.update(time);
                threshold.update(time);
            }
            EffectType::Warp {
                amplitude,
                frequency,
                direction,
                speed,
            } => {
                amplitude.update(time);
                frequency.update(time);
                direction.update(time);
                speed.update(time);
            }
            EffectType::Swirl { radius, angle } => {
                radius.update(time);
                angle.update(time);
            }
            EffectType::Ripple {
                amplitude,
                frequency,
                speed,
            } => {
                amplitude.update(time);
                frequency.update(time);
                speed.update(time);
            }
            EffectType::Glass { scale, intensity } => {
                scale.update(time);
                intensity.update(time);
            }
            EffectType::Shake { intensity, speed } => {
                intensity.update(time);
                speed.update(time);
            }
            EffectType::Duotone { .. } => {}
            EffectType::Posterize(a) => a.update(time),
            EffectType::Threshold(a) => a.update(time),
            EffectType::Scanlines { intensity, count } => {
                intensity.update(time);
                count.update(time);
            }
            EffectType::Glitch { intensity, speed } => {
                intensity.update(time);
                speed.update(time);
            }
            EffectType::EdgeDetect(a) => a.update(time),
            EffectType::ZoomBlur(a) => a.update(time),
            EffectType::Kaleidoscope(a) => a.update(time),
            EffectType::Thermal => {}
            EffectType::Liquid { frequency, amplitude } => {
                frequency.update(time);
                amplitude.update(time);
            }
            EffectType::Acid { speed, .. } => {
                speed.update(time);
            }
            EffectType::Halftone(a) => a.update(time),
            EffectType::Ascii { scale, .. } => {
                scale.update(time);
            }
            EffectType::Sketch(a) => a.update(time),
            EffectType::Mirror => {}
            EffectType::TriTone { midpoint, .. } => {
                midpoint.update(time);
            }
            EffectType::Vhs { distortion, scanline } => {
                distortion.update(time);
                scanline.update(time);
            }
            EffectType::NeonEdge { threshold, .. } => {
                threshold.update(time);
            }
        }
    }
}

pub fn build_effect_filter(
    effects: &[EffectType],
    shader_cache: Option<&Arc<Mutex<HashMap<String, RuntimeEffect>>>>,
    resolution: (f32, f32),
    time: f32,
) -> Option<skia_safe::ImageFilter> {
    let mut current_filter = None;
    for effect in effects {
        match effect {
            EffectType::Blur(sigma) => {
                current_filter = image_filters::blur(
                    (sigma.current_value, sigma.current_value),
                    TileMode::Decal,
                    current_filter,
                    None,
                );
            }
            EffectType::DropShadow {
                blur,
                offset_x,
                offset_y,
                color,
            } => {
                current_filter = image_filters::drop_shadow(
                    (offset_x.current_value, offset_y.current_value),
                    (blur.current_value, blur.current_value),
                    color.current_value.to_skia(),
                    None,
                    current_filter,
                    None,
                );
            }
            EffectType::ColorMatrix(matrix) => {
                if matrix.len() == 20 {
                    if let Ok(m) = matrix.as_slice().try_into() {
                        let m: &[f32; 20] = m;
                        let cm = ColorMatrix::new(
                            m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7], m[8], m[9], m[10],
                            m[11], m[12], m[13], m[14], m[15], m[16], m[17], m[18], m[19],
                        );
                        let cf = color_filters::matrix(&cm, None);
                        current_filter = image_filters::color_filter(cf, current_filter, None);
                    }
                }
            }
            EffectType::RuntimeShader { sksl, uniforms } => {
                if let Some(cache_arc) = shader_cache {
                    let mut cache = cache_arc.lock().unwrap();
                    if !cache.contains_key(sksl) {
                        match RuntimeEffect::make_for_shader(sksl, None) {
                            Ok(effect) => {
                                cache.insert(sksl.clone(), effect);
                            }
                            Err(e) => {
                                error!("Shader compilation error: {}", e);
                                continue;
                            }
                        }
                    }

                    if let Some(effect) = cache.get(sksl) {
                        let mut builder = RuntimeShaderBuilder::new(effect.clone());

                        // Inject Automatic Uniforms
                        let _ = builder
                            .set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
                        let _ = builder.set_uniform_float("u_time", &[time]);

                        for (key, val) in uniforms {
                            match val {
                                ShaderUniform::Float(anim) => {
                                    let _ = builder.set_uniform_float(key, &[anim.current_value]);
                                }
                                ShaderUniform::Vec(anim) => {
                                    let vec_data = &anim.current_value.0;
                                    let _ = builder.set_uniform_float(key, vec_data);
                                }
                            }
                        }
                        // "image" is the standard name for the input texture in SkSL for ImageFilters
                        current_filter =
                            image_filters::runtime_shader(&builder, "image", current_filter);
                    }
                }
            }
            EffectType::DirectionalBlur {
                strength,
                angle,
                samples: _,
            } => {
                // Use fixed 16 samples for simplicity and compatibility
                // (SkSL has restrictions on dynamic loop bounds)
                let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_strength;
uniform float u_angle;

half4 main(float2 pos) {
    float rad = radians(u_angle);
    float2 dir = float2(cos(rad), sin(rad));
    
    half4 color = half4(0.0);
    // Fixed 16 samples for motion blur
    for (int i = 0; i < 16; i++) {
        float t = (float(i) - 7.5) / 8.0;  // Range: -0.9375 to +0.9375
        float2 offset = dir * u_strength * t;
        color += image.eval(pos + offset);
    }
    return color / 16.0;
}
"#;
                if let Some(cache_arc) = shader_cache {
                    let cache_key = "__directional_blur__".to_string();
                    let mut cache = cache_arc.lock().unwrap();
                    if !cache.contains_key(&cache_key) {
                        match RuntimeEffect::make_for_shader(sksl, None) {
                            Ok(effect) => {
                                cache.insert(cache_key.clone(), effect);
                            }
                            Err(e) => {
                                error!("DirectionalBlur shader compilation error: {}", e);
                                continue;
                            }
                        }
                    }
                    if let Some(effect) = cache.get(&cache_key) {
                        let mut builder = RuntimeShaderBuilder::new(effect.clone());
                        let _ = builder
                            .set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
                        let _ = builder.set_uniform_float("u_strength", &[strength.current_value]);
                        let _ = builder.set_uniform_float("u_angle", &[angle.current_value]);
                        current_filter =
                            image_filters::runtime_shader(&builder, "image", current_filter);
                    }
                }
            }
            EffectType::FilmGrain { intensity, size } => {
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
    
    // Animated grain based on time and position
    float2 grain_pos = floor(pos / u_size) + floor(u_time * 24.0);
    float grain = hash(grain_pos) * 2.0 - 1.0;
    
    color.rgb += half3(grain * u_intensity);
    return color;
}
"#;
                if let Some(cache_arc) = shader_cache {
                    let cache_key = "__film_grain__".to_string();
                    let mut cache = cache_arc.lock().unwrap();
                    if !cache.contains_key(&cache_key) {
                        match RuntimeEffect::make_for_shader(sksl, None) {
                            Ok(effect) => {
                                cache.insert(cache_key.clone(), effect);
                            }
                            Err(e) => {
                                error!("FilmGrain shader compilation error: {}", e);
                                continue;
                            }
                        }
                    }
                    if let Some(effect) = cache.get(&cache_key) {
                        let mut builder = RuntimeShaderBuilder::new(effect.clone());
                        let _ = builder
                            .set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
                        let _ = builder.set_uniform_float("u_time", &[time]);
                        let _ =
                            builder.set_uniform_float("u_intensity", &[intensity.current_value]);
                        let _ = builder.set_uniform_float("u_size", &[size.current_value]);
                        current_filter =
                            image_filters::runtime_shader(&builder, "image", current_filter);
                    }
                }
            }
            EffectType::Vignette {
                intensity,
                radius,
                feather,
            } => if let Some(cache_arc) = shader_cache {
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
                let cache_key = "__vignette__".to_string();
                let mut cache = cache_arc.lock().unwrap();
                if !cache.contains_key(&cache_key) {
                    match RuntimeEffect::make_for_shader(sksl, None) {
                        Ok(effect) => cache.insert(cache_key.clone(), effect),
                        Err(e) => {
                            error!("Vignette shader compilation error: {}", e);
                            continue;
                        }
                    };
                }
                if let Some(effect) = cache.get(&cache_key) {
                    let mut builder = RuntimeShaderBuilder::new(effect.clone());
                    let _ = builder.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
                    let _ = builder.set_uniform_float("u_intensity", &[intensity.current_value]);
                    let _ = builder.set_uniform_float("u_radius", &[radius.current_value]);
                    let _ = builder.set_uniform_float("u_feather", &[feather.current_value]);
                    current_filter = image_filters::runtime_shader(&builder, "image", current_filter);
                }
            }
            EffectType::ChromaticAberration(strength) => if let Some(cache_arc) = shader_cache {
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
                let cache_key = "__chromatic_aberration__".to_string();
                let mut cache = cache_arc.lock().unwrap();
                if !cache.contains_key(&cache_key) {
                    match RuntimeEffect::make_for_shader(sksl, None) {
                        Ok(effect) => cache.insert(cache_key.clone(), effect),
                        Err(e) => {
                            error!("ChromaticAberration shader compilation error: {}", e);
                            continue;
                        }
                    };
                }
                if let Some(effect) = cache.get(&cache_key) {
                    let mut builder = RuntimeShaderBuilder::new(effect.clone());
                    let _ = builder.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
                    let _ = builder.set_uniform_float("u_strength", &[strength.current_value]);
                    current_filter = image_filters::runtime_shader(&builder, "image", current_filter);
                }
            }
            EffectType::Pixelate(block_size) => if let Some(cache_arc) = shader_cache {
                let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_block_size;

half4 main(float2 pos) {
    float2 block = floor(pos / u_block_size) * u_block_size + u_block_size * 0.5;
    return image.eval(block);
}
"#;
                let cache_key = "__pixelate__".to_string();
                let mut cache = cache_arc.lock().unwrap();
                if !cache.contains_key(&cache_key) {
                    match RuntimeEffect::make_for_shader(sksl, None) {
                        Ok(effect) => cache.insert(cache_key.clone(), effect),
                        Err(e) => {
                            error!("Pixelate shader compilation error: {}", e);
                            continue;
                        }
                    };
                }
                if let Some(effect) = cache.get(&cache_key) {
                    let mut builder = RuntimeShaderBuilder::new(effect.clone());
                    let _ = builder.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
                    let _ = builder.set_uniform_float("u_block_size", &[block_size.current_value]);
                    current_filter = image_filters::runtime_shader(&builder, "image", current_filter);
                }
            }
            EffectType::Sharpen(strength) => if let Some(cache_arc) = shader_cache {
                let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_strength;

half4 main(float2 pos) {
    half4 center = image.eval(pos);
    half4 n[8];
    n[0] = image.eval(pos + float2(-1, -1));
    n[1] = image.eval(pos + float2(0, -1));
    n[2] = image.eval(pos + float2(1, -1));
    n[3] = image.eval(pos + float2(-1, 0));
    n[4] = image.eval(pos + float2(1, 0));
    n[5] = image.eval(pos + float2(-1, 1));
    n[6] = image.eval(pos + float2(0, 1));
    n[7] = image.eval(pos + float2(1, 1));
    half4 sum = n[0] + n[1] + n[2] + n[3] + n[4] + n[5] + n[6] + n[7];
    half4 laplacian = center * 9.0 - sum;
    return center + laplacian * u_strength;
}
"#;
                let cache_key = "__sharpen__".to_string();
                let mut cache = cache_arc.lock().unwrap();
                if !cache.contains_key(&cache_key) {
                    match RuntimeEffect::make_for_shader(sksl, None) {
                        Ok(effect) => cache.insert(cache_key.clone(), effect),
                        Err(e) => {
                            error!("Sharpen shader compilation error: {}", e);
                            continue;
                        }
                    };
                }
                if let Some(effect) = cache.get(&cache_key) {
                    let mut builder = RuntimeShaderBuilder::new(effect.clone());
                    let _ = builder.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
                    let _ = builder.set_uniform_float("u_strength", &[strength.current_value]);
                    current_filter = image_filters::runtime_shader(&builder, "image", current_filter);
                }
            }
            EffectType::Glow {
                intensity,
                threshold,
            } => if let Some(cache_arc) = shader_cache {
                let sksl = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_intensity;
uniform float u_threshold;

half4 main(float2 pos) {
    half4 color = image.eval(pos);
    float lum = dot(color.rgb, half3(0.2126, 0.7152, 0.0722));
    half4 blur = half4(0);
    int radius = 4;
    int count = 0;
    for (int x = -radius; x <= radius; x += 2) {
        for (int y = -radius; y <= radius; y += 2) {
            blur += image.eval(pos + float2(float(x), float(y)));
            count++;
        }
    }
    blur /= float(count);
    float glow_lum = dot(blur.rgb, half3(0.2126, 0.7152, 0.0722));
    float mask = smoothstep(u_threshold, 1.0, glow_lum) * u_intensity;
    return half4(min(color.rgb + blur.rgb * mask, 1.0), color.a);
}
"#;
                let cache_key = "__glow__".to_string();
                let mut cache = cache_arc.lock().unwrap();
                if !cache.contains_key(&cache_key) {
                    match RuntimeEffect::make_for_shader(sksl, None) {
                        Ok(effect) => cache.insert(cache_key.clone(), effect),
                        Err(e) => {
                            error!("Glow shader compilation error: {}", e);
                            continue;
                        }
                    };
                }
                if let Some(effect) = cache.get(&cache_key) {
                    let mut builder = RuntimeShaderBuilder::new(effect.clone());
                    let _ = builder.set_uniform_float("u_resolution", &[resolution.0, resolution.1]);
                    let _ = builder.set_uniform_float("u_intensity", &[intensity.current_value]);
                    let _ = builder.set_uniform_float("u_threshold", &[threshold.current_value]);
                    current_filter = image_filters::runtime_shader(&builder, "image", current_filter);
                }
            }
            EffectType::Warp {
                amplitude,
                frequency,
                direction,
                speed,
            } => {
                current_filter = super::effect_shaders::build_warp(
                    shader_cache,
                    resolution,
                    time,
                    current_filter,
                    amplitude.current_value,
                    frequency.current_value,
                    direction.current_value,
                    speed.current_value,
                );
            }
            EffectType::Swirl { radius, angle } => {
                current_filter = super::effect_shaders::build_swirl(
                    shader_cache,
                    resolution,
                    time,
                    current_filter,
                    radius.current_value,
                    angle.current_value,
                );
            }
            EffectType::Ripple {
                amplitude,
                frequency,
                speed,
            } => {
                current_filter = super::effect_shaders::build_ripple(
                    shader_cache,
                    resolution,
                    time,
                    current_filter,
                    amplitude.current_value,
                    frequency.current_value,
                    speed.current_value,
                );
            }
            EffectType::Glass { scale, intensity } => {
                current_filter = super::effect_shaders::build_glass(
                    shader_cache,
                    resolution,
                    time,
                    current_filter,
                    scale.current_value,
                    intensity.current_value,
                );
            }
            EffectType::Shake { intensity, speed } => {
                current_filter = super::effect_shaders::build_shake(
                    shader_cache,
                    resolution,
                    time,
                    current_filter,
                    intensity.current_value,
                    speed.current_value,
                );
            }
            EffectType::Duotone { shadow, highlight } => {
                current_filter = super::effect_shaders::build_duotone(
                    shader_cache,
                    resolution,
                    time,
                    current_filter,
                    *shadow,
                    *highlight,
                );
            }
            EffectType::Posterize(levels) => {
                current_filter = super::effect_shaders::build_posterize(
                    shader_cache,
                    resolution,
                    time,
                    current_filter,
                    levels.current_value,
                );
            }
            EffectType::Threshold(level) => {
                current_filter = super::effect_shaders::build_threshold(
                    shader_cache,
                    resolution,
                    time,
                    current_filter,
                    level.current_value,
                );
            }
            EffectType::Scanlines { intensity, count } => {
                current_filter = super::effect_shaders::build_scanlines(
                    shader_cache,
                    resolution,
                    time,
                    current_filter,
                    intensity.current_value,
                    count.current_value,
                );
            }
            EffectType::Glitch { intensity, speed } => {
                current_filter = super::effect_shaders::build_glitch(
                    shader_cache,
                    resolution,
                    time,
                    current_filter,
                    intensity.current_value,
                    speed.current_value,
                );
            }
            EffectType::EdgeDetect(intensity) => {
                current_filter = super::effect_shaders::build_edge_detect(
                    shader_cache,
                    resolution,
                    time,
                    current_filter,
                    intensity.current_value,
                );
            }
            EffectType::ZoomBlur(strength) => {
                current_filter = super::effect_shaders::build_zoom_blur(
                    shader_cache,
                    resolution,
                    time,
                    current_filter,
                    strength.current_value,
                );
            }
            EffectType::Kaleidoscope(segments) => {
                current_filter = super::effect_shaders::build_kaleidoscope(
                    shader_cache,
                    resolution,
                    time,
                    current_filter,
                    segments.current_value,
                );
            }
            EffectType::Thermal => {
                current_filter = super::effect_shaders::build_thermal(
                    shader_cache, resolution, time, current_filter,
                );
            }
            EffectType::Liquid { frequency, amplitude } => {
                current_filter = super::effect_shaders::build_liquid(
                    shader_cache, resolution, time, current_filter,
                    frequency.current_value, amplitude.current_value,
                );
            }
            EffectType::Acid { color, speed } => {
                current_filter = super::effect_shaders::build_acid(
                    shader_cache, resolution, time, current_filter,
                    *color, speed.current_value,
                );
            }
            EffectType::Halftone(dot_size) => {
                current_filter = super::effect_shaders::build_halftone(
                    shader_cache, resolution, time, current_filter,
                    dot_size.current_value,
                );
            }
            EffectType::Ascii { scale, color } => {
                current_filter = super::effect_shaders::build_ascii(
                    shader_cache, resolution, time, current_filter,
                    scale.current_value, *color,
                );
            }
            EffectType::Sketch(contrast) => {
                current_filter = super::effect_shaders::build_sketch(
                    shader_cache, resolution, time, current_filter,
                    contrast.current_value,
                );
            }
            EffectType::Mirror => {
                current_filter = super::effect_shaders::build_mirror(
                    shader_cache, resolution, time, current_filter,
                );
            }
            EffectType::TriTone { shadow, mid, highlight, midpoint } => {
                current_filter = super::effect_shaders::build_tritone(
                    shader_cache, resolution, time, current_filter,
                    *shadow, *mid, *highlight, midpoint.current_value,
                );
            }
            EffectType::Vhs { distortion, scanline } => {
                current_filter = super::effect_shaders::build_vhs(
                    shader_cache, resolution, time, current_filter,
                    distortion.current_value, scanline.current_value,
                );
            }
            EffectType::NeonEdge { glow_color, threshold } => {
                current_filter = super::effect_shaders::build_neon_edge(
                    shader_cache, resolution, time, current_filter,
                    *glow_color, threshold.current_value,
                );
            }
        }
    }
    current_filter
}

/// A node that applies visual effects (filters) to its children.
///
/// It does not render content itself but wraps its children in a layer with applied image filters.
pub struct EffectNode {
    pub effects: Vec<EffectType>,
    pub style: Style,
    pub shader_cache: Arc<Mutex<HashMap<String, RuntimeEffect>>>,
    pub current_time: f32,
}

impl Clone for EffectNode {
    fn clone(&self) -> Self {
        Self {
            effects: self.effects.clone(),
            style: self.style.clone(),
            shader_cache: self.shader_cache.clone(),
            current_time: self.current_time,
        }
    }
}

impl fmt::Debug for EffectNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EffectNode")
            .field("effects", &self.effects)
            .finish()
    }
}

impl Element for EffectNode {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout_style(&self) -> Style {
        self.style.clone()
    }

    fn update(&mut self, time: f64) -> bool {
        self.current_time = time as f32;
        for effect in &mut self.effects {
            effect.update(time);
        }
        true
    }

    fn render(
        &self,
        canvas: &Canvas,
        rect: Rect,
        opacity: f32,
        draw_children: &mut dyn FnMut(&Canvas),
    ) -> Result<(), RenderError> {
        let resolution = (rect.width(), rect.height());
        let filter = build_effect_filter(
            &self.effects,
            Some(&self.shader_cache),
            resolution,
            self.current_time,
        );

        let mut paint = Paint::default();
        paint.set_alpha_f(opacity);
        if let Some(f) = filter {
            paint.set_image_filter(f);
        }

        // Do not restrict bounds to rect, otherwise shadows/blurs are clipped
        canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default().paint(&paint));
        draw_children(canvas);
        canvas.restore();
        Ok(())
    }

    fn animate_property(
        &mut self,
        property: &str,
        start: f32,
        target: f32,
        duration: f64,
        easing: &str,
    ) {
        let ease_fn = parse_easing(easing);
        for effect in &mut self.effects {
            if let EffectType::RuntimeShader { uniforms, .. } = effect {
                if let Some(anim) = uniforms.get_mut(property) {
                    if let ShaderUniform::Float(a) = anim {
                        a.add_segment(start, target, duration, ease_fn);
                    }
                }
            }
        }
    }

    fn animate_property_spring(
        &mut self,
        property: &str,
        start: Option<f32>,
        target: f32,
        config: crate::animation::SpringConfig,
    ) {
        for effect in &mut self.effects {
            if let EffectType::RuntimeShader { uniforms, .. } = effect {
                if let Some(anim) = uniforms.get_mut(property) {
                    if let ShaderUniform::Float(a) = anim {
                        if let Some(s) = start {
                            a.add_spring_with_start(s, target, config);
                        } else {
                            a.add_spring(target, config);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod shader_compile_tests {
    use super::*;
    use crate::animation::Animated;
    use crate::types::Color;

    fn cache() -> Arc<Mutex<HashMap<String, RuntimeEffect>>> {
        Arc::new(Mutex::new(HashMap::new()))
    }

    /// Every extended SkSL effect must compile and build an ImageFilter.
    #[test]
    fn new_shader_effects_compile() {
        let cache = cache();
        let sc = Some(&cache);
        let res = (64.0_f32, 64.0_f32);
        let t = 0.0_f32;
        let cases: Vec<EffectType> = vec![
            EffectType::Warp {
                amplitude: Animated::new(1.0),
                frequency: Animated::new(0.02),
                direction: Animated::new(0.0),
                speed: Animated::new(1.0),
            },
            EffectType::Swirl {
                radius: Animated::new(50.0),
                angle: Animated::new(180.0),
            },
            EffectType::Ripple {
                amplitude: Animated::new(2.0),
                frequency: Animated::new(0.05),
                speed: Animated::new(1.0),
            },
            EffectType::Glass {
                scale: Animated::new(0.01),
                intensity: Animated::new(2.0),
            },
            EffectType::Shake {
                intensity: Animated::new(4.0),
                speed: Animated::new(10.0),
            },
            EffectType::Duotone {
                shadow: Color::new(0.0, 0.0, 0.2, 1.0),
                highlight: Color::new(1.0, 0.8, 0.4, 1.0),
            },
            EffectType::Posterize(Animated::new(4.0)),
            EffectType::Threshold(Animated::new(0.5)),
            EffectType::Scanlines {
                intensity: Animated::new(0.4),
                count: Animated::new(200.0),
            },
            EffectType::Glitch {
                intensity: Animated::new(1.0),
                speed: Animated::new(10.0),
            },
            EffectType::EdgeDetect(Animated::new(1.0)),
            EffectType::ZoomBlur(Animated::new(0.3)),
            EffectType::Kaleidoscope(Animated::new(6.0)),
        ];
        for (i, eff) in cases.iter().enumerate() {
            let filter = build_effect_filter(std::slice::from_ref(eff), sc, res, t);
            assert!(
                filter.is_some(),
                "effect #{i} ({:?}) did not produce an ImageFilter (SkSL compile failed)",
                eff
            );
        }
    }

    /// Every new shader-color-panels effect must compile and build an ImageFilter.
    #[test]
    fn new_shader_panels_effects_compile() {
        let cache = cache();
        let sc = Some(&cache);
        let res = (64.0_f32, 64.0_f32);
        let t = 0.0_f32;
        let cases: Vec<EffectType> = vec![
            EffectType::Thermal,
            EffectType::Liquid {
                frequency: Animated::new(12.0),
                amplitude: Animated::new(0.015),
            },
            EffectType::Acid {
                color: Color::new(0.063, 0.725, 0.506, 1.0),
                speed: Animated::new(1.8),
            },
            EffectType::Halftone(Animated::new(55.0)),
            EffectType::Ascii {
                scale: Animated::new(90.0),
                color: Color::new(0.0, 1.0, 0.255, 1.0),
            },
            EffectType::Sketch(Animated::new(1.2)),
            EffectType::Mirror,
            EffectType::TriTone {
                shadow: Color::new(0.031, 0.031, 0.039, 1.0),
                mid: Color::new(0.231, 0.510, 0.965, 1.0),
                highlight: Color::new(0.376, 0.647, 0.980, 1.0),
                midpoint: Animated::new(0.45),
            },
            EffectType::Vhs {
                distortion: Animated::new(0.35),
                scanline: Animated::new(0.5),
            },
            EffectType::NeonEdge {
                glow_color: Color::new(0.0, 1.0, 0.8, 1.0),
                threshold: Animated::new(0.12),
            },
        ];
        for (i, eff) in cases.iter().enumerate() {
            let filter = build_effect_filter(std::slice::from_ref(eff), sc, res, t);
            assert!(
                filter.is_some(),
                "new shader #{i} ({:?}) did not produce an ImageFilter (SkSL compile failed)",
                eff
            );
        }
    }
}
