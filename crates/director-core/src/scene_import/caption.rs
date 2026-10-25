//! Caption and smart caption elements from their bodies.

use super::nodes::{mismatch, text_align};
use super::{animated, color, ImportContext, ImportError};
use crate::element::Element;
use crate::entity::EntityId;
use crate::node::caption::{CaptionLine, CaptionNode, CaptionPosition, CaptionStyle, CaptionWord, PhraseLineMode};
use crate::node::smart_caption::SmartCaptionConfig;
use crate::node::SmartCaptionNode;
use crate::semantic::{Anchor, CaptionTrajectory, SemanticManifest, SolverConfig, TrajectorySample};
use crate::text_direction::TextDirectionMode;
use director_scene::caption as ds;
use director_scene::{Channel, NodeBody, NodeRecord, TextDirection};

pub(crate) fn decode_caption(record: &NodeRecord, cx: &mut ImportContext) -> Result<Box<dyn Element>, ImportError> {
    let NodeBody::Caption(body) = &record.body else {
        return Err(mismatch(record));
    };
    Ok(Box::new(caption_node(body, &record.opacity, cx)?))
}

pub(crate) fn decode_smart_caption(
    record: &NodeRecord,
    cx: &mut ImportContext,
) -> Result<Box<dyn Element>, ImportError> {
    let NodeBody::SmartCaption(body) = &record.body else {
        return Err(mismatch(record));
    };
    Ok(Box::new(smart_caption_node(body, &record.opacity, cx)?))
}

/// A caption node from its body, with the context's fonts.
pub fn caption_node(body: &ds::CaptionBody, opacity: &Channel, cx: &ImportContext) -> Result<CaptionNode, ImportError> {
    let mut node = CaptionNode::new(Vec::new(), cx.assets.font_collection.clone());
    node.lines = body.lines.iter().map(caption_line).collect();
    node.style = caption_style(&body.style);
    node.opacity = animated(opacity)?;
    node.after_style_changed();
    node.rebuild_layout_style();
    Ok(node)
}

/// A smart caption node from its body, with the trajectory it was solved to.
pub fn smart_caption_node(
    body: &ds::SmartCaptionBody,
    opacity: &Channel,
    cx: &ImportContext,
) -> Result<SmartCaptionNode, ImportError> {
    let inner = caption_node(&body.caption, opacity, cx)?;
    let trajectory = CaptionTrajectory::new(body.trajectory.samples.iter().map(trajectory_sample).collect());
    let manifest = body.manifest.as_ref().map(manifest).transpose()?;
    let config = body.config.as_ref().map(smart_caption_config);
    Ok(SmartCaptionNode::from_parts(
        inner,
        trajectory,
        body.canvas_w,
        body.canvas_h,
        body.continuous,
        manifest,
        config,
    ))
}

fn caption_line(line: &ds::CaptionLine) -> CaptionLine {
    CaptionLine {
        text: line.text.clone(),
        words: line
            .words
            .iter()
            .map(|word| CaptionWord {
                text: word.text.clone(),
                start_sec: word.start_sec,
                end_sec: word.end_sec,
                display_line_idx: word.display_line_idx,
            })
            .collect(),
        start_sec: line.start_sec,
        end_sec: line.end_sec,
    }
}

fn caption_style(style: &ds::CaptionStyle) -> CaptionStyle {
    CaptionStyle {
        font_size: style.font_size,
        color: color(style.color),
        highlight_color: color(style.highlight_color),
        spoken_color: color(style.spoken_color),
        position: caption_position(style.position),
        text_align: text_align(style.text_align),
        left_margin: style.left_margin,
        right_margin: style.right_margin,
        text_direction: match style.text_direction {
            TextDirection::Auto => TextDirectionMode::Auto,
            TextDirection::Ltr => TextDirectionMode::Ltr,
            TextDirection::Rtl => TextDirectionMode::Rtl,
        },
        font_family: style.font_family.clone(),
        font_weight: style.font_weight,
        bottom_margin: style.bottom_margin,
        top_margin: style.top_margin,
        line_spacing: style.line_spacing,
        layout_width_fraction: style.layout_width_fraction,
        active_opacity: style.active_opacity,
        future_opacity: style.future_opacity,
        spoken_color_opacity: style.spoken_color_opacity,
        color_transition_duration: style.color_transition_duration,
        active_pop_scale: style.active_pop_scale,
        active_pop_duration: style.active_pop_duration,
        text_shadow_enabled: style.text_shadow_enabled,
        text_shadow_color: color(style.text_shadow_color),
        text_shadow_offset_x: style.text_shadow_offset_x,
        text_shadow_offset_y: style.text_shadow_offset_y,
        text_shadow_blur: style.text_shadow_blur,
        auto_adapt_shadow: style.auto_adapt_shadow,
        stroke_enabled: style.stroke_enabled,
        stroke_width: style.stroke_width,
        stroke_color: color(style.stroke_color),
        glow_enabled: style.glow_enabled,
        glow_blur: style.glow_blur,
        line_reveal_offsets: style.line_reveal_offsets.clone(),
        line_reveal_durations: style.line_reveal_durations.clone(),
        pill_enabled: style.pill_enabled,
        pill_color: color(style.pill_color),
        pill_adaptive: style.pill_adaptive,
        pill_padding_h: style.pill_padding_h,
        pill_padding_v: style.pill_padding_v,
        pill_radius: style.pill_radius,
        auto_phrase_break: style.auto_phrase_break,
        max_words_per_line: style.max_words_per_line,
        phrase_line_mode: match style.phrase_line_mode {
            ds::PhraseLineMode::Replace => PhraseLineMode::Replace,
            ds::PhraseLineMode::Stack => PhraseLineMode::Stack,
        },
        safe_area_preset: style.safe_area_preset.clone(),
        scale_with_canvas: style.scale_with_canvas,
        reference_canvas_height: style.reference_canvas_height,
        uppercase: style.uppercase,
    }
}

fn caption_position(position: ds::CaptionPosition) -> CaptionPosition {
    match position {
        ds::CaptionPosition::Bottom => CaptionPosition::Bottom,
        ds::CaptionPosition::Top => CaptionPosition::Top,
        ds::CaptionPosition::Center => CaptionPosition::Center,
        ds::CaptionPosition::Left => CaptionPosition::Left,
        ds::CaptionPosition::Right => CaptionPosition::Right,
        ds::CaptionPosition::TopLeft => CaptionPosition::TopLeft,
        ds::CaptionPosition::TopRight => CaptionPosition::TopRight,
        ds::CaptionPosition::BottomLeft => CaptionPosition::BottomLeft,
        ds::CaptionPosition::BottomRight => CaptionPosition::BottomRight,
        ds::CaptionPosition::Free { y } => CaptionPosition::Free(y),
        ds::CaptionPosition::FreeXY { x, y } => CaptionPosition::FreeXY { x, y },
    }
}

fn trajectory_sample(sample: &ds::TrajectorySample) -> TrajectorySample {
    TrajectorySample {
        time: sample.time,
        x: sample.x,
        y: sample.y,
        anchor: sample.anchor.map(anchor),
    }
}

fn anchor(anchor: ds::CaptionAnchor) -> Anchor {
    match anchor {
        ds::CaptionAnchor::TopLeft => Anchor::TopLeft,
        ds::CaptionAnchor::TopCenter => Anchor::TopCenter,
        ds::CaptionAnchor::TopRight => Anchor::TopRight,
        ds::CaptionAnchor::BottomLeft => Anchor::BottomLeft,
        ds::CaptionAnchor::BottomCenter => Anchor::BottomCenter,
        ds::CaptionAnchor::BottomRight => Anchor::BottomRight,
        ds::CaptionAnchor::Left => Anchor::Left,
        ds::CaptionAnchor::Right => Anchor::Right,
    }
}

fn manifest(manifest: &ds::SemanticManifest) -> Result<SemanticManifest, ImportError> {
    let entity_refs = manifest
        .entity_refs
        .iter()
        .map(|id| {
            EntityId::parse_str(id)
                .map_err(|error| ImportError::Invalid(format!("smart_caption entity ref `{id}`: {error}")))
        })
        .collect::<Result<_, _>>()?;
    Ok(SemanticManifest {
        entity_refs,
        start_time: manifest.start_time,
        end_time: manifest.end_time,
        solver: solver_config(&manifest.solver),
    })
}

fn solver_config(solver: &ds::SolverConfig) -> SolverConfig {
    SolverConfig {
        clearance: solver.clearance,
        smoothing_window: solver.smoothing_window,
        motion_penalty: solver.motion_penalty,
        overlap_penalty: solver.overlap_penalty,
        safe_zone_penalty: solver.safe_zone_penalty,
        grid_steps: solver.grid_steps,
        negative_space_weight: solver.negative_space_weight,
        smoothness_weight: solver.smoothness_weight,
    }
}

fn smart_caption_config(config: &ds::SmartCaptionConfig) -> SmartCaptionConfig {
    SmartCaptionConfig {
        canvas_w: config.canvas_w,
        canvas_h: config.canvas_h,
        caption_w: config.caption_w,
        caption_h: config.caption_h,
        safe_top: config.safe_top,
        safe_bottom: config.safe_bottom,
        safe_left: config.safe_left,
        safe_right: config.safe_right,
        sample_rate: config.sample_rate,
        continuous: config.continuous,
    }
}
