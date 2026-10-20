use crate::animation::{Animated, TweenableVector};
use crate::element::Element;
use crate::errors::RenderError;
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
    /// Fisheye: radial magnification (or pinch when negative).
    Fisheye(Animated<f32>),
    /// Bulge: localized magnification/pinch around the center.
    Bulge {
        strength: Animated<f32>,
        radius: Animated<f32>,
    },
    /// Barrel: barrel/pincushion lens distortion.
    Barrel(Animated<f32>),
    /// LensFlare: additive radial glow and streak.
    LensFlare {
        intensity: Animated<f32>,
        color: Color,
    },
    /// Noir: high-contrast monochrome.
    Noir(Animated<f32>),
    /// Cyberpunk: magenta/cyan color grade.
    Cyberpunk(Animated<f32>),
    /// Vintage: faded warm film grade.
    Vintage(Animated<f32>),
    /// Gameboy: 4-tone green palette.
    Gameboy,
    /// BleachBypass: desaturated high-contrast film look.
    BleachBypass(Animated<f32>),
    /// TealOrange: cinematic teal-shadow / orange-highlight grade.
    TealOrange(Animated<f32>),
    /// Plasma: animated color plasma overlay.
    Plasma {
        scale: Animated<f32>,
        speed: Animated<f32>,
    },
    /// Hologram: chromatic scanline cyan hologram.
    Hologram(Animated<f32>),
    /// Emboss: directional emboss.
    Emboss {
        angle: Animated<f32>,
        strength: Animated<f32>,
    },
    /// Toon: cel-shaded posterize with outline.
    Toon {
        levels: Animated<f32>,
        edge: Animated<f32>,
    },
    /// OilPaint: quantized color blocks.
    OilPaint {
        block_size: Animated<f32>,
        levels: Animated<f32>,
    },
    /// LightLeak: warm radial gradient overlay.
    LightLeak {
        intensity: Animated<f32>,
        color: Color,
    },
    /// Aurora: animated vertical light bands.
    Aurora {
        intensity: Animated<f32>,
        speed: Animated<f32>,
    },
    /// SunRays: radial light rays from the center.
    SunRays {
        intensity: Animated<f32>,
        angle: Animated<f32>,
        speed: Animated<f32>,
    },
    /// SkinSmooth: edge-preserving (bilateral-ish) smoothing for portraits.
    SkinSmooth {
        intensity: Animated<f32>,
        radius: Animated<f32>,
    },
    /// BeautyGlow: soft diffuse glow biased to bright areas.
    BeautyGlow {
        intensity: Animated<f32>,
        threshold: Animated<f32>,
    },
    /// SoftFocus: dreamy diffusion (source mixed with a wide blur).
    SoftFocus {
        amount: Animated<f32>,
    },
    /// Vibrance: selective saturation boost (lifts low-saturation colors more).
    Vibrance {
        amount: Animated<f32>,
    },
    /// Fog: ground-hugging atmospheric mist toward a tint color.
    Fog {
        intensity: Animated<f32>,
        color: Color,
    },
    /// Mist: animated drifting haze bands.
    Mist {
        intensity: Animated<f32>,
        speed: Animated<f32>,
    },
    /// Rain: animated slanted rain columns.
    Rain {
        intensity: Animated<f32>,
        speed: Animated<f32>,
    },
    /// Snow: animated drifting snow flakes.
    Snow {
        intensity: Animated<f32>,
        speed: Animated<f32>,
        size: Animated<f32>,
    },
    /// Dust: floating bokeh dust motes.
    Dust {
        intensity: Animated<f32>,
        speed: Animated<f32>,
    },
    /// BokehDots: decorative out-of-focus colored circles.
    BokehDots {
        intensity: Animated<f32>,
        size: Animated<f32>,
        color: Color,
    },
    /// FilmLook: film-stock emulation (contrast + color + grain).
    FilmLook {
        amount: Animated<f32>,
        grain: Animated<f32>,
    },
    /// OrangeTeal: split-tone teal-shadow/orange-highlight grade (alt algorithm).
    OrangeTeal {
        amount: Animated<f32>,
    },
    /// CrossProcess: lomo cross-process (crushed greens, cyan shadows, vignette).
    CrossProcess {
        amount: Animated<f32>,
    },
    /// Polaroid: instant-film warm wash with a soft vignette.
    Polaroid {
        amount: Animated<f32>,
    },
    /// FadedFilm: low-contrast matte washed look with lifted blacks.
    FadedFilm {
        amount: Animated<f32>,
    },
    /// Midnight: cool blue-night grade biased to shadows.
    Midnight {
        amount: Animated<f32>,
    },
    /// Watercolor: soft quantized washes with a paper edge.
    Watercolor {
        intensity: Animated<f32>,
        edge: Animated<f32>,
    },
    /// Crosshatch: progressive ink hatching in darker regions.
    Crosshatch {
        intensity: Animated<f32>,
        density: Animated<f32>,
    },
    /// Pointillism: painted-dot stipple on a paper background.
    Pointillism {
        dot_size: Animated<f32>,
        intensity: Animated<f32>,
    },
    /// Mosaic: square tile grid with dark grout between tiles.
    Mosaic {
        tile_size: Animated<f32>,
    },
    /// Ink: bold black ink outline on white.
    Ink {
        threshold: Animated<f32>,
        intensity: Animated<f32>,
    },
    /// PencilColor: colored-pencil sketch keeping the source hue.
    PencilColor {
        intensity: Animated<f32>,
    },
}

impl EffectType {
    /// Stable PascalCase tag used by project JSON `"type"` fields.
    pub fn type_tag(&self) -> &'static str {
        match self {
            EffectType::Blur(_) => "Blur",
            EffectType::ColorMatrix(_) => "ColorMatrix",
            EffectType::RuntimeShader { .. } => "RuntimeShader",
            EffectType::DropShadow { .. } => "DropShadow",
            EffectType::DirectionalBlur { .. } => "DirectionalBlur",
            EffectType::FilmGrain { .. } => "FilmGrain",
            EffectType::Vignette { .. } => "Vignette",
            EffectType::ChromaticAberration(_) => "ChromaticAberration",
            EffectType::Pixelate(_) => "Pixelate",
            EffectType::Sharpen(_) => "Sharpen",
            EffectType::Glow { .. } => "Glow",
            EffectType::Warp { .. } => "Warp",
            EffectType::Swirl { .. } => "Swirl",
            EffectType::Ripple { .. } => "Ripple",
            EffectType::Glass { .. } => "Glass",
            EffectType::Shake { .. } => "Shake",
            EffectType::Duotone { .. } => "Duotone",
            EffectType::Posterize(_) => "Posterize",
            EffectType::Threshold(_) => "Threshold",
            EffectType::Scanlines { .. } => "Scanlines",
            EffectType::Glitch { .. } => "Glitch",
            EffectType::EdgeDetect(_) => "EdgeDetect",
            EffectType::ZoomBlur(_) => "ZoomBlur",
            EffectType::Kaleidoscope(_) => "Kaleidoscope",
            EffectType::Thermal => "Thermal",
            EffectType::Liquid { .. } => "Liquid",
            EffectType::Acid { .. } => "Acid",
            EffectType::Halftone(_) => "Halftone",
            EffectType::Ascii { .. } => "Ascii",
            EffectType::Sketch(_) => "Sketch",
            EffectType::Mirror => "Mirror",
            EffectType::TriTone { .. } => "TriTone",
            EffectType::Vhs { .. } => "Vhs",
            EffectType::NeonEdge { .. } => "NeonEdge",
            EffectType::Fisheye(_) => "Fisheye",
            EffectType::Bulge { .. } => "Bulge",
            EffectType::Barrel(_) => "Barrel",
            EffectType::LensFlare { .. } => "LensFlare",
            EffectType::Noir(_) => "Noir",
            EffectType::Cyberpunk(_) => "Cyberpunk",
            EffectType::Vintage(_) => "Vintage",
            EffectType::Gameboy => "Gameboy",
            EffectType::BleachBypass(_) => "BleachBypass",
            EffectType::TealOrange(_) => "TealOrange",
            EffectType::Plasma { .. } => "Plasma",
            EffectType::Hologram(_) => "Hologram",
            EffectType::Emboss { .. } => "Emboss",
            EffectType::Toon { .. } => "Toon",
            EffectType::OilPaint { .. } => "OilPaint",
            EffectType::LightLeak { .. } => "LightLeak",
            EffectType::Aurora { .. } => "Aurora",
            EffectType::SunRays { .. } => "SunRays",
            EffectType::SkinSmooth { .. } => "SkinSmooth",
            EffectType::BeautyGlow { .. } => "BeautyGlow",
            EffectType::SoftFocus { .. } => "SoftFocus",
            EffectType::Vibrance { .. } => "Vibrance",
            EffectType::Fog { .. } => "Fog",
            EffectType::Mist { .. } => "Mist",
            EffectType::Rain { .. } => "Rain",
            EffectType::Snow { .. } => "Snow",
            EffectType::Dust { .. } => "Dust",
            EffectType::BokehDots { .. } => "BokehDots",
            EffectType::FilmLook { .. } => "FilmLook",
            EffectType::OrangeTeal { .. } => "OrangeTeal",
            EffectType::CrossProcess { .. } => "CrossProcess",
            EffectType::Polaroid { .. } => "Polaroid",
            EffectType::FadedFilm { .. } => "FadedFilm",
            EffectType::Midnight { .. } => "Midnight",
            EffectType::Watercolor { .. } => "Watercolor",
            EffectType::Crosshatch { .. } => "Crosshatch",
            EffectType::Pointillism { .. } => "Pointillism",
            EffectType::Mosaic { .. } => "Mosaic",
            EffectType::Ink { .. } => "Ink",
            EffectType::PencilColor { .. } => "PencilColor",
        }
    }

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
            EffectType::Fisheye(a) => a.update(time),
            EffectType::Bulge { strength, radius } => {
                strength.update(time);
                radius.update(time);
            }
            EffectType::Barrel(a) => a.update(time),
            EffectType::LensFlare { intensity, .. } => {
                intensity.update(time);
            }
            EffectType::Noir(a) => a.update(time),
            EffectType::Cyberpunk(a) => a.update(time),
            EffectType::Vintage(a) => a.update(time),
            EffectType::Gameboy => {}
            EffectType::BleachBypass(a) => a.update(time),
            EffectType::TealOrange(a) => a.update(time),
            EffectType::Plasma { scale, speed } => {
                scale.update(time);
                speed.update(time);
            }
            EffectType::Hologram(a) => a.update(time),
            EffectType::Emboss { angle, strength } => {
                angle.update(time);
                strength.update(time);
            }
            EffectType::Toon { levels, edge } => {
                levels.update(time);
                edge.update(time);
            }
            EffectType::OilPaint { block_size, levels } => {
                block_size.update(time);
                levels.update(time);
            }
            EffectType::LightLeak { intensity, .. } => {
                intensity.update(time);
            }
            EffectType::Aurora { intensity, speed } => {
                intensity.update(time);
                speed.update(time);
            }
            EffectType::SunRays { intensity, angle, speed } => {
                intensity.update(time);
                angle.update(time);
                speed.update(time);
            }
            EffectType::SkinSmooth { intensity, radius } => {
                intensity.update(time);
                radius.update(time);
            }
            EffectType::BeautyGlow { intensity, threshold } => {
                intensity.update(time);
                threshold.update(time);
            }
            EffectType::SoftFocus { amount } => amount.update(time),
            EffectType::Vibrance { amount } => amount.update(time),
            EffectType::Fog { intensity, .. } => {
                intensity.update(time);
            }
            EffectType::Mist { intensity, speed } => {
                intensity.update(time);
                speed.update(time);
            }
            EffectType::Rain { intensity, speed } => {
                intensity.update(time);
                speed.update(time);
            }
            EffectType::Snow { intensity, speed, size } => {
                intensity.update(time);
                speed.update(time);
                size.update(time);
            }
            EffectType::Dust { intensity, speed } => {
                intensity.update(time);
                speed.update(time);
            }
            EffectType::BokehDots { intensity, size, .. } => {
                intensity.update(time);
                size.update(time);
            }
            EffectType::FilmLook { amount, grain } => {
                amount.update(time);
                grain.update(time);
            }
            EffectType::OrangeTeal { amount } => amount.update(time),
            EffectType::CrossProcess { amount } => amount.update(time),
            EffectType::Polaroid { amount } => amount.update(time),
            EffectType::FadedFilm { amount } => amount.update(time),
            EffectType::Midnight { amount } => amount.update(time),
            EffectType::Watercolor { intensity, edge } => {
                intensity.update(time);
                edge.update(time);
            }
            EffectType::Crosshatch { intensity, density } => {
                intensity.update(time);
                density.update(time);
            }
            EffectType::Pointillism { dot_size, intensity } => {
                dot_size.update(time);
                intensity.update(time);
            }
            EffectType::Mosaic { tile_size } => tile_size.update(time),
            EffectType::Ink { threshold, intensity } => {
                threshold.update(time);
                intensity.update(time);
            }
            EffectType::PencilColor { intensity } => intensity.update(time),
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
                current_filter = super::effect_shaders::build_directional_blur(
                    shader_cache,
                    resolution,
                    time,
                    current_filter,
                    strength.current_value,
                    angle.current_value,
                );
            }
            EffectType::FilmGrain { intensity, size } => {
                current_filter = super::effect_shaders::build_film_grain(
                    shader_cache,
                    resolution,
                    time,
                    current_filter,
                    intensity.current_value,
                    size.current_value,
                );
            }
            EffectType::Vignette {
                intensity,
                radius,
                feather,
            } => {
                current_filter = super::effect_shaders::build_vignette(
                    shader_cache,
                    resolution,
                    time,
                    current_filter,
                    intensity.current_value,
                    radius.current_value,
                    feather.current_value,
                );
            }
            EffectType::ChromaticAberration(strength) => {
                current_filter = super::effect_shaders::build_chromatic_aberration(
                    shader_cache,
                    resolution,
                    time,
                    current_filter,
                    strength.current_value,
                );
            }
            EffectType::Pixelate(block_size) => {
                current_filter = super::effect_shaders::build_pixelate(
                    shader_cache,
                    resolution,
                    time,
                    current_filter,
                    block_size.current_value,
                );
            }
            EffectType::Sharpen(strength) => {
                current_filter = super::effect_shaders::build_sharpen(
                    shader_cache,
                    resolution,
                    time,
                    current_filter,
                    strength.current_value,
                );
            }
            EffectType::Glow {
                intensity,
                threshold,
            } => {
                current_filter = super::effect_shaders::build_glow(
                    shader_cache,
                    resolution,
                    time,
                    current_filter,
                    intensity.current_value,
                    threshold.current_value,
                );
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
            EffectType::Fisheye(strength) => {
                current_filter = super::effect_shaders::build_fisheye(
                    shader_cache, resolution, time, current_filter,
                    strength.current_value,
                );
            }
            EffectType::Bulge { strength, radius } => {
                current_filter = super::effect_shaders::build_bulge(
                    shader_cache, resolution, time, current_filter,
                    strength.current_value, radius.current_value,
                );
            }
            EffectType::Barrel(curvature) => {
                current_filter = super::effect_shaders::build_barrel(
                    shader_cache, resolution, time, current_filter,
                    curvature.current_value,
                );
            }
            EffectType::LensFlare { intensity, color } => {
                current_filter = super::effect_shaders::build_lens_flare(
                    shader_cache, resolution, time, current_filter,
                    intensity.current_value, *color,
                );
            }
            EffectType::Noir(contrast) => {
                current_filter = super::effect_shaders::build_noir(
                    shader_cache, resolution, time, current_filter,
                    contrast.current_value,
                );
            }
            EffectType::Cyberpunk(intensity) => {
                current_filter = super::effect_shaders::build_cyberpunk(
                    shader_cache, resolution, time, current_filter,
                    intensity.current_value,
                );
            }
            EffectType::Vintage(amount) => {
                current_filter = super::effect_shaders::build_vintage(
                    shader_cache, resolution, time, current_filter,
                    amount.current_value,
                );
            }
            EffectType::Gameboy => {
                current_filter = super::effect_shaders::build_gameboy(
                    shader_cache, resolution, time, current_filter,
                );
            }
            EffectType::BleachBypass(amount) => {
                current_filter = super::effect_shaders::build_bleach_bypass(
                    shader_cache, resolution, time, current_filter,
                    amount.current_value,
                );
            }
            EffectType::TealOrange(amount) => {
                current_filter = super::effect_shaders::build_teal_orange(
                    shader_cache, resolution, time, current_filter,
                    amount.current_value,
                );
            }
            EffectType::Plasma { scale, speed } => {
                current_filter = super::effect_shaders::build_plasma(
                    shader_cache, resolution, time, current_filter,
                    scale.current_value, speed.current_value,
                );
            }
            EffectType::Hologram(intensity) => {
                current_filter = super::effect_shaders::build_hologram(
                    shader_cache, resolution, time, current_filter,
                    intensity.current_value,
                );
            }
            EffectType::Emboss { angle, strength } => {
                current_filter = super::effect_shaders::build_emboss(
                    shader_cache, resolution, time, current_filter,
                    angle.current_value, strength.current_value,
                );
            }
            EffectType::Toon { levels, edge } => {
                current_filter = super::effect_shaders::build_toon(
                    shader_cache, resolution, time, current_filter,
                    levels.current_value, edge.current_value,
                );
            }
            EffectType::OilPaint { block_size, levels } => {
                current_filter = super::effect_shaders::build_oil_paint(
                    shader_cache, resolution, time, current_filter,
                    block_size.current_value, levels.current_value,
                );
            }
            EffectType::LightLeak { intensity, color } => {
                current_filter = super::effect_shaders::build_light_leak(
                    shader_cache, resolution, time, current_filter,
                    intensity.current_value, *color,
                );
            }
            EffectType::Aurora { intensity, speed } => {
                current_filter = super::effect_shaders::build_aurora(
                    shader_cache, resolution, time, current_filter,
                    intensity.current_value, speed.current_value,
                );
            }
            EffectType::SunRays { intensity, angle, speed } => {
                current_filter = super::effect_shaders::build_sunrays(
                    shader_cache, resolution, time, current_filter,
                    intensity.current_value, angle.current_value, speed.current_value,
                );
            }
            EffectType::SkinSmooth { intensity, radius } => {
                current_filter = super::effect_shaders::build_skin_smooth(
                    shader_cache, resolution, time, current_filter,
                    intensity.current_value, radius.current_value,
                );
            }
            EffectType::BeautyGlow { intensity, threshold } => {
                current_filter = super::effect_shaders::build_beauty_glow(
                    shader_cache, resolution, time, current_filter,
                    intensity.current_value, threshold.current_value,
                );
            }
            EffectType::SoftFocus { amount } => {
                current_filter = super::effect_shaders::build_soft_focus(
                    shader_cache, resolution, time, current_filter,
                    amount.current_value,
                );
            }
            EffectType::Vibrance { amount } => {
                current_filter = super::effect_shaders::build_vibrance(
                    shader_cache, resolution, time, current_filter,
                    amount.current_value,
                );
            }
            EffectType::Fog { intensity, color } => {
                current_filter = super::effect_shaders::build_fog(
                    shader_cache, resolution, time, current_filter,
                    intensity.current_value, *color,
                );
            }
            EffectType::Mist { intensity, speed } => {
                current_filter = super::effect_shaders::build_mist(
                    shader_cache, resolution, time, current_filter,
                    intensity.current_value, speed.current_value,
                );
            }
            EffectType::Rain { intensity, speed } => {
                current_filter = super::effect_shaders::build_rain(
                    shader_cache, resolution, time, current_filter,
                    intensity.current_value, speed.current_value,
                );
            }
            EffectType::Snow { intensity, speed, size } => {
                current_filter = super::effect_shaders::build_snow(
                    shader_cache, resolution, time, current_filter,
                    intensity.current_value, speed.current_value, size.current_value,
                );
            }
            EffectType::Dust { intensity, speed } => {
                current_filter = super::effect_shaders::build_dust(
                    shader_cache, resolution, time, current_filter,
                    intensity.current_value, speed.current_value,
                );
            }
            EffectType::BokehDots { intensity, size, color } => {
                current_filter = super::effect_shaders::build_bokeh_dots(
                    shader_cache, resolution, time, current_filter,
                    intensity.current_value, size.current_value, *color,
                );
            }
            EffectType::FilmLook { amount, grain } => {
                current_filter = super::effect_shaders::build_film_look(
                    shader_cache, resolution, time, current_filter,
                    amount.current_value, grain.current_value,
                );
            }
            EffectType::OrangeTeal { amount } => {
                current_filter = super::effect_shaders::build_orange_teal(
                    shader_cache, resolution, time, current_filter,
                    amount.current_value,
                );
            }
            EffectType::CrossProcess { amount } => {
                current_filter = super::effect_shaders::build_cross_process(
                    shader_cache, resolution, time, current_filter,
                    amount.current_value,
                );
            }
            EffectType::Polaroid { amount } => {
                current_filter = super::effect_shaders::build_polaroid(
                    shader_cache, resolution, time, current_filter,
                    amount.current_value,
                );
            }
            EffectType::FadedFilm { amount } => {
                current_filter = super::effect_shaders::build_faded_film(
                    shader_cache, resolution, time, current_filter,
                    amount.current_value,
                );
            }
            EffectType::Midnight { amount } => {
                current_filter = super::effect_shaders::build_midnight(
                    shader_cache, resolution, time, current_filter,
                    amount.current_value,
                );
            }
            EffectType::Watercolor { intensity, edge } => {
                current_filter = super::effect_shaders::build_watercolor(
                    shader_cache, resolution, time, current_filter,
                    intensity.current_value, edge.current_value,
                );
            }
            EffectType::Crosshatch { intensity, density } => {
                current_filter = super::effect_shaders::build_crosshatch(
                    shader_cache, resolution, time, current_filter,
                    intensity.current_value, density.current_value,
                );
            }
            EffectType::Pointillism { dot_size, intensity } => {
                current_filter = super::effect_shaders::build_pointillism(
                    shader_cache, resolution, time, current_filter,
                    dot_size.current_value, intensity.current_value,
                );
            }
            EffectType::Mosaic { tile_size } => {
                current_filter = super::effect_shaders::build_mosaic(
                    shader_cache, resolution, time, current_filter,
                    tile_size.current_value,
                );
            }
            EffectType::Ink { threshold, intensity } => {
                current_filter = super::effect_shaders::build_ink(
                    shader_cache, resolution, time, current_filter,
                    threshold.current_value, intensity.current_value,
                );
            }
            EffectType::PencilColor { intensity } => {
                current_filter = super::effect_shaders::build_pencil_color(
                    shader_cache, resolution, time, current_filter,
                    intensity.current_value,
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

    fn kind(&self) -> &'static str {
        "effect"
    }

    fn layout_style(&self) -> Style {
        self.style.clone()
    }

    fn set_layout_style(&mut self, style: Style) {
        self.style = style;
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
    ) -> Result<(), String> {
        let ease_fn = crate::scripting::utils::parse_easing_result(easing)?;
        let mut found = false;
        for effect in &mut self.effects {
            if let EffectType::RuntimeShader { uniforms, .. } = effect {
                if let Some(anim) = uniforms.get_mut(property) {
                    if let ShaderUniform::Float(a) = anim {
                        a.add_segment(start, target, duration, ease_fn.clone());
                        found = true;
                    }
                }
            }
        }
        if found {
            Ok(())
        } else {
            Err(format!(
                "unknown property '{property}' on effect; expected a float shader uniform name"
            ))
        }
    }

    fn animate_property_spring(
        &mut self,
        property: &str,
        start: Option<f32>,
        target: f32,
        config: crate::animation::SpringConfig,
    ) -> Result<(), String> {
        let mut found = false;
        for effect in &mut self.effects {
            if let EffectType::RuntimeShader { uniforms, .. } = effect {
                if let Some(anim) = uniforms.get_mut(property) {
                    if let ShaderUniform::Float(a) = anim {
                        if let Some(s) = start {
                            a.add_spring_with_start(s, target, config);
                        } else {
                            a.add_spring(target, config);
                        }
                        found = true;
                    }
                }
            }
        }
        if found {
            Ok(())
        } else {
            Err(format!(
                "spring: unknown property '{property}' on effect; expected a float shader uniform"
            ))
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

    /// Batch 2 shaders (lens, grade presets, stylized, atmosphere) must compile.
    #[test]
    fn batch2_shader_effects_compile() {
        let cache = cache();
        let sc = Some(&cache);
        let res = (64.0_f32, 64.0_f32);
        let t = 0.0_f32;
        let cases: Vec<EffectType> = vec![
            EffectType::Fisheye(Animated::new(0.5)),
            EffectType::Bulge {
                strength: Animated::new(0.5),
                radius: Animated::new(200.0),
            },
            EffectType::Barrel(Animated::new(0.3)),
            EffectType::LensFlare {
                intensity: Animated::new(0.6),
                color: Color::new(1.0, 0.9, 0.7, 1.0),
            },
            EffectType::Noir(Animated::new(1.4)),
            EffectType::Cyberpunk(Animated::new(0.6)),
            EffectType::Vintage(Animated::new(0.6)),
            EffectType::Gameboy,
            EffectType::BleachBypass(Animated::new(0.7)),
            EffectType::TealOrange(Animated::new(0.6)),
            EffectType::Plasma {
                scale: Animated::new(10.0),
                speed: Animated::new(1.5),
            },
            EffectType::Hologram(Animated::new(0.7)),
            EffectType::Emboss {
                angle: Animated::new(45.0),
                strength: Animated::new(2.0),
            },
            EffectType::Toon {
                levels: Animated::new(6.0),
                edge: Animated::new(0.08),
            },
            EffectType::OilPaint {
                block_size: Animated::new(8.0),
                levels: Animated::new(6.0),
            },
            EffectType::LightLeak {
                intensity: Animated::new(0.5),
                color: Color::new(1.0, 0.6, 0.3, 1.0),
            },
            EffectType::Aurora {
                intensity: Animated::new(0.5),
                speed: Animated::new(1.0),
            },
            EffectType::SunRays {
                intensity: Animated::new(0.5),
                angle: Animated::new(0.0),
                speed: Animated::new(1.0),
            },
        ];
        for (i, eff) in cases.iter().enumerate() {
            let filter = build_effect_filter(std::slice::from_ref(eff), sc, res, t);
            assert!(
                filter.is_some(),
                "batch2 shader #{i} ({:?}) did not produce an ImageFilter (SkSL compile failed)",
                eff
            );
        }
    }

    /// Batch 3 shaders (beauty, weather, cinematic grades, artistic stylize)
    /// must compile.
    #[test]
    fn batch3_shader_effects_compile() {
        let cache = cache();
        let sc = Some(&cache);
        let res = (64.0_f32, 64.0_f32);
        let t = 0.0_f32;
        let cases: Vec<EffectType> = vec![
            EffectType::SkinSmooth {
                intensity: Animated::new(0.6),
                radius: Animated::new(3.0),
            },
            EffectType::BeautyGlow {
                intensity: Animated::new(0.5),
                threshold: Animated::new(0.6),
            },
            EffectType::SoftFocus { amount: Animated::new(0.5) },
            EffectType::Vibrance { amount: Animated::new(0.5) },
            EffectType::Fog {
                intensity: Animated::new(0.5),
                color: Color::new(0.8, 0.85, 0.9, 1.0),
            },
            EffectType::Mist {
                intensity: Animated::new(0.5),
                speed: Animated::new(1.0),
            },
            EffectType::Rain {
                intensity: Animated::new(0.6),
                speed: Animated::new(1.0),
            },
            EffectType::Snow {
                intensity: Animated::new(0.6),
                speed: Animated::new(1.0),
                size: Animated::new(2.0),
            },
            EffectType::Dust {
                intensity: Animated::new(0.5),
                speed: Animated::new(1.0),
            },
            EffectType::BokehDots {
                intensity: Animated::new(0.5),
                size: Animated::new(12.0),
                color: Color::new(1.0, 0.9, 0.7, 1.0),
            },
            EffectType::FilmLook {
                amount: Animated::new(0.6),
                grain: Animated::new(1.0),
            },
            EffectType::OrangeTeal { amount: Animated::new(0.6) },
            EffectType::CrossProcess { amount: Animated::new(0.7) },
            EffectType::Polaroid { amount: Animated::new(0.7) },
            EffectType::FadedFilm { amount: Animated::new(0.6) },
            EffectType::Midnight { amount: Animated::new(0.6) },
            EffectType::Watercolor {
                intensity: Animated::new(0.7),
                edge: Animated::new(1.0),
            },
            EffectType::Crosshatch {
                intensity: Animated::new(0.8),
                density: Animated::new(8.0),
            },
            EffectType::Pointillism {
                dot_size: Animated::new(6.0),
                intensity: Animated::new(0.9),
            },
            EffectType::Mosaic { tile_size: Animated::new(20.0) },
            EffectType::Ink {
                threshold: Animated::new(0.1),
                intensity: Animated::new(0.9),
            },
            EffectType::PencilColor { intensity: Animated::new(0.8) },
        ];
        for (i, eff) in cases.iter().enumerate() {
            let filter = build_effect_filter(std::slice::from_ref(eff), sc, res, t);
            assert!(
                filter.is_some(),
                "batch3 shader #{i} ({:?}) did not produce an ImageFilter (SkSL compile failed)",
                eff
            );
        }
    }
}
