//! Effect nodes and their catalog-named effects.
//!
//! Each [`EffectType`] variant exports under the primary name of its
//! `api_catalog::EFFECT_CATALOG` entry, with one parameter per struct field.

use super::{
    channel, color_channel, rgba, vector_channel, ElementExport, ExportContext, ExportError,
};
use crate::animation::Animated;
use crate::node::effect::{EffectType, ShaderUniform};
use crate::node::EffectNode;
use crate::types::Color;
use director_scene::{Channel, Effect, EffectBody, EffectParam, NodeBody, Uniform};
use std::collections::BTreeMap;

pub(crate) fn effect_body(
    node: &EffectNode,
    _cx: &mut ExportContext,
) -> Result<ElementExport, ExportError> {
    let effects = node
        .effects
        .iter()
        .map(export_effect)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ElementExport {
        opacity: Channel::still(1.0),
        body: NodeBody::Effect(EffectBody { effects }),
    })
}

fn effect(kind: &str, params: impl IntoIterator<Item = (&'static str, EffectParam)>) -> Effect {
    Effect {
        kind: kind.to_string(),
        params: params
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    }
}

fn scalar(value: &Animated<f32>) -> EffectParam {
    EffectParam::Scalar(channel(value))
}

fn color(value: Color) -> EffectParam {
    EffectParam::Color(rgba(value))
}

fn uniform(value: &ShaderUniform) -> Uniform {
    match value {
        ShaderUniform::Float(a) => Uniform::Float(channel(a)),
        ShaderUniform::Vec(a) => Uniform::Vector(vector_channel(a)),
    }
}

pub(crate) fn export_effect(effect_type: &EffectType) -> Result<Effect, ExportError> {
    Ok(match effect_type {
        EffectType::Blur(sigma) => effect("blur", [("sigma", scalar(sigma))]),
        EffectType::ColorMatrix(matrix) => {
            effect("color_matrix", [("matrix", EffectParam::Numbers(matrix.clone()))])
        }
        EffectType::RuntimeShader { sksl, uniforms } => {
            let uniforms: BTreeMap<String, Uniform> = uniforms
                .iter()
                .map(|(name, value)| (name.clone(), uniform(value)))
                .collect();
            effect(
                "shader",
                [
                    ("code", EffectParam::Text(sksl.clone())),
                    ("uniforms", EffectParam::Uniforms(uniforms)),
                ],
            )
        }
        EffectType::DropShadow {
            blur,
            offset_x,
            offset_y,
            color,
        } => effect(
            "drop_shadow",
            [
                ("blur", scalar(blur)),
                ("offset_x", scalar(offset_x)),
                ("offset_y", scalar(offset_y)),
                ("color", EffectParam::ColorChannel(color_channel(color))),
            ],
        ),
        EffectType::DirectionalBlur {
            strength,
            angle,
            samples,
        } => effect(
            "directional_blur",
            [
                ("strength", scalar(strength)),
                ("angle", scalar(angle)),
                ("samples", EffectParam::Count(*samples)),
            ],
        ),
        EffectType::FilmGrain { intensity, size } => effect(
            "grain",
            [("intensity", scalar(intensity)), ("size", scalar(size))],
        ),
        EffectType::Vignette {
            intensity,
            radius,
            feather,
        } => effect(
            "vignette",
            [
                ("intensity", scalar(intensity)),
                ("radius", scalar(radius)),
                ("feather", scalar(feather)),
            ],
        ),
        EffectType::ChromaticAberration(strength) => {
            effect("chromatic_aberration", [("strength", scalar(strength))])
        }
        EffectType::Pixelate(block_size) => {
            effect("pixelate", [("block_size", scalar(block_size))])
        }
        EffectType::Sharpen(strength) => effect("sharpen", [("strength", scalar(strength))]),
        EffectType::Glow {
            intensity,
            threshold,
        } => effect(
            "glow",
            [
                ("intensity", scalar(intensity)),
                ("threshold", scalar(threshold)),
            ],
        ),
        EffectType::Warp {
            amplitude,
            frequency,
            direction,
            speed,
        } => effect(
            "warp",
            [
                ("amplitude", scalar(amplitude)),
                ("frequency", scalar(frequency)),
                ("direction", scalar(direction)),
                ("speed", scalar(speed)),
            ],
        ),
        EffectType::Swirl { radius, angle } => effect(
            "swirl",
            [("radius", scalar(radius)), ("angle", scalar(angle))],
        ),
        EffectType::Ripple {
            amplitude,
            frequency,
            speed,
        } => effect(
            "ripple",
            [
                ("amplitude", scalar(amplitude)),
                ("frequency", scalar(frequency)),
                ("speed", scalar(speed)),
            ],
        ),
        EffectType::Glass { scale, intensity } => effect(
            "glass",
            [("scale", scalar(scale)), ("intensity", scalar(intensity))],
        ),
        EffectType::Shake { intensity, speed } => effect(
            "shake",
            [("intensity", scalar(intensity)), ("speed", scalar(speed))],
        ),
        EffectType::Duotone { shadow, highlight } => effect(
            "duotone",
            [("shadow", color(*shadow)), ("highlight", color(*highlight))],
        ),
        EffectType::Posterize(levels) => effect("posterize", [("levels", scalar(levels))]),
        EffectType::Threshold(level) => effect("threshold", [("level", scalar(level))]),
        EffectType::Scanlines { intensity, count } => effect(
            "scanlines",
            [("intensity", scalar(intensity)), ("count", scalar(count))],
        ),
        EffectType::Glitch { intensity, speed } => effect(
            "glitch",
            [("intensity", scalar(intensity)), ("speed", scalar(speed))],
        ),
        EffectType::EdgeDetect(intensity) => {
            effect("edge_detect", [("intensity", scalar(intensity))])
        }
        EffectType::ZoomBlur(strength) => effect("zoom_blur", [("strength", scalar(strength))]),
        EffectType::Kaleidoscope(segments) => {
            effect("kaleidoscope", [("segments", scalar(segments))])
        }
        EffectType::Thermal => effect("thermal", []),
        EffectType::Liquid {
            frequency,
            amplitude,
        } => effect(
            "liquid",
            [
                ("frequency", scalar(frequency)),
                ("amplitude", scalar(amplitude)),
            ],
        ),
        EffectType::Acid { color: tint, speed } => effect(
            "acid",
            [("color", color(*tint)), ("speed", scalar(speed))],
        ),
        EffectType::Halftone(dot_size) => effect("halftone", [("dot_size", scalar(dot_size))]),
        EffectType::Ascii { scale, color: tint } => effect(
            "ascii",
            [("scale", scalar(scale)), ("color", color(*tint))],
        ),
        EffectType::Sketch(contrast) => effect("sketch", [("contrast", scalar(contrast))]),
        EffectType::Mirror => effect("mirror", []),
        EffectType::TriTone {
            shadow,
            mid,
            highlight,
            midpoint,
        } => effect(
            "tritone",
            [
                ("shadow", color(*shadow)),
                ("mid", color(*mid)),
                ("highlight", color(*highlight)),
                ("midpoint", scalar(midpoint)),
            ],
        ),
        EffectType::Vhs {
            distortion,
            scanline,
        } => effect(
            "vhs",
            [
                ("distortion", scalar(distortion)),
                ("scanline", scalar(scanline)),
            ],
        ),
        EffectType::NeonEdge {
            glow_color,
            threshold,
        } => effect(
            "neon_edge",
            [
                ("glow_color", color(*glow_color)),
                ("threshold", scalar(threshold)),
            ],
        ),
        EffectType::Fisheye(strength) => effect("fisheye", [("strength", scalar(strength))]),
        EffectType::Bulge { strength, radius } => effect(
            "bulge",
            [("strength", scalar(strength)), ("radius", scalar(radius))],
        ),
        EffectType::Barrel(curvature) => effect("barrel", [("curvature", scalar(curvature))]),
        EffectType::LensFlare {
            intensity,
            color: tint,
        } => effect(
            "lens_flare",
            [("intensity", scalar(intensity)), ("color", color(*tint))],
        ),
        EffectType::Noir(contrast) => effect("noir", [("contrast", scalar(contrast))]),
        EffectType::Cyberpunk(intensity) => {
            effect("cyberpunk", [("intensity", scalar(intensity))])
        }
        EffectType::Vintage(amount) => effect("vintage", [("amount", scalar(amount))]),
        EffectType::Gameboy => effect("gameboy", []),
        EffectType::BleachBypass(amount) => {
            effect("bleach_bypass", [("amount", scalar(amount))])
        }
        EffectType::TealOrange(amount) => effect("teal_orange", [("amount", scalar(amount))]),
        EffectType::Plasma { scale, speed } => effect(
            "plasma",
            [("scale", scalar(scale)), ("speed", scalar(speed))],
        ),
        EffectType::Hologram(intensity) => effect("hologram", [("intensity", scalar(intensity))]),
        EffectType::Emboss { angle, strength } => effect(
            "emboss",
            [("angle", scalar(angle)), ("strength", scalar(strength))],
        ),
        EffectType::Toon { levels, edge } => effect(
            "toon",
            [("levels", scalar(levels)), ("edge", scalar(edge))],
        ),
        EffectType::OilPaint { block_size, levels } => effect(
            "oil_paint",
            [("block_size", scalar(block_size)), ("levels", scalar(levels))],
        ),
        EffectType::LightLeak {
            intensity,
            color: tint,
        } => effect(
            "light_leak",
            [("intensity", scalar(intensity)), ("color", color(*tint))],
        ),
        EffectType::Aurora { intensity, speed } => effect(
            "aurora",
            [("intensity", scalar(intensity)), ("speed", scalar(speed))],
        ),
        EffectType::SunRays {
            intensity,
            angle,
            speed,
        } => effect(
            "sunrays",
            [
                ("intensity", scalar(intensity)),
                ("angle", scalar(angle)),
                ("speed", scalar(speed)),
            ],
        ),
        EffectType::SkinSmooth { intensity, radius } => effect(
            "skin_smooth",
            [("intensity", scalar(intensity)), ("radius", scalar(radius))],
        ),
        EffectType::BeautyGlow {
            intensity,
            threshold,
        } => effect(
            "beauty_glow",
            [
                ("intensity", scalar(intensity)),
                ("threshold", scalar(threshold)),
            ],
        ),
        EffectType::SoftFocus { amount } => effect("soft_focus", [("amount", scalar(amount))]),
        EffectType::Vibrance { amount } => effect("vibrance", [("amount", scalar(amount))]),
        EffectType::Fog {
            intensity,
            color: tint,
        } => effect(
            "fog",
            [("intensity", scalar(intensity)), ("color", color(*tint))],
        ),
        EffectType::Mist { intensity, speed } => effect(
            "mist",
            [("intensity", scalar(intensity)), ("speed", scalar(speed))],
        ),
        EffectType::Rain { intensity, speed } => effect(
            "rain",
            [("intensity", scalar(intensity)), ("speed", scalar(speed))],
        ),
        EffectType::Snow {
            intensity,
            speed,
            size,
        } => effect(
            "snow",
            [
                ("intensity", scalar(intensity)),
                ("speed", scalar(speed)),
                ("size", scalar(size)),
            ],
        ),
        EffectType::Dust { intensity, speed } => effect(
            "dust",
            [("intensity", scalar(intensity)), ("speed", scalar(speed))],
        ),
        EffectType::BokehDots {
            intensity,
            size,
            color: tint,
        } => effect(
            "bokeh_dots",
            [
                ("intensity", scalar(intensity)),
                ("size", scalar(size)),
                ("color", color(*tint)),
            ],
        ),
        EffectType::FilmLook { amount, grain } => effect(
            "film_look",
            [("amount", scalar(amount)), ("grain", scalar(grain))],
        ),
        EffectType::OrangeTeal { amount } => effect("orange_teal", [("amount", scalar(amount))]),
        EffectType::CrossProcess { amount } => {
            effect("cross_process", [("amount", scalar(amount))])
        }
        EffectType::Polaroid { amount } => effect("polaroid", [("amount", scalar(amount))]),
        EffectType::FadedFilm { amount } => effect("faded_film", [("amount", scalar(amount))]),
        EffectType::Midnight { amount } => effect("midnight", [("amount", scalar(amount))]),
        EffectType::Watercolor { intensity, edge } => effect(
            "watercolor",
            [("intensity", scalar(intensity)), ("edge", scalar(edge))],
        ),
        EffectType::Crosshatch { intensity, density } => effect(
            "crosshatch",
            [("intensity", scalar(intensity)), ("density", scalar(density))],
        ),
        EffectType::Pointillism {
            dot_size,
            intensity,
        } => effect(
            "pointillism",
            [("dot_size", scalar(dot_size)), ("intensity", scalar(intensity))],
        ),
        EffectType::Mosaic { tile_size } => effect("mosaic", [("tile_size", scalar(tile_size))]),
        EffectType::Ink {
            threshold,
            intensity,
        } => effect(
            "ink",
            [
                ("threshold", scalar(threshold)),
                ("intensity", scalar(intensity)),
            ],
        ),
        EffectType::PencilColor { intensity } => {
            effect("pencil_color", [("intensity", scalar(intensity))])
        }
    })
}
