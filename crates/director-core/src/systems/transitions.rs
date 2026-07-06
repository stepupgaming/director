//! # Transitions System
//!
//! Handles visual transitions between scenes using SkSL shaders.
//!
//! ## Responsibilities
//! - **Transition Types**: Enum of supported transition effects.
//! - **Shader Definitions**: GLSL/SkSL code for each transition.
//! - **Shader Execution**: Composites two scene images with transition effect.
//!
//! ## Key Types
//! - `TransitionType`: Fade, Slide, Wipe, CircleOpen, Wave, Glitch, Iris, Spiral variants.
//! - `Transition`: Defines a transition between two timeline scenes.

use crate::animation::EasingType;
use skia_safe::{runtime_effect::ChildPtr, Data, RuntimeEffect};
use tracing::error;

/// The type of visual transition between scenes.
#[derive(Clone, Debug)]
pub enum TransitionType {
    Fade,
    SlideLeft,
    SlideRight,
    WipeLeft,
    WipeRight,
    CircleOpen,
    /// Wave distortion transition with configurable amplitude and frequency.
    Wave {
        amplitude: f32,
        frequency: f32,
    },
    /// Glitch/scanline distortion effect with configurable intensity.
    Glitch {
        intensity: f32,
    },
    /// Iris/circular wipe transition with configurable start and end radius.
    Iris {
        start_radius: f32,
        end_radius: f32,
    },
    /// Spiral transition with configurable number of rotations.
    Spiral {
        rotations: f32,
    },
    /// Zoom In: image B scales up from center while A fades out.
    ZoomIn,
    /// Radial Wipe: A is swept away clockwise revealing B.
    RadialWipe,
    /// Pixelate Dissolve: grid cells individually flip from A to B.
    PixelateDissolve,
    /// CrossZoom: A zooms out while B zooms in simultaneously.
    CrossZoom,
    /// Wind: horizontal sweep with wavy shear distortion.
    Wind,
    /// Burn: heat-edge burn effect expanding from center.
    Burn,
    /// Dissolve: crossfade with slight blur for a dreamy dissolve.
    Dissolve,
    /// Flip: 3D card flip (scale_x perspective trick).
    Flip,
    /// ClockWipe: radial sweep from 12 o'clock position.
    ClockWipe,
    /// Cube: 3D cube rotation between scenes.
    Cube,
}

/// A definition of a transition between two scenes.
#[derive(Clone)]
pub struct Transition {
    pub from_scene_idx: usize,
    pub to_scene_idx: usize,
    pub start_time: f64,
    pub duration: f64,
    pub kind: TransitionType,
    pub easing: EasingType,
}

/// Returns the SkSL shader source for the given transition type.
pub fn get_transition_shader(kind: &TransitionType) -> &'static str {
    match kind {
        TransitionType::Fade => {
            r#"
            uniform shader imageA;
            uniform shader imageB;
            uniform float progress;
            half4 main(float2 p) {
                half4 colorA = imageA.eval(p);
                half4 colorB = imageB.eval(p);
                return mix(colorA, colorB, progress);
            }
        "#
        }
        TransitionType::SlideLeft => {
            r#"
            uniform shader imageA;
            uniform shader imageB;
            uniform float progress;
            uniform float2 resolution;
            half4 main(float2 p) {
                float x_offset = resolution.x * progress;
                if (p.x < (resolution.x - x_offset)) {
                    return imageA.eval(float2(p.x + x_offset, p.y));
                } else {
                    return imageB.eval(float2(p.x - (resolution.x - x_offset), p.y));
                }
            }
        "#
        }
        TransitionType::SlideRight => {
            r#"
            uniform shader imageA;
            uniform shader imageB;
            uniform float progress;
            uniform float2 resolution;
            half4 main(float2 p) {
                float x_offset = resolution.x * progress;
                if (p.x > x_offset) {
                    return imageA.eval(float2(p.x - x_offset, p.y));
                } else {
                    return imageB.eval(float2(p.x - x_offset + resolution.x, p.y));
                }
            }
        "#
        }
        TransitionType::WipeLeft => {
            r#"
            uniform shader imageA;
            uniform shader imageB;
            uniform float progress;
            uniform float2 resolution;
            half4 main(float2 p) {
                 float boundary = resolution.x * (1.0 - progress);
                 if (p.x < boundary) {
                     return imageA.eval(p);
                 } else {
                     return imageB.eval(p);
                 }
            }
        "#
        }
        TransitionType::WipeRight => {
            r#"
            uniform shader imageA;
            uniform shader imageB;
            uniform float progress;
            uniform float2 resolution;
            half4 main(float2 p) {
                 float boundary = resolution.x * progress;
                 if (p.x > boundary) {
                     return imageA.eval(p);
                 } else {
                     return imageB.eval(p);
                 }
            }
        "#
        }
        TransitionType::CircleOpen => {
            r#"
            uniform shader imageA;
            uniform shader imageB;
            uniform float progress;
            uniform float2 resolution;
            half4 main(float2 p) {
                float2 center = resolution / 2.0;
                float max_radius = length(resolution);
                float current_radius = max_radius * progress;
                float dist = distance(p, center);
                if (dist < current_radius) {
                    return imageB.eval(p);
                } else {
                    return imageA.eval(p);
                }
            }
        "#
        }
        TransitionType::Wave {
            amplitude: _,
            frequency: _,
        } => {
            r#"
            uniform shader imageA;
            uniform shader imageB;
            uniform float progress;
            uniform float2 resolution;
            uniform float amplitude;
            uniform float frequency;
            half4 main(float2 p) {
                float2 center = resolution / 2.0;
                float dist = distance(p, center);
                float wave = sin(dist * frequency - progress * 10.0) * amplitude * (1.0 - progress);
                float2 distorted_p = p + normalize(p - center) * wave;
                half4 colorA = imageA.eval(p);
                half4 colorB = imageB.eval(distorted_p);
                return mix(colorA, colorB, progress);
            }
        "#
        }
        TransitionType::Glitch { intensity: _ } => {
            r#"
            uniform shader imageA;
            uniform shader imageB;
            uniform float progress;
            uniform float2 resolution;
            uniform float intensity;
            half4 main(float2 p) {
                float glitch_amount = intensity * progress * (1.0 - progress) * 4.0;
                float slice_height = resolution.y / 20.0;
                float slice_index = floor(p.y / slice_height);
                float offset = sin(slice_index * 12.9898) * glitch_amount * 50.0;
                float2 distorted_p = float2(p.x + offset, p.y);
                half4 colorA = imageA.eval(p);
                half4 colorB = imageB.eval(distorted_p);
                return mix(colorA, colorB, progress);
            }
        "#
        }
        TransitionType::Iris {
            start_radius: _,
            end_radius: _,
        } => {
            r#"
            uniform shader imageA;
            uniform shader imageB;
            uniform float progress;
            uniform float2 resolution;
            uniform float start_radius;
            uniform float end_radius;
            half4 main(float2 p) {
                float2 center = resolution / 2.0;
                float dist = distance(p, center);
                float current_radius = mix(start_radius, end_radius, progress);
                if (dist < current_radius) {
                    return imageB.eval(p);
                } else {
                    return imageA.eval(p);
                }
            }
        "#
        }
        TransitionType::Spiral { rotations: _ } => {
            r#"
            uniform shader imageA;
            uniform shader imageB;
            uniform float progress;
            uniform float2 resolution;
            uniform float rotations;
            half4 main(float2 p) {
                float2 center = resolution / 2.0;
                float2 uv = (p - center) / resolution;
                float angle = atan(uv.y, uv.x);
                float radius = length(uv);
                float spiral_angle = angle + rotations * 6.28318 * progress;
                float spiral_mask = smoothstep(0.0, 0.1, spiral_angle - progress * 6.28318 * rotations);
                half4 colorA = imageA.eval(p);
                half4 colorB = imageB.eval(p);
                return mix(colorA, colorB, spiral_mask);
            }
        "#
        }
        TransitionType::ZoomIn => {
            r#"
            uniform shader imageA;
            uniform shader imageB;
            uniform float progress;
            uniform float2 resolution;
            half4 main(float2 p) {
                float2 center = resolution / 2.0;
                float scale = 1.0 + progress * 0.3;
                float2 zoomed_p = (p - center) / scale + center;
                half4 colorA = imageA.eval(p);
                half4 colorB = imageB.eval(zoomed_p);
                return mix(colorA, colorB, smoothstep(0.0, 1.0, progress));
            }
        "#
        }
        TransitionType::RadialWipe => {
            r#"
            uniform shader imageA;
            uniform shader imageB;
            uniform float progress;
            uniform float2 resolution;
            half4 main(float2 p) {
                float2 center = resolution / 2.0;
                float2 uv = p - center;
                float angle = atan(uv.y, uv.x);
                float t = (angle / 6.28318) + 0.5;
                t = fract(t + 0.75);
                float mask = 1.0 - smoothstep(progress - 0.02, progress + 0.02, t);
                half4 colorA = imageA.eval(p);
                half4 colorB = imageB.eval(p);
                return mix(colorA, colorB, mask);
            }
        "#
        }
        TransitionType::PixelateDissolve => {
            r#"
            uniform shader imageA;
            uniform shader imageB;
            uniform float progress;
            uniform float2 resolution;
            half4 main(float2 p) {
                float cells = 40.0;
                float2 cell_size = resolution / cells;
                float2 cell = floor(p / cell_size);
                float hash = fract(sin(dot(cell, float2(12.9898, 78.233))) * 43758.5453);
                float reveal = hash * 0.8 + 0.1;
                float mask = smoothstep(reveal - 0.05, reveal + 0.05, progress);
                half4 colorA = imageA.eval(p);
                half4 colorB = imageB.eval(p);
                return mix(colorA, colorB, mask);
            }
        "#
        }
        TransitionType::CrossZoom => {
            r#"
            uniform shader imageA;
            uniform shader imageB;
            uniform float progress;
            uniform float2 resolution;
            half4 main(float2 p) {
                float2 center = resolution / 2.0;
                float scaleA = 1.0 + progress * 0.2;
                float scaleB = 1.0 + (1.0 - progress) * 0.2;
                float2 pa = (p - center) / scaleA + center;
                float2 pb = (p - center) / scaleB + center;
                half4 colorA = imageA.eval(pa);
                half4 colorB = imageB.eval(pb);
                return mix(colorA, colorB, smoothstep(0.0, 1.0, progress));
            }
        "#
        }
        TransitionType::Wind => {
            r#"
            uniform shader imageA;
            uniform shader imageB;
            uniform float progress;
            uniform float2 resolution;
            half4 main(float2 p) {
                float sweep = p.x / resolution.x - progress;
                float wind = sin(p.y * 0.05 + progress * 30.0) * 0.1;
                float edge = sweep + wind;
                float mask = 1.0 - smoothstep(-0.05, 0.05, edge);
                half4 colorA = imageA.eval(p);
                half4 colorB = imageB.eval(p);
                return mix(colorA, colorB, mask);
            }
        "#
        }
        TransitionType::Burn => {
            r#"
            uniform shader imageA;
            uniform shader imageB;
            uniform float progress;
            uniform float2 resolution;
            half4 main(float2 p) {
                float2 center = resolution / 2.0;
                float dist = distance(p, center);
                float radius = length(resolution) * progress * 0.7;
                float edge = dist - radius;
                float heat = exp(-abs(edge) * 0.02);
                half4 colorA = imageA.eval(p);
                half4 colorB = imageB.eval(p);
                colorA.rgb += half3(heat * 0.5, heat * 0.3, 0.0);
                colorA.rgb = min(colorA.rgb, 1.0);
                float reveal = smoothstep(-5.0, 5.0, edge);
                return mix(colorA, colorB, reveal);
            }
        "#
        }
        TransitionType::Dissolve => {
            r#"
            uniform shader imageA;
            uniform shader imageB;
            uniform float progress;
            uniform float2 resolution;
            half4 main(float2 p) {
                float2 center = resolution / 2.0;
                float blur_amount = 20.0 * sin(progress * 3.14159);
                // Simple box blur approximation for dissolve effect
                half4 colorA = half4(0);
                half4 colorB = half4(0);
                for (int dx = -2; dx <= 2; dx++) {
                    for (int dy = -2; dy <= 2; dy++) {
                        float2 offset = float2(float(dx), float(dy)) * blur_amount * 0.2;
                        colorA += imageA.eval(p + offset);
                        colorB += imageB.eval(p + offset);
                    }
                }
                colorA /= 25.0;
                colorB /= 25.0;
                return mix(colorA, colorB, smoothstep(0.0, 1.0, progress));
            }
        "#
        }
        TransitionType::Flip => {
            r#"
            uniform shader imageA;
            uniform shader imageB;
            uniform float progress;
            uniform float2 resolution;
            half4 main(float2 p) {
                float2 center = resolution / 2.0;
                float angle = progress * 3.14159;
                float cos_a = cos(angle);
                // Simulate 3D flip: scale_x goes from 1 → 0 → -1 → 0 → 1
                float scale_x = abs(cos_a);
                // Determine which side we're showing
                float showing_a = step(0.0, cos_a);
                float2 flipped_p = float2(center.x + (p.x - center.x) / max(scale_x, 0.001), p.y);
                flipped_p.x = clamp(flipped_p.x, 0.0, resolution.x);
                half4 colorA = imageA.eval(flipped_p);
                half4 colorB = imageB.eval(flipped_p);
                // Add shadow effect at flip edges
                float shadow = 1.0 - scale_x;
                colorA.rgb *= (1.0 - shadow * 0.3);
                colorB.rgb *= (1.0 - shadow * 0.3);
                return mix(colorB, colorA, showing_a);
            }
        "#
        }
        TransitionType::ClockWipe => {
            r#"
            uniform shader imageA;
            uniform shader imageB;
            uniform float progress;
            uniform float2 resolution;
            half4 main(float2 p) {
                float2 center = resolution / 2.0;
                float2 uv = p - center;
                // Angle from 12 o'clock (top), clockwise
                float angle = atan(uv.x, -uv.y);
                if (angle < 0.0) angle += 6.28318;
                float t = angle / 6.28318;
                float mask = smoothstep(progress - 0.008, progress + 0.008, t);
                half4 colorA = imageA.eval(p);
                half4 colorB = imageB.eval(p);
                return mix(colorB, colorA, mask);
            }
        "#
        }
        TransitionType::Cube => {
            r#"
            uniform shader imageA;
            uniform shader imageB;
            uniform float progress;
            uniform float2 resolution;
            half4 main(float2 p) {
                float2 center = resolution / 2.0;
                float angle = progress * 3.14159;
                float cos_a = cos(angle);
                float sin_a = sin(angle);
                // Determine which face is visible (front or back)
                float showing_a = step(0.0, cos_a);
                // Apply perspective distortion: shift x based on y position and rotation
                float perspective = sin_a * 0.3;
                float x_shift = (p.y - center.y) / resolution.y * perspective * resolution.x;
                float scale = abs(cos_a);
                float2 distorted_p = float2(center.x + (p.x - center.x - x_shift) / max(scale, 0.001), p.y);
                distorted_p.x = clamp(distorted_p.x, 0.0, resolution.x);
                half4 colorA = imageA.eval(distorted_p);
                half4 colorB = imageB.eval(distorted_p);
                // Darken at edges for depth illusion
                float darken = 1.0 - (1.0 - abs(cos_a)) * 0.4;
                colorA.rgb *= darken;
                colorB.rgb *= darken;
                return mix(colorB, colorA, showing_a);
            }
        "#
        }
    }
}

/// Draws a transition effect between two images onto the canvas.
pub fn draw_transition(
    canvas: &skia_safe::Canvas,
    img_a: &skia_safe::Image,
    img_b: &skia_safe::Image,
    progress: f32,
    kind: &TransitionType,
    width: i32,
    height: i32,
) {
    let sksl = get_transition_shader(kind);
    let result = RuntimeEffect::make_for_shader(sksl, None);
    if let Ok(effect) = result {
        let mut uniform_bytes = Vec::new();
        uniform_bytes.extend_from_slice(&progress.to_le_bytes());

        match kind {
            TransitionType::Fade => {}
            _ => {
                uniform_bytes.extend_from_slice(&(width as f32).to_le_bytes());
                uniform_bytes.extend_from_slice(&(height as f32).to_le_bytes());
            }
        }

        // Add transition-specific uniforms
        match kind {
            TransitionType::Wave {
                amplitude,
                frequency,
            } => {
                uniform_bytes.extend_from_slice(&amplitude.to_le_bytes());
                uniform_bytes.extend_from_slice(&frequency.to_le_bytes());
            }
            TransitionType::Glitch { intensity } => {
                uniform_bytes.extend_from_slice(&intensity.to_le_bytes());
            }
            TransitionType::Iris {
                start_radius,
                end_radius,
            } => {
                uniform_bytes.extend_from_slice(&start_radius.to_le_bytes());
                uniform_bytes.extend_from_slice(&end_radius.to_le_bytes());
            }
            TransitionType::Spiral { rotations } => {
                uniform_bytes.extend_from_slice(&rotations.to_le_bytes());
            }
            _ => {}
        }

        let uniforms_data = Data::new_copy(&uniform_bytes);

        let shader_a = img_a
            .to_shader(None, skia_safe::SamplingOptions::default(), None)
            .unwrap();
        let shader_b = img_b
            .to_shader(None, skia_safe::SamplingOptions::default(), None)
            .unwrap();

        let children = [ChildPtr::Shader(shader_a), ChildPtr::Shader(shader_b)];

        if let Some(shader) = effect.make_shader(uniforms_data, &children, None) {
            let mut paint = skia_safe::Paint::default();
            paint.set_shader(Some(shader));
            canvas.draw_rect(
                skia_safe::Rect::from_wh(width as f32, height as f32),
                &paint,
            );
        } else {
            error!("Failed to make shader");
        }
    } else {
        error!("Shader compilation error: {:?}", result.err());
    }
}
