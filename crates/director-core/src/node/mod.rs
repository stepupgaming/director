// NOTE: Custom SkSL shaders are already supported via EffectNode with RuntimeShader.
// 3D rendering via wgpu: WgpuNode (feature-gated behind "wgpu").
use crate::animation::EasingType;
use crate::types::ObjectFit;
use skia_safe::Rect;

pub mod text;
pub use text::TextNode;

pub mod text_animator;
pub use text_animator::{parse_text_anim_property, GlyphAnimState, TextAnimProperty, TextAnimator};

pub mod vector;
pub use vector::VectorNode;

pub mod lottie;
pub use lottie::LottieNode;

pub mod box_node;
pub use box_node::{BoxNode, BoxShadow};

pub mod shape_node;
pub use shape_node::{PathCommand, ShapeNode, ShapeType};

pub mod image_node;
pub use image_node::ImageNode;

pub mod video_node;
pub use video_node::{VideoNode, VideoSource};

pub mod composition;
pub use composition::CompositionNode;

pub mod spectrum_visualizer;
pub use spectrum_visualizer::SpectrumVisualizerNode;

pub mod caption;
pub use caption::{
    apply_phrase_break, auto_time_words, estimate_legibility, format_caption_word,
    parse_srt, parse_vtt, refresh_caption_lines, scaled_safe_zone, CaptionLine, CaptionNode,
    CaptionPosition, CaptionStyle, CaptionWord, PhraseLineMode, REFERENCE_SHORT_FORM_HEIGHT,
};

pub mod effect;
pub mod effect_shaders;
pub use effect::{build_effect_filter, EffectNode, EffectType, ShaderUniform};

#[cfg(feature = "wgpu")]
pub mod wgpu_node;
#[cfg(feature = "wgpu")]
pub use wgpu_node::WgpuNode;

// Helper to parse easing.
//
// `parse_easing` keeps the historical silent-fallback behaviour so existing
// internal callers don't break; new and agent-facing callers should prefer
// `parse_easing_result` so a typo surfaces as an error rather than a
// hand-cranked `Linear` segment.
pub fn parse_easing(e: &str) -> EasingType {
    match parse_easing_result(e) {
        Ok(e) => e,
        Err(msg) => {
            use tracing::warn;
            warn!("{msg}");
            EasingType::Linear
        }
    }
}

/// Loud easing parser for Rhai / agent-facing paths that want a typo to shout
/// in the logs (tracing `error!`) instead of slipping past as a `Linear`
/// segment. Use this everywhere an unknown name would otherwise silently
/// degrade on-screen motion.
pub fn parse_easing_loud(e: &str) -> EasingType {
    match parse_easing_result(e) {
        Ok(e) => e,
        Err(msg) => {
            tracing::error!("{msg}");
            EasingType::Linear
        }
    }
}

/// Strict easing parser used by Rhai / agent-facing paths: returns
/// `Err(message)` on unknown names so the caller can surface it. The message
/// lists every valid name so a typo produced by an LLM becomes self-correcting.
pub fn parse_easing_result(e: &str) -> Result<EasingType, String> {
    match e {
        "linear" => Ok(EasingType::Linear),
        "ease_in" => Ok(EasingType::EaseIn),
        "ease_out" => Ok(EasingType::EaseOut),
        "ease_in_out" => Ok(EasingType::EaseInOut),
        "bounce_out" => Ok(EasingType::BounceOut),
        "bounce_in" => Ok(EasingType::BounceIn),
        "bounce_in_out" => Ok(EasingType::BounceInOut),
        "elastic_out" => Ok(EasingType::ElasticOut),
        "elastic_in" => Ok(EasingType::ElasticIn),
        "elastic_in_out" => Ok(EasingType::ElasticInOut),
        "back_out" => Ok(EasingType::BackOut),
        "back_in" => Ok(EasingType::BackIn),
        "back_in_out" => Ok(EasingType::BackInOut),
        other => Err(format!(
            "Unknown easing '{other}'; valid names: linear, ease_in, ease_out, ease_in_out, \
             bounce_in, bounce_out, bounce_in_out, elastic_in, elastic_out, elastic_in_out, \
             back_in, back_out, back_in_out"
        )),
    }
}

/// Names accepted by `parse_easing` / `parse_easing_result`. Useful for
/// generating API documentation / introspection output without hard-coding
/// the list in two places.
pub const VALID_EASING_NAMES: &[&str] = &[
    "linear",
    "ease_in",
    "ease_out",
    "ease_in_out",
    "bounce_in",
    "bounce_out",
    "bounce_in_out",
    "elastic_in",
    "elastic_out",
    "elastic_in_out",
    "back_in",
    "back_out",
    "back_in_out",
];

pub fn calculate_object_fit_rect(src_w: f32, src_h: f32, dst_rect: Rect, fit: ObjectFit) -> Rect {
    match fit {
        ObjectFit::Fill => dst_rect,
        ObjectFit::Contain | ObjectFit::Cover => {
            let src_ratio = src_w / src_h;
            let dst_w = dst_rect.width();
            let dst_h = dst_rect.height();
            let dst_ratio = dst_w / dst_h;

            let scale = match fit {
                ObjectFit::Contain => {
                    if src_ratio > dst_ratio {
                        dst_w / src_w
                    } else {
                        dst_h / src_h
                    }
                }
                ObjectFit::Cover => {
                    if src_ratio > dst_ratio {
                        dst_h / src_h
                    } else {
                        dst_w / src_w
                    }
                }
                _ => 1.0,
            };

            let new_w = src_w * scale;
            let new_h = src_h * scale;
            let new_x = dst_rect.left + (dst_w - new_w) / 2.0;
            let new_y = dst_rect.top + (dst_h - new_h) / 2.0;

            Rect::from_xywh(new_x, new_y, new_w, new_h)
        }
    }
}

#[cfg(test)]
mod easing_tests {
    use super::*;

    #[test]
    fn parse_easing_result_accepts_every_documented_name() {
        for name in VALID_EASING_NAMES {
            assert!(
                parse_easing_result(name).is_ok(),
                "{name} should be a recognized easing"
            );
        }
    }

    #[test]
    fn parse_easing_result_rejects_unknown_names_with_self_correcting_message() {
        let err = parse_easing_result("eaze-inn").unwrap_err();
        // The error message must list every valid name so an LLM producing a
        // typo has the corrective set in front of it.
        for name in VALID_EASING_NAMES {
            assert!(err.contains(name), "Help message missing valid name '{name}'");
        }
        assert!(err.contains("eaze-inn"));
    }

    #[test]
    fn parse_easing_loud_falls_back_to_linear_but_flags_via_logger() {
        // The function still produces *some* EasingType so existing call
        // shapes compile, but tracing::error fires. We verify the return is
        // Linear (the safe fallback) and that the function is callable.
        assert!(matches!(
            parse_easing_loud("nonsense"),
            crate::animation::EasingType::Linear
        ));
        assert!(matches!(
            parse_easing_loud("linear"),
            crate::animation::EasingType::Linear
        ));
    }
}
