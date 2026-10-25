//! Caption, smart caption and active speaker camera nodes.

use super::{channel, rgba, ElementExport, ExportContext, ExportError};
use crate::node::active_speaker_camera::CameraTrajectory;
use crate::node::caption::{CaptionPosition, CaptionStyle, PhraseLineMode};
use crate::node::{ActiveSpeakerCamera, CaptionNode, ContinuityGroup, Framing, SmartCaptionNode};
use crate::node::smart_caption::SmartCaptionConfig;
use crate::semantic::{Anchor, CaptionTrajectory, SemanticManifest, SolverConfig};
use crate::text_direction::TextDirectionMode;
use director_scene::caption as ds;
use director_scene::{NodeBody, TextAlign, TextDirection};
use skia_safe::textlayout::TextAlign as SkTextAlign;

pub(crate) fn caption_body(
    node: &CaptionNode,
    _cx: &mut ExportContext,
) -> Result<ElementExport, ExportError> {
    Ok(ElementExport {
        opacity: channel(&node.opacity),
        body: NodeBody::Caption(caption_fields(node)),
    })
}

pub(crate) fn smart_caption_body(
    node: &SmartCaptionNode,
    _cx: &mut ExportContext,
) -> Result<ElementExport, ExportError> {
    Ok(ElementExport {
        opacity: channel(&node.inner.opacity),
        body: NodeBody::SmartCaption(ds::SmartCaptionBody {
            caption: caption_fields(&node.inner),
            trajectory: caption_trajectory(&node.trajectory),
            canvas_w: node.canvas_w,
            canvas_h: node.canvas_h,
            continuous: node.is_continuous(),
            manifest: node.manifest.as_ref().map(manifest),
            config: node.config.as_ref().map(smart_caption_config),
        }),
    })
}

pub(crate) fn active_speaker_camera_body(
    node: &ActiveSpeakerCamera,
    cx: &mut ExportContext,
) -> Result<ElementExport, ExportError> {
    let video = super::nodes::video_media(&node.inner, cx)?;
    Ok(ElementExport {
        opacity: channel(&node.inner.opacity),
        body: NodeBody::ActiveSpeakerCamera(ds::ActiveSpeakerCameraBody {
            video,
            trajectory: camera_trajectory(node.trajectory()),
            groups: node.groups.iter().map(continuity_group).collect(),
        }),
    })
}

fn caption_fields(node: &CaptionNode) -> ds::CaptionBody {
    ds::CaptionBody {
        lines: node
            .lines
            .iter()
            .map(|line| ds::CaptionLine {
                text: line.text.clone(),
                words: line
                    .words
                    .iter()
                    .map(|word| ds::CaptionWord {
                        text: word.text.clone(),
                        start_sec: word.start_sec,
                        end_sec: word.end_sec,
                        display_line_idx: word.display_line_idx,
                    })
                    .collect(),
                start_sec: line.start_sec,
                end_sec: line.end_sec,
            })
            .collect(),
        style: caption_style(&node.style),
    }
}

fn caption_style(style: &CaptionStyle) -> ds::CaptionStyle {
    ds::CaptionStyle {
        font_size: style.font_size,
        color: rgba(style.color),
        highlight_color: rgba(style.highlight_color),
        spoken_color: rgba(style.spoken_color),
        position: caption_position(&style.position),
        text_align: match style.text_align {
            SkTextAlign::Left => TextAlign::Left,
            SkTextAlign::Right => TextAlign::Right,
            SkTextAlign::Center => TextAlign::Center,
            SkTextAlign::Justify => TextAlign::Justify,
            SkTextAlign::Start => TextAlign::Start,
            SkTextAlign::End => TextAlign::End,
        },
        left_margin: style.left_margin,
        right_margin: style.right_margin,
        text_direction: match style.text_direction {
            TextDirectionMode::Auto => TextDirection::Auto,
            TextDirectionMode::Ltr => TextDirection::Ltr,
            TextDirectionMode::Rtl => TextDirection::Rtl,
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
        text_shadow_color: rgba(style.text_shadow_color),
        text_shadow_offset_x: style.text_shadow_offset_x,
        text_shadow_offset_y: style.text_shadow_offset_y,
        text_shadow_blur: style.text_shadow_blur,
        auto_adapt_shadow: style.auto_adapt_shadow,
        stroke_enabled: style.stroke_enabled,
        stroke_width: style.stroke_width,
        stroke_color: rgba(style.stroke_color),
        glow_enabled: style.glow_enabled,
        glow_blur: style.glow_blur,
        line_reveal_offsets: style.line_reveal_offsets.clone(),
        line_reveal_durations: style.line_reveal_durations.clone(),
        pill_enabled: style.pill_enabled,
        pill_color: rgba(style.pill_color),
        pill_adaptive: style.pill_adaptive,
        pill_padding_h: style.pill_padding_h,
        pill_padding_v: style.pill_padding_v,
        pill_radius: style.pill_radius,
        auto_phrase_break: style.auto_phrase_break,
        max_words_per_line: style.max_words_per_line,
        phrase_line_mode: match style.phrase_line_mode {
            PhraseLineMode::Replace => ds::PhraseLineMode::Replace,
            PhraseLineMode::Stack => ds::PhraseLineMode::Stack,
        },
        safe_area_preset: style.safe_area_preset.clone(),
        scale_with_canvas: style.scale_with_canvas,
        reference_canvas_height: style.reference_canvas_height,
        uppercase: style.uppercase,
    }
}

fn caption_position(position: &CaptionPosition) -> ds::CaptionPosition {
    match position {
        CaptionPosition::Bottom => ds::CaptionPosition::Bottom,
        CaptionPosition::Top => ds::CaptionPosition::Top,
        CaptionPosition::Center => ds::CaptionPosition::Center,
        CaptionPosition::Left => ds::CaptionPosition::Left,
        CaptionPosition::Right => ds::CaptionPosition::Right,
        CaptionPosition::TopLeft => ds::CaptionPosition::TopLeft,
        CaptionPosition::TopRight => ds::CaptionPosition::TopRight,
        CaptionPosition::BottomLeft => ds::CaptionPosition::BottomLeft,
        CaptionPosition::BottomRight => ds::CaptionPosition::BottomRight,
        CaptionPosition::Free(y) => ds::CaptionPosition::Free { y: *y },
        CaptionPosition::FreeXY { x, y } => ds::CaptionPosition::FreeXY { x: *x, y: *y },
    }
}

fn caption_trajectory(trajectory: &CaptionTrajectory) -> ds::CaptionTrajectory {
    ds::CaptionTrajectory {
        samples: trajectory
            .as_slice()
            .iter()
            .map(|sample| ds::TrajectorySample {
                time: sample.time,
                x: sample.x,
                y: sample.y,
                anchor: sample.anchor.map(caption_anchor),
            })
            .collect(),
    }
}

fn caption_anchor(anchor: Anchor) -> ds::CaptionAnchor {
    match anchor {
        Anchor::TopLeft => ds::CaptionAnchor::TopLeft,
        Anchor::TopCenter => ds::CaptionAnchor::TopCenter,
        Anchor::TopRight => ds::CaptionAnchor::TopRight,
        Anchor::BottomLeft => ds::CaptionAnchor::BottomLeft,
        Anchor::BottomCenter => ds::CaptionAnchor::BottomCenter,
        Anchor::BottomRight => ds::CaptionAnchor::BottomRight,
        Anchor::Left => ds::CaptionAnchor::Left,
        Anchor::Right => ds::CaptionAnchor::Right,
    }
}

fn manifest(manifest: &SemanticManifest) -> ds::SemanticManifest {
    ds::SemanticManifest {
        entity_refs: manifest.entity_refs.iter().map(|id| id.to_string()).collect(),
        start_time: manifest.start_time,
        end_time: manifest.end_time,
        solver: solver_config(&manifest.solver),
    }
}

fn solver_config(solver: &SolverConfig) -> ds::SolverConfig {
    ds::SolverConfig {
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

fn smart_caption_config(config: &SmartCaptionConfig) -> ds::SmartCaptionConfig {
    ds::SmartCaptionConfig {
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

fn camera_trajectory(trajectory: &CameraTrajectory) -> ds::CameraTrajectory {
    ds::CameraTrajectory {
        samples: trajectory
            .as_slice()
            .iter()
            .map(|sample| ds::CameraSample {
                time: sample.time,
                center_x: sample.center_x,
                center_y: sample.center_y,
                height: sample.height,
            })
            .collect(),
    }
}

fn continuity_group(group: &ContinuityGroup) -> ds::ContinuityGroup {
    ds::ContinuityGroup {
        start_time: group.start_time,
        end_time: group.end_time,
        target_id: group.target_id.to_string(),
        framing: match group.framing {
            Framing::CloseUp => ds::Framing::CloseUp,
            Framing::Medium => ds::Framing::Medium,
            Framing::Wide => ds::Framing::Wide,
            Framing::Custom(multiplier) => ds::Framing::Custom { multiplier },
        },
        face_ids: group.face_ids.iter().map(|id| id.to_string()).collect(),
    }
}
