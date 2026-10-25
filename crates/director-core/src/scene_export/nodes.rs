//! Bodies for the box, text, media, shape, vector, Lottie, composition and
//! spectrum elements.

use super::{channel, color_channel, export_scene, gradient, rgba, ElementExport, ExportContext, ExportError};
use crate::element::{TextFit, TextShadow};
use crate::node::{
    BoxNode, CompositionNode, ImageNode, LottieNode, PathCommand, ShapeNode, ShapeType,
    SpectrumVisualizerNode, TextAnimProperty, TextNode, VectorNode, VideoNode,
};
use crate::text_direction::TextDirectionMode;
use crate::types::{ObjectFit, ObjectPosition};
use director_scene as doc;
use director_scene::{Channel, NodeBody};
use skia_safe::textlayout::TextAlign;

pub fn box_body(node: &BoxNode, _cx: &mut ExportContext) -> Result<ElementExport, ExportError> {
    Ok(ElementExport {
        opacity: channel(&node.opacity),
        body: NodeBody::Box(doc::BoxBody {
            background: node.bg_color.as_ref().map(color_channel),
            background_gradient: node.bg_gradient.as_ref().map(gradient),
            blur: channel(&node.blur),
            shadow_color: node.shadow_color.as_ref().map(color_channel),
            shadow_blur: channel(&node.shadow_blur),
            shadow_offset_x: channel(&node.shadow_offset_x),
            shadow_offset_y: channel(&node.shadow_offset_y),
            border_radius: channel(&node.border_radius),
            border_width: channel(&node.border_width),
            border_color: node.border_color.as_ref().map(color_channel),
            overflow: node.overflow.clone(),
            backdrop_blur: channel(&node.backdrop_blur),
            extra_shadows: node
                .extra_shadows
                .iter()
                .map(|shadow| doc::BoxShadow {
                    color: color_channel(&shadow.color),
                    blur: channel(&shadow.blur),
                    offset_x: channel(&shadow.offset_x),
                    offset_y: channel(&shadow.offset_y),
                })
                .collect(),
            clip_path: node
                .clip_path
                .as_ref()
                .map(|points| points.iter().map(|(x, y)| [*x, *y]).collect()),
        }),
    })
}

pub(crate) fn text_body(node: &TextNode, _cx: &mut ExportContext) -> Result<ElementExport, ExportError> {
    if node.stroke_paragraph.lock().map(|p| p.is_some()).unwrap_or(true) {
        return Err(ExportError::Unrepresentable(
            "text with a prebuilt stroke paragraph".to_string(),
        ));
    }
    Ok(text_export(node))
}

/// The spans and channels of a text node. The caller owns any prebuilt stroke
/// paragraph, which this leaves out.
pub fn text_export(node: &TextNode) -> ElementExport {
    ElementExport {
        opacity: channel(&node.opacity),
        body: NodeBody::Text(doc::TextBody {
            spans: node
                .spans
                .iter()
                .map(|span| doc::TextSpan {
                    text: span.text.clone(),
                    color: span.color.map(rgba),
                    font_family: span.font_family.clone(),
                    font_weight: span.font_weight,
                    font_style: span.font_style.clone(),
                    font_size: span.font_size,
                    background_color: span.background_color.map(rgba),
                    background_padding: span.background_padding,
                    stroke_width: span.stroke_width,
                    stroke_color: span.stroke_color.map(rgba),
                    fill_gradient: span.fill_gradient.as_ref().map(gradient),
                    letter_spacing: span.letter_spacing,
                    line_height: span.line_height,
                })
                .collect(),
            font_size: channel(&node.default_font_size),
            color: color_channel(&node.default_color),
            fit: match node.fit_mode {
                TextFit::None => doc::TextFit::None,
                TextFit::Shrink => doc::TextFit::Shrink,
            },
            min_size: node.min_size,
            max_size: node.max_size,
            shadows: node.shadows.iter().map(text_shadow).collect(),
            align: text_align(node.text_align),
            direction: match node.text_direction {
                TextDirectionMode::Auto => doc::TextDirection::Auto,
                TextDirectionMode::Ltr => doc::TextDirection::Ltr,
                TextDirectionMode::Rtl => doc::TextDirection::Rtl,
            },
            stroke_over_fill: node.stroke_over_fill,
            animators: node
                .animators
                .iter()
                .map(|animator| doc::TextAnimator {
                    start: animator.start_idx,
                    end: animator.end_idx,
                    property: match animator.property {
                        TextAnimProperty::Opacity => doc::GlyphProperty::Opacity,
                        TextAnimProperty::OffsetX => doc::GlyphProperty::OffsetX,
                        TextAnimProperty::OffsetY => doc::GlyphProperty::OffsetY,
                        TextAnimProperty::Scale => doc::GlyphProperty::Scale,
                        TextAnimProperty::Rotation => doc::GlyphProperty::Rotation,
                    },
                    animation: channel(&animator.animation),
                    stagger: animator.stagger,
                    duration: animator.duration,
                })
                .collect(),
            counter: node.counter.as_ref().map(|counter| doc::Counter {
                decimals: counter.decimals,
                prefix: counter.prefix.clone(),
                suffix: counter.suffix.clone(),
                thousands_separator: counter.thousands_sep,
            }),
            counter_value: channel(&node.counter_value),
        }),
    }
}

pub(crate) fn image_body(node: &ImageNode, cx: &mut ExportContext) -> Result<ElementExport, ExportError> {
    let asset = match (&node.source_path, &node.image) {
        (Some(path), _) => cx.path_asset("image", "image/*", path),
        (None, Some(image)) => image_asset(image, cx)?,
        (None, None) => {
            return Err(ExportError::Unrepresentable(
                "image node with neither pixels nor a source path".to_string(),
            ))
        }
    };
    Ok(ElementExport {
        opacity: channel(&node.opacity),
        body: NodeBody::Image(media(asset, node.object_fit, node.object_position, &node.border_radius)),
    })
}

/// Embed an image as its encoded bytes, or as PNG when it has none.
pub fn image_asset(image: &skia_safe::Image, cx: &mut ExportContext) -> Result<doc::AssetId, ExportError> {
    if let Some(data) = image.encoded_data() {
        return Ok(cx.embedded_asset("image", "image/*", data.as_bytes()));
    }
    let data = image
        .encode(None, skia_safe::EncodedImageFormat::PNG, 100)
        .ok_or_else(|| ExportError::Unrepresentable("image that Skia cannot encode as PNG".to_string()))?;
    Ok(cx.embedded_asset("image", "image/png", data.as_bytes()))
}

pub(crate) fn video_body(node: &VideoNode, cx: &mut ExportContext) -> Result<ElementExport, ExportError> {
    Ok(ElementExport {
        opacity: channel(&node.opacity),
        body: NodeBody::Video(video_media(node, cx)?),
    })
}

/// The media fields of a video, registering its file in the asset table.
pub fn video_media(node: &VideoNode, cx: &mut ExportContext) -> Result<doc::MediaBody, ExportError> {
    let asset = match &node.source_path {
        Some(path) => cx.path_asset("video", "video/*", path),
        None => {
            let path = node.path.display().to_string();
            let bytes = std::fs::read(&node.path).map_err(|source| ExportError::Io {
                path: path.clone(),
                source,
            })?;
            cx.embedded_asset("video", "video/*", &bytes)
        }
    };
    Ok(media(asset, node.object_fit, node.object_position, &node.border_radius))
}

/// A text shadow.
pub fn text_shadow(shadow: &TextShadow) -> doc::TextShadow {
    doc::TextShadow {
        color: rgba(shadow.color),
        blur: shadow.blur,
        offset: [shadow.offset.0, shadow.offset.1],
    }
}

/// A paragraph alignment.
pub fn text_align(align: TextAlign) -> doc::TextAlign {
    match align {
        TextAlign::Left => doc::TextAlign::Left,
        TextAlign::Right => doc::TextAlign::Right,
        TextAlign::Center => doc::TextAlign::Center,
        TextAlign::Justify => doc::TextAlign::Justify,
        TextAlign::Start => doc::TextAlign::Start,
        TextAlign::End => doc::TextAlign::End,
    }
}

/// An `object-position`.
pub fn object_position(position: ObjectPosition) -> doc::ObjectPosition {
    doc::ObjectPosition {
        x_percent: position.x_percent,
        x_offset: position.x_offset,
        y_percent: position.y_percent,
        y_offset: position.y_offset,
    }
}

/// An `object-fit`.
pub fn object_fit(fit: ObjectFit) -> doc::ObjectFit {
    match fit {
        ObjectFit::Cover => doc::ObjectFit::Cover,
        ObjectFit::Contain => doc::ObjectFit::Contain,
        ObjectFit::Fill => doc::ObjectFit::Fill,
        ObjectFit::None => doc::ObjectFit::None,
        ObjectFit::ScaleDown => doc::ObjectFit::ScaleDown,
    }
}

fn media(
    asset: doc::AssetId,
    fit: ObjectFit,
    position: ObjectPosition,
    border_radius: &crate::animation::Animated<f32>,
) -> doc::MediaBody {
    doc::MediaBody {
        asset,
        fit: object_fit(fit),
        position: object_position(position),
        border_radius: channel(border_radius),
    }
}

pub(crate) fn shape_body(node: &ShapeNode, _cx: &mut ExportContext) -> Result<ElementExport, ExportError> {
    let geometry = match &node.shape {
        ShapeType::Circle { cx, cy, r } => doc::ShapeGeometry::Circle { cx: *cx, cy: *cy, r: *r },
        ShapeType::Ellipse { cx, cy, rx, ry } => doc::ShapeGeometry::Ellipse {
            cx: *cx,
            cy: *cy,
            rx: *rx,
            ry: *ry,
        },
        ShapeType::Path { commands } => doc::ShapeGeometry::Path {
            commands: commands.iter().map(path_command).collect(),
        },
    };
    Ok(ElementExport {
        opacity: channel(&node.opacity),
        body: NodeBody::Shape(doc::ShapeBody {
            geometry,
            fill: node.fill_color.as_ref().map(color_channel),
            fill_gradient: node.fill_gradient.as_ref().map(gradient),
            stroke: node.stroke_color.as_ref().map(color_channel),
            stroke_width: channel(&node.stroke_width),
        }),
    })
}

fn path_command(command: &PathCommand) -> doc::PathCommand {
    match *command {
        PathCommand::MoveTo { x, y } => doc::PathCommand::MoveTo { x, y },
        PathCommand::LineTo { x, y } => doc::PathCommand::LineTo { x, y },
        PathCommand::QuadTo { cx, cy, x, y } => doc::PathCommand::QuadTo { cx, cy, x, y },
        PathCommand::CubicTo { c1x, c1y, c2x, c2y, x, y } => doc::PathCommand::CubicTo { c1x, c1y, c2x, c2y, x, y },
        PathCommand::ArcTo {
            cx,
            cy,
            rx,
            ry,
            start_angle,
            sweep_angle,
        } => doc::PathCommand::ArcTo {
            cx,
            cy,
            rx,
            ry,
            start_angle,
            sweep_angle,
        },
        PathCommand::Close => doc::PathCommand::Close,
    }
}

pub(crate) fn vector_body(node: &VectorNode, cx: &mut ExportContext) -> Result<ElementExport, ExportError> {
    let svg = node.tree().to_string(&usvg::WriteOptions::default());
    Ok(ElementExport {
        opacity: channel(&node.opacity),
        body: NodeBody::Vector(doc::VectorBody {
            asset: cx.embedded_asset("vector", "image/svg+xml", svg.as_bytes()),
        }),
    })
}

pub(crate) fn lottie_body(node: &LottieNode, cx: &mut ExportContext) -> Result<ElementExport, ExportError> {
    Ok(ElementExport {
        opacity: channel(&node.opacity),
        body: NodeBody::Lottie(doc::LottieBody {
            animation: node.source().clone(),
            frame: channel(&node.frame),
            speed: node.speed,
            loop_animation: node.loop_anim,
            images: node
                .asset_manager
                .images
                .iter()
                .map(|(id, image)| Ok((id.clone(), image_asset(image, cx)?)))
                .collect::<Result<_, ExportError>>()?,
        }),
    })
}

pub(crate) fn composition_body(
    node: &CompositionNode,
    _cx: &mut ExportContext,
) -> Result<ElementExport, ExportError> {
    let inner = node
        .internal_director
        .lock()
        .map_err(|_| ExportError::Unrepresentable("poisoned composition director".to_string()))?;
    Ok(ElementExport {
        opacity: Channel::still(1.0),
        body: NodeBody::Composition(doc::CompositionBody {
            start_offset: node.start_offset,
            document: Box::new(export_scene(&inner)?),
        }),
    })
}

pub(crate) fn spectrum_body(
    node: &SpectrumVisualizerNode,
    _cx: &mut ExportContext,
) -> Result<ElementExport, ExportError> {
    Ok(ElementExport {
        opacity: channel(&node.opacity),
        body: NodeBody::SpectrumVisualizer(doc::SpectrumBody {
            track: node.track_id,
            bar_count: node.bar_count,
            bar_color: rgba(node.bar_color),
            bar_gap: node.bar_gap,
            smoothing: node.smoothing,
            position: node.position.clone(),
            min_height: node.min_height,
            max_height: node.max_height,
            border_radius: node.border_radius,
        }),
    })
}
