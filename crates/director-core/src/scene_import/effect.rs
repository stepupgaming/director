//! Effect nodes from their catalog-named effects.
//!
//! [`effect_type`] inverts `scene_export::effect::export_effect`: one arm per
//! catalog name, one parameter read per struct field.

use super::nodes::mismatch;
use super::{animated, animated_with, color, color_animated, ImportContext, ImportError};
use crate::animation::{Animated, TweenableVector};
use crate::element::Element;
use crate::node::effect::{EffectType, ShaderUniform};
use crate::node::EffectNode;
use crate::types::Color;
use director_scene::{Effect, EffectParam, NodeBody, NodeRecord, Uniform};
use std::collections::{BTreeMap, HashMap};
use taffy::style::Style;

pub(crate) fn decode_effect(record: &NodeRecord, cx: &mut ImportContext) -> Result<Box<dyn Element>, ImportError> {
    let NodeBody::Effect(body) = &record.body else {
        return Err(mismatch(record));
    };
    let effects = body.effects.iter().map(effect_type).collect::<Result<Vec<_>, _>>()?;
    Ok(Box::new(EffectNode {
        effects,
        style: Style::default(),
        shader_cache: cx.assets.shader_cache.clone(),
        current_time: 0.0,
    }))
}

/// The parameters of one effect, read by catalog key.
struct Params<'a> {
    kind: &'a str,
    params: &'a BTreeMap<String, EffectParam>,
}

impl Params<'_> {
    fn get(&self, key: &str) -> Result<&EffectParam, ImportError> {
        self.params
            .get(key)
            .ok_or_else(|| ImportError::Invalid(format!("effect `{}` is missing param `{key}`", self.kind)))
    }

    fn wrong(&self, key: &str, want: &str) -> ImportError {
        ImportError::Invalid(format!("effect `{}` param `{key}` must be {want}", self.kind))
    }

    fn scalar(&self, key: &str) -> Result<Animated<f32>, ImportError> {
        match self.get(key)? {
            EffectParam::Scalar(channel) => animated(channel),
            _ => Err(self.wrong(key, "a scalar")),
        }
    }

    fn color(&self, key: &str) -> Result<Color, ImportError> {
        match self.get(key)? {
            EffectParam::Color(value) => Ok(color(*value)),
            _ => Err(self.wrong(key, "a color")),
        }
    }

    fn color_channel(&self, key: &str) -> Result<Animated<Color>, ImportError> {
        match self.get(key)? {
            EffectParam::ColorChannel(channel) => color_animated(channel),
            _ => Err(self.wrong(key, "a color channel")),
        }
    }

    fn count(&self, key: &str) -> Result<u32, ImportError> {
        match self.get(key)? {
            EffectParam::Count(value) => Ok(*value),
            _ => Err(self.wrong(key, "a count")),
        }
    }

    fn numbers(&self, key: &str) -> Result<Vec<f32>, ImportError> {
        match self.get(key)? {
            EffectParam::Numbers(values) => Ok(values.clone()),
            _ => Err(self.wrong(key, "a list of numbers")),
        }
    }

    fn text(&self, key: &str) -> Result<String, ImportError> {
        match self.get(key)? {
            EffectParam::Text(value) => Ok(value.clone()),
            _ => Err(self.wrong(key, "text")),
        }
    }

    fn uniforms(&self, key: &str) -> Result<HashMap<String, ShaderUniform>, ImportError> {
        let EffectParam::Uniforms(uniforms) = self.get(key)? else {
            return Err(self.wrong(key, "a uniform table"));
        };
        uniforms
            .iter()
            .map(|(name, uniform)| Ok((name.clone(), shader_uniform(uniform)?)))
            .collect()
    }
}

fn shader_uniform(uniform: &Uniform) -> Result<ShaderUniform, ImportError> {
    Ok(match uniform {
        Uniform::Float(channel) => ShaderUniform::Float(animated(channel)?),
        Uniform::Vector(channel) => ShaderUniform::Vec(animated_with(channel, |value| TweenableVector(value.clone()))?),
    })
}

/// The effect a catalog entry names, with its parameters restored.
pub fn effect_type(effect: &Effect) -> Result<EffectType, ImportError> {
    let p = Params {
        kind: &effect.kind,
        params: &effect.params,
    };
    Ok(match effect.kind.as_str() {
        "blur" => EffectType::Blur(p.scalar("sigma")?),
        "color_matrix" => EffectType::ColorMatrix(p.numbers("matrix")?),
        "shader" => EffectType::RuntimeShader {
            sksl: p.text("code")?,
            uniforms: p.uniforms("uniforms")?,
        },
        "drop_shadow" => EffectType::DropShadow {
            blur: p.scalar("blur")?,
            offset_x: p.scalar("offset_x")?,
            offset_y: p.scalar("offset_y")?,
            color: p.color_channel("color")?,
        },
        "directional_blur" => EffectType::DirectionalBlur {
            strength: p.scalar("strength")?,
            angle: p.scalar("angle")?,
            samples: p.count("samples")?,
        },
        "grain" => EffectType::FilmGrain {
            intensity: p.scalar("intensity")?,
            size: p.scalar("size")?,
        },
        "vignette" => EffectType::Vignette {
            intensity: p.scalar("intensity")?,
            radius: p.scalar("radius")?,
            feather: p.scalar("feather")?,
        },
        "chromatic_aberration" => EffectType::ChromaticAberration(p.scalar("strength")?),
        "pixelate" => EffectType::Pixelate(p.scalar("block_size")?),
        "sharpen" => EffectType::Sharpen(p.scalar("strength")?),
        "glow" => EffectType::Glow {
            intensity: p.scalar("intensity")?,
            threshold: p.scalar("threshold")?,
        },
        "warp" => EffectType::Warp {
            amplitude: p.scalar("amplitude")?,
            frequency: p.scalar("frequency")?,
            direction: p.scalar("direction")?,
            speed: p.scalar("speed")?,
        },
        "swirl" => EffectType::Swirl {
            radius: p.scalar("radius")?,
            angle: p.scalar("angle")?,
        },
        "ripple" => EffectType::Ripple {
            amplitude: p.scalar("amplitude")?,
            frequency: p.scalar("frequency")?,
            speed: p.scalar("speed")?,
        },
        "glass" => EffectType::Glass {
            scale: p.scalar("scale")?,
            intensity: p.scalar("intensity")?,
        },
        "shake" => EffectType::Shake {
            intensity: p.scalar("intensity")?,
            speed: p.scalar("speed")?,
        },
        "duotone" => EffectType::Duotone {
            shadow: p.color("shadow")?,
            highlight: p.color("highlight")?,
        },
        "posterize" => EffectType::Posterize(p.scalar("levels")?),
        "threshold" => EffectType::Threshold(p.scalar("level")?),
        "scanlines" => EffectType::Scanlines {
            intensity: p.scalar("intensity")?,
            count: p.scalar("count")?,
        },
        "glitch" => EffectType::Glitch {
            intensity: p.scalar("intensity")?,
            speed: p.scalar("speed")?,
        },
        "edge_detect" => EffectType::EdgeDetect(p.scalar("intensity")?),
        "zoom_blur" => EffectType::ZoomBlur(p.scalar("strength")?),
        "kaleidoscope" => EffectType::Kaleidoscope(p.scalar("segments")?),
        "thermal" => EffectType::Thermal,
        "liquid" => EffectType::Liquid {
            frequency: p.scalar("frequency")?,
            amplitude: p.scalar("amplitude")?,
        },
        "acid" => EffectType::Acid {
            color: p.color("color")?,
            speed: p.scalar("speed")?,
        },
        "halftone" => EffectType::Halftone(p.scalar("dot_size")?),
        "ascii" => EffectType::Ascii {
            scale: p.scalar("scale")?,
            color: p.color("color")?,
        },
        "sketch" => EffectType::Sketch(p.scalar("contrast")?),
        "mirror" => EffectType::Mirror,
        "tritone" => EffectType::TriTone {
            shadow: p.color("shadow")?,
            mid: p.color("mid")?,
            highlight: p.color("highlight")?,
            midpoint: p.scalar("midpoint")?,
        },
        "vhs" => EffectType::Vhs {
            distortion: p.scalar("distortion")?,
            scanline: p.scalar("scanline")?,
        },
        "neon_edge" => EffectType::NeonEdge {
            glow_color: p.color("glow_color")?,
            threshold: p.scalar("threshold")?,
        },
        "fisheye" => EffectType::Fisheye(p.scalar("strength")?),
        "bulge" => EffectType::Bulge {
            strength: p.scalar("strength")?,
            radius: p.scalar("radius")?,
        },
        "barrel" => EffectType::Barrel(p.scalar("curvature")?),
        "lens_flare" => EffectType::LensFlare {
            intensity: p.scalar("intensity")?,
            color: p.color("color")?,
        },
        "noir" => EffectType::Noir(p.scalar("contrast")?),
        "cyberpunk" => EffectType::Cyberpunk(p.scalar("intensity")?),
        "vintage" => EffectType::Vintage(p.scalar("amount")?),
        "gameboy" => EffectType::Gameboy,
        "bleach_bypass" => EffectType::BleachBypass(p.scalar("amount")?),
        "teal_orange" => EffectType::TealOrange(p.scalar("amount")?),
        "plasma" => EffectType::Plasma {
            scale: p.scalar("scale")?,
            speed: p.scalar("speed")?,
        },
        "hologram" => EffectType::Hologram(p.scalar("intensity")?),
        "emboss" => EffectType::Emboss {
            angle: p.scalar("angle")?,
            strength: p.scalar("strength")?,
        },
        "toon" => EffectType::Toon {
            levels: p.scalar("levels")?,
            edge: p.scalar("edge")?,
        },
        "oil_paint" => EffectType::OilPaint {
            block_size: p.scalar("block_size")?,
            levels: p.scalar("levels")?,
        },
        "light_leak" => EffectType::LightLeak {
            intensity: p.scalar("intensity")?,
            color: p.color("color")?,
        },
        "aurora" => EffectType::Aurora {
            intensity: p.scalar("intensity")?,
            speed: p.scalar("speed")?,
        },
        "sunrays" => EffectType::SunRays {
            intensity: p.scalar("intensity")?,
            angle: p.scalar("angle")?,
            speed: p.scalar("speed")?,
        },
        "skin_smooth" => EffectType::SkinSmooth {
            intensity: p.scalar("intensity")?,
            radius: p.scalar("radius")?,
        },
        "beauty_glow" => EffectType::BeautyGlow {
            intensity: p.scalar("intensity")?,
            threshold: p.scalar("threshold")?,
        },
        "soft_focus" => EffectType::SoftFocus {
            amount: p.scalar("amount")?,
        },
        "vibrance" => EffectType::Vibrance {
            amount: p.scalar("amount")?,
        },
        "fog" => EffectType::Fog {
            intensity: p.scalar("intensity")?,
            color: p.color("color")?,
        },
        "mist" => EffectType::Mist {
            intensity: p.scalar("intensity")?,
            speed: p.scalar("speed")?,
        },
        "rain" => EffectType::Rain {
            intensity: p.scalar("intensity")?,
            speed: p.scalar("speed")?,
        },
        "snow" => EffectType::Snow {
            intensity: p.scalar("intensity")?,
            speed: p.scalar("speed")?,
            size: p.scalar("size")?,
        },
        "dust" => EffectType::Dust {
            intensity: p.scalar("intensity")?,
            speed: p.scalar("speed")?,
        },
        "bokeh_dots" => EffectType::BokehDots {
            intensity: p.scalar("intensity")?,
            size: p.scalar("size")?,
            color: p.color("color")?,
        },
        "film_look" => EffectType::FilmLook {
            amount: p.scalar("amount")?,
            grain: p.scalar("grain")?,
        },
        "orange_teal" => EffectType::OrangeTeal {
            amount: p.scalar("amount")?,
        },
        "cross_process" => EffectType::CrossProcess {
            amount: p.scalar("amount")?,
        },
        "polaroid" => EffectType::Polaroid {
            amount: p.scalar("amount")?,
        },
        "faded_film" => EffectType::FadedFilm {
            amount: p.scalar("amount")?,
        },
        "midnight" => EffectType::Midnight {
            amount: p.scalar("amount")?,
        },
        "watercolor" => EffectType::Watercolor {
            intensity: p.scalar("intensity")?,
            edge: p.scalar("edge")?,
        },
        "crosshatch" => EffectType::Crosshatch {
            intensity: p.scalar("intensity")?,
            density: p.scalar("density")?,
        },
        "pointillism" => EffectType::Pointillism {
            dot_size: p.scalar("dot_size")?,
            intensity: p.scalar("intensity")?,
        },
        "mosaic" => EffectType::Mosaic {
            tile_size: p.scalar("tile_size")?,
        },
        "ink" => EffectType::Ink {
            threshold: p.scalar("threshold")?,
            intensity: p.scalar("intensity")?,
        },
        "pencil_color" => EffectType::PencilColor {
            intensity: p.scalar("intensity")?,
        },
        other => return Err(ImportError::Invalid(format!("unknown effect `{other}`"))),
    })
}
