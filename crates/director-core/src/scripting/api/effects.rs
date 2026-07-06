//! # Effects API
//!
//! Visual effects functions for Rhai scripts.
//!
//! ## Responsibilities
//! - **Color Effects** (named): grayscale, sepia, invert
//! - **Color Effects** (float): contrast, brightness, saturation, exposure, hue_rotate
//! - **Color Effects** (map): tint with `#{ color, amount }`
//! - **Blur Effect**: `apply_effect("blur", value)` for Gaussian blur
//! - **Custom Shaders**: `apply_effect("shader", {...})` for SkSL shaders
//! - **Cinematic Shaders**:
//!   - `apply_effect("directional_blur", #{ strength: 10.0, angle: 45.0 })` for motion blur
//!   - `apply_effect("grain", #{ intensity: 0.1, size: 2.0 })` for film grain
//!   - `apply_effect("vignette", #{ intensity: 0.5, radius: 0.3, feather: 0.2 })` for edge darkening
//!   - `apply_effect("chromatic_aberration", #{ strength: 2.0 })` for RGB channel offset
//!   - `apply_effect("pixelate", #{ block_size: 8.0 })` for blocky pixelation
//!   - `apply_effect("sharpen", #{ strength: 0.5 })` for unsharp mask
//!   - `apply_effect("glow", #{ intensity: 0.5, threshold: 0.6 })` for bright-area glow
//! - **Extended Shader Library** (SkSL runtime shaders unless noted):
//!   - Distortion: `warp`, `swirl`, `ripple`, `glass`, `shake`
//!   - Color/Grade: `duotone`, `posterize`, `threshold`, `temperature` (color matrix)
//!   - Stylize: `scanlines`, `glitch`, `edge_detect`
//!   - Light/Atmosphere: `zoom_blur`, `kaleidoscope`
//! - **Safety Guards**: stale-handle checks with script-level errors

use crate::animation::{Animated, TweenableVector};
use crate::node::{EffectNode, EffectType, ShaderUniform};
use rhai::{Engine, Map};
use std::collections::HashMap;

use super::super::types::NodeHandle;
use super::super::utils::apply_effect_to_node;

/// Append an effect to an existing EffectNode, or wrap the target in a new EffectNode.
fn push_or_wrap_effect(
    d: &mut crate::director::Director,
    node_id: crate::types::NodeId,
    effect: EffectType,
) -> crate::types::NodeId {
    if let Some(node) = d.scene.get_node_mut(node_id) {
        if let Some(effect_node) = node.element.as_any_mut().downcast_mut::<EffectNode>() {
            effect_node.effects.push(effect);
            return node_id;
        }
    }
    apply_effect_to_node(d, node_id, effect)
}

/// Register effect-related Rhai functions.
pub fn register(engine: &mut Engine) {
    // Named effects (no value)
    engine.register_fn(
        "apply_effect",
        |node: &mut NodeHandle, name: &str| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = node.lock_director()?;
            node.ensure_alive(&d)?;
            let effect = match name {
                "grayscale" => Some(EffectType::ColorMatrix(vec![
                    0.2126, 0.7152, 0.0722, 0.0, 0.0, 0.2126, 0.7152, 0.0722, 0.0, 0.0, 0.2126,
                    0.7152, 0.0722, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
                ])),
                "sepia" => Some(EffectType::ColorMatrix(vec![
                    0.393, 0.769, 0.189, 0.0, 0.0, 0.349, 0.686, 0.168, 0.0, 0.0, 0.272, 0.534,
                    0.131, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
                ])),
                "invert" => Some(EffectType::ColorMatrix(vec![
                    -1.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 1.0,
                    0.0, 0.0, 0.0, 1.0, 0.0,
                ])),
                _ => None,
            };

            if let Some(eff) = effect {
                let id = push_or_wrap_effect(&mut d, node.id, eff);
                Ok(NodeHandle {
                    director: node.director.clone(),
                    id,
                })
            } else {
                Ok(NodeHandle {
                    director: node.director.clone(),
                    id: node.id,
                })
            }
        },
    );

    // Effects with a value parameter
    engine.register_fn(
        "apply_effect",
        |node: &mut NodeHandle,
         name: &str,
         val: f64|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = node.lock_director()?;
            node.ensure_alive(&d)?;
            let val = val as f32;
            let effect = match name {
                "contrast" => {
                    let t = (1.0 - val) / 2.0;
                    Some(EffectType::ColorMatrix(vec![
                        val, 0.0, 0.0, 0.0, t, 0.0, val, 0.0, 0.0, t, 0.0, 0.0, val, 0.0, t, 0.0,
                        0.0, 0.0, 1.0, 0.0,
                    ]))
                }
                "brightness" => Some(EffectType::ColorMatrix(vec![
                    1.0, 0.0, 0.0, 0.0, val, 0.0, 1.0, 0.0, 0.0, val, 0.0, 0.0, 1.0, 0.0, val, 0.0,
                    0.0, 0.0, 1.0, 0.0,
                ])),
                "saturation" => {
                    let s = val;
                    Some(EffectType::ColorMatrix(vec![
                        0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s, 0.0, 0.0,
                        0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s, 0.0, 0.0,
                        0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s, 0.0, 0.0,
                        0.0, 0.0, 0.0, 1.0, 0.0,
                    ]))
                }
                "exposure" => {
                    let m = (2.0_f32).powf(val);
                    Some(EffectType::ColorMatrix(vec![
                        m, 0.0, 0.0, 0.0, 0.0,
                        0.0, m, 0.0, 0.0, 0.0,
                        0.0, 0.0, m, 0.0, 0.0,
                        0.0, 0.0, 0.0, 1.0, 0.0,
                    ]))
                }
                "hue_rotate" | "hue" => {
                    let angle_rad = val * std::f32::consts::PI / 180.0;
                    let cos_a = angle_rad.cos();
                    let sin_a = angle_rad.sin();
                    let inv_cos = 1.0 - cos_a;
                    let isqrt3 = 0.577350269;
                    Some(EffectType::ColorMatrix(vec![
                        cos_a + inv_cos / 3.0,              inv_cos / 3.0 - sin_a * isqrt3,  inv_cos / 3.0 + sin_a * isqrt3,  0.0, 0.0,
                        inv_cos / 3.0 + sin_a * isqrt3,     cos_a + inv_cos / 3.0,            inv_cos / 3.0 - sin_a * isqrt3,  0.0, 0.0,
                        inv_cos / 3.0 - sin_a * isqrt3,     inv_cos / 3.0 + sin_a * isqrt3,  cos_a + inv_cos / 3.0,            0.0, 0.0,
                        0.0,                                  0.0,                              0.0,                              1.0, 0.0,
                    ]))
                }
                "blur" => Some(EffectType::Blur(Animated::new(val))),
                // Temperature: cool/warm color shift (warmth: -1 cool .. +1 warm)
                "temperature" | "warmth" => {
                    let w = val;
                    let r_gain = 1.0 + w * 0.3;
                    let b_gain = 1.0 - w * 0.3;
                    Some(EffectType::ColorMatrix(vec![
                        r_gain, 0.0, 0.0, 0.0, 0.0,
                        0.0, 1.0, 0.0, 0.0, 0.0,
                        0.0, 0.0, b_gain, 0.0, 0.0,
                        0.0, 0.0, 0.0, 1.0, 0.0,
                    ]))
                }
                _ => None,
            };

            if let Some(eff) = effect {
                let id = push_or_wrap_effect(&mut d, node.id, eff);
                Ok(NodeHandle {
                    director: node.director.clone(),
                    id,
                })
            } else {
                Ok(NodeHandle {
                    director: node.director.clone(),
                    id: node.id,
                })
            }
        },
    );

    // Custom shader effects and cinematic effects (directional_blur, grain)
    engine.register_fn(
        "apply_effect",
        |node: &mut NodeHandle,
         name: &str,
         map: rhai::Map|
         -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            let mut d = node.lock_director()?;
            node.ensure_alive(&d)?;

            let effect = match name {
                "shader" => {
                    if let Some(code) = map.get("code").and_then(|v| v.clone().into_string().ok()) {
                        let mut uniforms = HashMap::new();
                        if let Some(u_map) = map
                            .get("uniforms")
                            .and_then(|v| v.clone().try_cast::<Map>())
                        {
                            for (k, v) in u_map {
                                if let Ok(f) = v.as_float() {
                                    uniforms.insert(
                                        k.to_string(),
                                        ShaderUniform::Float(Animated::new(f as f32)),
                                    );
                                } else if let Ok(arr) = v.clone().into_array() {
                                    // Handle array -> Vec<f32>
                                    let mut vec_data = Vec::new();
                                    for item in arr {
                                        if let Ok(f) = item.as_float() {
                                            vec_data.push(f as f32);
                                        }
                                    }
                                    if !vec_data.is_empty() {
                                        uniforms.insert(
                                            k.to_string(),
                                            ShaderUniform::Vec(Animated::new(TweenableVector(
                                                vec_data,
                                            ))),
                                        );
                                    }
                                }
                            }
                        }
                        Some(EffectType::RuntimeShader {
                            sksl: code,
                            uniforms,
                        })
                    } else {
                        None
                    }
                }
                // Directional blur (motion blur)
                // Usage: node.apply_effect("directional_blur", #{ strength: 10.0, angle: 45.0, samples: 16 })
                "directional_blur" | "motion_blur" => {
                    let strength = map
                        .get("strength")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(10.0) as f32;
                    let angle = map
                        .get("angle")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(0.0) as f32;
                    let samples = map
                        .get("samples")
                        .and_then(|v| v.as_int().ok())
                        .unwrap_or(16) as u32;
                    Some(EffectType::DirectionalBlur {
                        strength: Animated::new(strength),
                        angle: Animated::new(angle),
                        samples,
                    })
                }
                // Film grain effect
                // Usage: node.apply_effect("grain", #{ intensity: 0.1, size: 2.0 })
                "grain" | "film_grain" | "noise" => {
                    let intensity = map
                        .get("intensity")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(0.1) as f32;
                    let size = map
                        .get("size")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(2.0) as f32;
                    Some(EffectType::FilmGrain {
                        intensity: Animated::new(intensity),
                        size: Animated::new(size),
                    })
                }
                // Tint: blend image toward a solid color
                // Usage: node.apply_effect("tint", #{ color: "#ff6b6b", amount: 0.5 })
                "tint" => {
                    let color = map
                        .get("color")
                        .and_then(|v| v.clone().into_string().ok())
                        .and_then(|s| super::super::utils::parse_hex_color(&s))
                        .unwrap_or(crate::types::Color::new(0.0, 0.0, 0.0, 0.0));
                    let amount = map
                        .get("amount")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(0.5) as f32;
                    let t = 1.0 - amount;
                    Some(EffectType::ColorMatrix(vec![
                        t, 0.0, 0.0, 0.0, color.r * amount,
                        0.0, t, 0.0, 0.0, color.g * amount,
                        0.0, 0.0, t, 0.0, color.b * amount,
                        0.0, 0.0, 0.0, 1.0, 0.0,
                    ]))
                }
                // Vignette: darken edges of frame
                // Usage: node.apply_effect("vignette", #{ intensity: 0.5, radius: 0.3, feather: 0.2 })
                "vignette" => {
                    let intensity = map
                        .get("intensity")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(0.5) as f32;
                    let radius = map
                        .get("radius")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(0.3) as f32;
                    let feather = map
                        .get("feather")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(0.2) as f32;
                    Some(EffectType::Vignette {
                        intensity: Animated::new(intensity),
                        radius: Animated::new(radius),
                        feather: Animated::new(feather),
                    })
                }
                // Chromatic Aberration: RGB channel offset
                // Usage: node.apply_effect("chromatic_aberration", #{ strength: 3.0 })
                "chromatic_aberration" | "chromatic" | "ca" => {
                    let strength = map
                        .get("strength")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(2.0) as f32;
                    Some(EffectType::ChromaticAberration(Animated::new(strength)))
                }
                // Pixelate: blocky pixelation grid
                // Usage: node.apply_effect("pixelate", #{ block_size: 10.0 })
                "pixelate" => {
                    let block_size = map
                        .get("block_size")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(8.0) as f32;
                    Some(EffectType::Pixelate(Animated::new(block_size)))
                }
                // Sharpen: unsharp mask via 3x3 Laplacian
                // Usage: node.apply_effect("sharpen", #{ strength: 0.5 })
                "sharpen" => {
                    let strength = map
                        .get("strength")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(0.5) as f32;
                    Some(EffectType::Sharpen(Animated::new(strength)))
                }
                // Glow: additive bright-area glow with blur
                // Usage: node.apply_effect("glow", #{ intensity: 0.5, threshold: 0.6 })
                "glow" | "bloom" => {
                    let intensity = map
                        .get("intensity")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(0.5) as f32;
                    let threshold = map
                        .get("threshold")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(0.6) as f32;
                    Some(EffectType::Glow {
                        intensity: Animated::new(intensity),
                        threshold: Animated::new(threshold),
                    })
                }
                // Warp: sine-wave displacement distortion
                "wave_distort" | "warp" => {
                    let amplitude = map.get("amplitude").and_then(|v| v.as_float().ok()).unwrap_or(10.0) as f32;
                    let frequency = map.get("frequency").and_then(|v| v.as_float().ok()).unwrap_or(0.02) as f32;
                    let direction = map.get("direction").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
                    let speed = map.get("speed").and_then(|v| v.as_float().ok()).unwrap_or(2.0) as f32;
                    Some(EffectType::Warp {
                        amplitude: Animated::new(amplitude),
                        frequency: Animated::new(frequency),
                        direction: Animated::new(direction),
                        speed: Animated::new(speed),
                    })
                }
                // Swirl: rotational twist around the center
                "swirl" => {
                    let radius = map.get("radius").and_then(|v| v.as_float().ok()).unwrap_or(150.0) as f32;
                    let angle = map.get("angle").and_then(|v| v.as_float().ok()).unwrap_or(180.0) as f32;
                    Some(EffectType::Swirl {
                        radius: Animated::new(radius),
                        angle: Animated::new(angle),
                    })
                }
                // Ripple: concentric waves from the center
                "ripple" => {
                    let amplitude = map.get("amplitude").and_then(|v| v.as_float().ok()).unwrap_or(5.0) as f32;
                    let frequency = map.get("frequency").and_then(|v| v.as_float().ok()).unwrap_or(0.05) as f32;
                    let speed = map.get("speed").and_then(|v| v.as_float().ok()).unwrap_or(3.0) as f32;
                    Some(EffectType::Ripple {
                        amplitude: Animated::new(amplitude),
                        frequency: Animated::new(frequency),
                        speed: Animated::new(speed),
                    })
                }
                // Glass: frosted-glass refraction and blur
                "glass" | "frosted_glass" => {
                    let scale = map.get("scale").and_then(|v| v.as_float().ok()).unwrap_or(0.01) as f32;
                    let intensity = map.get("intensity").and_then(|v| v.as_float().ok()).unwrap_or(5.0) as f32;
                    Some(EffectType::Glass {
                        scale: Animated::new(scale),
                        intensity: Animated::new(intensity),
                    })
                }
                // Shake: block-based jitter displacement
                "shake" | "jitter" => {
                    let intensity = map.get("intensity").and_then(|v| v.as_float().ok()).unwrap_or(8.0) as f32;
                    let speed = map.get("speed").and_then(|v| v.as_float().ok()).unwrap_or(15.0) as f32;
                    Some(EffectType::Shake {
                        intensity: Animated::new(intensity),
                        speed: Animated::new(speed),
                    })
                }
                // Duotone: luminance-mapped two-color grade
                "duotone" => {
                    let shadow = map
                        .get("shadow")
                        .and_then(|v| v.clone().into_string().ok())
                        .and_then(|s| crate::scripting::utils::parse_hex_color(&s))
                        .unwrap_or(crate::types::Color::new(0.05, 0.05, 0.2, 1.0));
                    let highlight = map
                        .get("highlight")
                        .and_then(|v| v.clone().into_string().ok())
                        .and_then(|s| crate::scripting::utils::parse_hex_color(&s))
                        .unwrap_or(crate::types::Color::new(1.0, 0.8, 0.4, 1.0));
                    Some(EffectType::Duotone { shadow, highlight })
                }
                // Posterize: reduce color levels
                "posterize" => {
                    let levels = map.get("levels").and_then(|v| v.as_float().ok()).unwrap_or(4.0) as f32;
                    Some(EffectType::Posterize(Animated::new(levels)))
                }
                // Threshold: black/white luminance cutoff
                "threshold" => {
                    let level = map.get("level").and_then(|v| v.as_float().ok()).unwrap_or(0.5) as f32;
                    Some(EffectType::Threshold(Animated::new(level)))
                }
                // Scanlines: CRT-style scanline overlay
                "scanlines" | "crt" => {
                    let intensity = map.get("intensity").and_then(|v| v.as_float().ok()).unwrap_or(0.4) as f32;
                    let count = map.get("count").and_then(|v| v.as_float().ok()).unwrap_or(300.0) as f32;
                    Some(EffectType::Scanlines {
                        intensity: Animated::new(intensity),
                        count: Animated::new(count),
                    })
                }
                // Glitch: RGB split with block displacement
                "glitch" => {
                    let intensity = map.get("intensity").and_then(|v| v.as_float().ok()).unwrap_or(1.0) as f32;
                    let speed = map.get("speed").and_then(|v| v.as_float().ok()).unwrap_or(10.0) as f32;
                    Some(EffectType::Glitch {
                        intensity: Animated::new(intensity),
                        speed: Animated::new(speed),
                    })
                }
                // Edge detect: Sobel outline
                "edge_detect" | "edges" => {
                    let intensity = map.get("intensity").and_then(|v| v.as_float().ok()).unwrap_or(1.0) as f32;
                    Some(EffectType::EdgeDetect(Animated::new(intensity)))
                }
                // Zoom blur: radial motion blur from the center
                "zoom_blur" => {
                    let strength = map.get("strength").and_then(|v| v.as_float().ok()).unwrap_or(0.3) as f32;
                    Some(EffectType::ZoomBlur(Animated::new(strength)))
                }
                // Kaleidoscope: mirror symmetry segments
                "kaleidoscope" | "kaleido" => {
                    let segments = map.get("segments").and_then(|v| v.as_float().ok()).unwrap_or(6.0) as f32;
                    Some(EffectType::Kaleidoscope(Animated::new(segments)))
                }
                // Thermal: heat map color spectrum
                "thermal" | "heatmap" => {
                    Some(EffectType::Thermal)
                }
                // Liquid: bidirectional sine/cosine coordinate warp
                "liquid" => {
                    let frequency = map.get("frequency").and_then(|v| v.as_float().ok()).unwrap_or(12.0) as f32;
                    let amplitude = map.get("amplitude").and_then(|v| v.as_float().ok()).unwrap_or(0.015) as f32;
                    Some(EffectType::Liquid {
                        frequency: Animated::new(frequency),
                        amplitude: Animated::new(amplitude),
                    })
                }
                // Acid: fluorescent acid melt
                "acid" => {
                    let color = map
                        .get("color")
                        .and_then(|v| v.clone().into_string().ok())
                        .and_then(|s| crate::scripting::utils::parse_hex_color(&s))
                        .unwrap_or(crate::types::Color::new(0.063, 0.725, 0.506, 1.0));
                    let speed = map.get("speed").and_then(|v| v.as_float().ok()).unwrap_or(1.8) as f32;
                    Some(EffectType::Acid {
                        color,
                        speed: Animated::new(speed),
                    })
                }
                // Halftone: pop-art dot grid
                "halftone" => {
                    let dot_size = map.get("dot_size").and_then(|v| v.as_float().ok()).unwrap_or(55.0) as f32;
                    Some(EffectType::Halftone(Animated::new(dot_size)))
                }
                // Ascii: terminal ASCII art
                "ascii" => {
                    let scale = map.get("scale").and_then(|v| v.as_float().ok()).unwrap_or(90.0) as f32;
                    let color = map
                        .get("color")
                        .and_then(|v| v.clone().into_string().ok())
                        .and_then(|s| crate::scripting::utils::parse_hex_color(&s))
                        .unwrap_or(crate::types::Color::new(0.0, 1.0, 0.255, 1.0));
                    Some(EffectType::Ascii {
                        scale: Animated::new(scale),
                        color,
                    })
                }
                // Sketch: charcoal pencil sketch
                "sketch" | "pencil" => {
                    let contrast = map.get("contrast").and_then(|v| v.as_float().ok()).unwrap_or(1.2) as f32;
                    Some(EffectType::Sketch(Animated::new(contrast)))
                }
                // Mirror: quad-mirror symmetry
                "mirror" | "quad_mirror" => {
                    Some(EffectType::Mirror)
                }
                // TriTone: three-color luminance grade with midpoint
                "tritone" | "tri_tone" => {
                    let shadow = map
                        .get("shadow")
                        .or_else(|| map.get("low"))
                        .and_then(|v| v.clone().into_string().ok())
                        .and_then(|s| crate::scripting::utils::parse_hex_color(&s))
                        .unwrap_or(crate::types::Color::new(0.031, 0.031, 0.039, 1.0));
                    let mid = map
                        .get("mid")
                        .and_then(|v| v.clone().into_string().ok())
                        .and_then(|s| crate::scripting::utils::parse_hex_color(&s))
                        .unwrap_or(crate::types::Color::new(0.231, 0.510, 0.965, 1.0));
                    let highlight = map
                        .get("highlight")
                        .or_else(|| map.get("high"))
                        .and_then(|v| v.clone().into_string().ok())
                        .and_then(|s| crate::scripting::utils::parse_hex_color(&s))
                        .unwrap_or(crate::types::Color::new(0.376, 0.647, 0.980, 1.0));
                    let midpoint = map.get("midpoint").and_then(|v| v.as_float().ok()).unwrap_or(0.45) as f32;
                    Some(EffectType::TriTone {
                        shadow,
                        mid,
                        highlight,
                        midpoint: Animated::new(midpoint),
                    })
                }
                // VHS: combined CRT glitch (jitter + scanlines + noise + RGB shift)
                "vhs" | "crt_glitch" => {
                    let distortion = map.get("distortion").and_then(|v| v.as_float().ok()).unwrap_or(0.35) as f32;
                    let scanline = map.get("scanline").and_then(|v| v.as_float().ok()).unwrap_or(0.5) as f32;
                    Some(EffectType::Vhs {
                        distortion: Animated::new(distortion),
                        scanline: Animated::new(scanline),
                    })
                }
                // NeonEdge: edge detect with glow color and darkened background
                "neon_edge" | "neon_edge_detect" => {
                    let glow_color = map
                        .get("glow_color")
                        .or_else(|| map.get("color"))
                        .and_then(|v| v.clone().into_string().ok())
                        .and_then(|s| crate::scripting::utils::parse_hex_color(&s))
                        .unwrap_or(crate::types::Color::new(0.0, 1.0, 0.8, 1.0));
                    let threshold = map.get("threshold").and_then(|v| v.as_float().ok()).unwrap_or(0.12) as f32;
                    Some(EffectType::NeonEdge {
                        glow_color,
                        threshold: Animated::new(threshold),
                    })
                }
                _ => None,
            };

            if let Some(eff) = effect {
                let id = push_or_wrap_effect(&mut d, node.id, eff);
                Ok(NodeHandle {
                    director: node.director.clone(),
                    id,
                })
            } else {
                Ok(NodeHandle {
                    director: node.director.clone(),
                    id: node.id,
                })
            }
        },
    );
}
