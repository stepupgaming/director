//! Elements for the box, text, image, video, vector, shape, composition,
//! Lottie and spectrum visualizer bodies.
//!
//! Each `*_node` builder reads one body without a node record, so frontends
//! can rebuild the content they nest inside their own elements.

use super::{animated, color, color_animated, gradient, ImportContext, ImportError};
use crate::element::{Element, TextFit, TextShadow, TextSpan};
use crate::node::{
    text::CounterConfig, BoxNode, BoxShadow, CompositionNode, ImageNode, LottieNode, PathCommand, ShapeNode, ShapeType,
    SpectrumVisualizerNode, TextAnimProperty, TextAnimator, TextNode, VectorNode, VideoNode, VideoSource,
};
use crate::text_direction::TextDirectionMode;
use crate::types::{ObjectFit, ObjectPosition};
use director_scene as doc;
use director_scene::{Channel, NodeBody, NodeRecord};
use skia_safe::textlayout::TextAlign;

pub(crate) fn mismatch(record: &NodeRecord) -> ImportError {
    ImportError::Invalid(format!("a `{}` record holds a different body", record.kind))
}

pub(crate) fn decode_box(record: &NodeRecord, _cx: &mut ImportContext) -> Result<Box<dyn Element>, ImportError> {
    let NodeBody::Box(body) = &record.body else {
        return Err(mismatch(record));
    };
    Ok(Box::new(box_node(body, &record.opacity)?))
}

pub(crate) fn decode_text(record: &NodeRecord, cx: &mut ImportContext) -> Result<Box<dyn Element>, ImportError> {
    let NodeBody::Text(body) = &record.body else {
        return Err(mismatch(record));
    };
    Ok(Box::new(text_node(body, &record.opacity, cx)?))
}

pub(crate) fn decode_image(record: &NodeRecord, cx: &mut ImportContext) -> Result<Box<dyn Element>, ImportError> {
    let NodeBody::Image(body) = &record.body else {
        return Err(mismatch(record));
    };
    Ok(Box::new(image_node(body, &record.opacity, cx)?))
}

pub(crate) fn decode_vector(record: &NodeRecord, cx: &mut ImportContext) -> Result<Box<dyn Element>, ImportError> {
    let NodeBody::Vector(body) = &record.body else {
        return Err(mismatch(record));
    };
    Ok(Box::new(vector_node(body, &record.opacity, cx)?))
}

pub(crate) fn decode_shape(record: &NodeRecord, _cx: &mut ImportContext) -> Result<Box<dyn Element>, ImportError> {
    let NodeBody::Shape(body) = &record.body else {
        return Err(mismatch(record));
    };
    Ok(Box::new(shape_node(body, &record.opacity)?))
}

pub(crate) fn decode_composition(
    record: &NodeRecord,
    cx: &mut ImportContext,
) -> Result<Box<dyn Element>, ImportError> {
    let NodeBody::Composition(body) = &record.body else {
        return Err(mismatch(record));
    };
    let mut node = CompositionNode::new(cx.nested(&body.document)?);
    node.start_offset = body.start_offset;
    Ok(Box::new(node))
}

/// A box from its body.
pub fn box_node(body: &doc::BoxBody, opacity: &Channel) -> Result<BoxNode, ImportError> {
    let mut node = BoxNode::new();
    node.opacity = animated(opacity)?;
    node.bg_color = body.background.as_ref().map(color_animated).transpose()?;
    node.bg_gradient = body.background_gradient.as_ref().map(gradient);
    node.blur = animated(&body.blur)?;
    node.shadow_color = body.shadow_color.as_ref().map(color_animated).transpose()?;
    node.shadow_blur = animated(&body.shadow_blur)?;
    node.shadow_offset_x = animated(&body.shadow_offset_x)?;
    node.shadow_offset_y = animated(&body.shadow_offset_y)?;
    node.border_radius = animated(&body.border_radius)?;
    node.border_width = animated(&body.border_width)?;
    node.border_color = body.border_color.as_ref().map(color_animated).transpose()?;
    node.overflow = body.overflow.clone();
    node.backdrop_blur = animated(&body.backdrop_blur)?;
    node.extra_shadows = body
        .extra_shadows
        .iter()
        .map(|shadow| {
            Ok(BoxShadow {
                color: color_animated(&shadow.color)?,
                blur: animated(&shadow.blur)?,
                offset_x: animated(&shadow.offset_x)?,
                offset_y: animated(&shadow.offset_y)?,
            })
        })
        .collect::<Result<_, ImportError>>()?;
    node.clip_path = body
        .clip_path
        .as_ref()
        .map(|points| points.iter().map(|[x, y]| (*x, *y)).collect());
    Ok(node)
}

/// A text node from its body, laid out with the context's fonts.
pub fn text_node(body: &doc::TextBody, opacity: &Channel, cx: &ImportContext) -> Result<TextNode, ImportError> {
    let spans = body
        .spans
        .iter()
        .map(|span| TextSpan {
            text: span.text.clone(),
            color: span.color.map(color),
            font_family: span.font_family.clone(),
            font_weight: span.font_weight,
            font_style: span.font_style.clone(),
            font_size: span.font_size,
            background_color: span.background_color.map(color),
            background_padding: span.background_padding,
            stroke_width: span.stroke_width,
            stroke_color: span.stroke_color.map(color),
            fill_gradient: span.fill_gradient.as_ref().map(gradient),
            letter_spacing: span.letter_spacing,
            word_spacing: span.word_spacing,
            line_height: span.line_height,
            font_variations: span.font_variations.clone(),
        })
        .collect();
    let mut node = TextNode::new(spans, cx.assets.font_collection.clone());
    node.opacity = animated(opacity)?;
    node.default_font_size = animated(&body.font_size)?;
    node.default_color = color_animated(&body.color)?;
    node.fit_mode = match body.fit {
        doc::TextFit::None => TextFit::None,
        doc::TextFit::Shrink => TextFit::Shrink,
    };
    node.min_size = body.min_size;
    node.max_size = body.max_size;
    node.shadows = body.shadows.iter().map(text_shadow).collect();
    node.text_align = text_align(body.align);
    node.text_direction = match body.direction {
        doc::TextDirection::Auto => TextDirectionMode::Auto,
        doc::TextDirection::Ltr => TextDirectionMode::Ltr,
        doc::TextDirection::Rtl => TextDirectionMode::Rtl,
    };
    node.stroke_over_fill = body.stroke_over_fill;
    node.animators = body
        .animators
        .iter()
        .map(|animator| {
            Ok(TextAnimator {
                start_idx: animator.start,
                end_idx: animator.end,
                property: match animator.property {
                    doc::GlyphProperty::Opacity => TextAnimProperty::Opacity,
                    doc::GlyphProperty::OffsetX => TextAnimProperty::OffsetX,
                    doc::GlyphProperty::OffsetY => TextAnimProperty::OffsetY,
                    doc::GlyphProperty::Scale => TextAnimProperty::Scale,
                    doc::GlyphProperty::Rotation => TextAnimProperty::Rotation,
                },
                animation: animated(&animator.animation)?,
                stagger: animator.stagger,
                duration: animator.duration,
            })
        })
        .collect::<Result<_, ImportError>>()?;
    node.counter = body.counter.as_ref().map(|counter| CounterConfig {
        decimals: counter.decimals,
        prefix: counter.prefix.clone(),
        suffix: counter.suffix.clone(),
        thousands_sep: counter.thousands_separator,
    });
    node.counter_value = animated(&body.counter_value)?;
    node.init_paragraph();
    Ok(node)
}

/// A text shadow.
pub fn text_shadow(shadow: &doc::TextShadow) -> TextShadow {
    TextShadow {
        color: color(shadow.color),
        blur: shadow.blur,
        offset: (shadow.offset[0], shadow.offset[1]),
    }
}

/// A paragraph alignment.
pub fn text_align(align: doc::TextAlign) -> TextAlign {
    match align {
        doc::TextAlign::Left => TextAlign::Left,
        doc::TextAlign::Right => TextAlign::Right,
        doc::TextAlign::Center => TextAlign::Center,
        doc::TextAlign::Justify => TextAlign::Justify,
        doc::TextAlign::Start => TextAlign::Start,
        doc::TextAlign::End => TextAlign::End,
    }
}

/// An `object-fit`.
pub fn object_fit(fit: doc::ObjectFit) -> ObjectFit {
    match fit {
        doc::ObjectFit::Cover => ObjectFit::Cover,
        doc::ObjectFit::Contain => ObjectFit::Contain,
        doc::ObjectFit::Fill => ObjectFit::Fill,
        doc::ObjectFit::None => ObjectFit::None,
        doc::ObjectFit::ScaleDown => ObjectFit::ScaleDown,
    }
}

/// An `object-position`.
pub fn object_position(position: doc::ObjectPosition) -> ObjectPosition {
    ObjectPosition {
        x_percent: position.x_percent,
        x_offset: position.x_offset,
        y_percent: position.y_percent,
        y_offset: position.y_offset,
    }
}

/// Decode an image asset. A file asset keeps its path on the node.
pub fn image(id: &str, cx: &ImportContext) -> Result<skia_safe::Image, ImportError> {
    let bytes = cx.asset_bytes(id)?;
    skia_safe::Image::from_encoded(skia_safe::Data::new_copy(&bytes)).ok_or_else(|| ImportError::Asset {
        id: id.to_string(),
        reason: "Skia cannot decode the image".to_string(),
    })
}

/// An image node from its media body.
pub fn image_node(body: &doc::MediaBody, opacity: &Channel, cx: &ImportContext) -> Result<ImageNode, ImportError> {
    let pixels = image(&body.asset, cx)?;
    let mut node = match cx.asset_path(&body.asset) {
        Some(path) => ImageNode::from_image_with_path(Some(pixels), path),
        None => ImageNode::from_image(Some(pixels)),
    };
    node.opacity = animated(opacity)?;
    node.object_fit = object_fit(body.fit);
    node.object_position = object_position(body.position);
    node.border_radius = animated(&body.border_radius)?;
    Ok(node)
}

/// A vector node from its SVG asset.
pub fn vector_node(body: &doc::VectorBody, opacity: &Channel, cx: &ImportContext) -> Result<VectorNode, ImportError> {
    let bytes = cx.asset_bytes(&body.asset)?;
    let tree = usvg::Tree::from_data(&bytes, &usvg::Options::default()).map_err(|error| ImportError::Asset {
        id: body.asset.clone(),
        reason: format!("SVG: {error}"),
    })?;
    let mut node = VectorNode::from_tree(tree);
    node.opacity = animated(opacity)?;
    Ok(node)
}

/// A shape node from its body.
pub fn shape_node(body: &doc::ShapeBody, opacity: &Channel) -> Result<ShapeNode, ImportError> {
    let shape = match &body.geometry {
        doc::ShapeGeometry::Circle { cx, cy, r } => ShapeType::Circle { cx: *cx, cy: *cy, r: *r },
        doc::ShapeGeometry::Ellipse { cx, cy, rx, ry } => ShapeType::Ellipse {
            cx: *cx,
            cy: *cy,
            rx: *rx,
            ry: *ry,
        },
        doc::ShapeGeometry::Path { commands } => ShapeType::Path {
            commands: commands.iter().map(path_command).collect(),
        },
    };
    let mut node = ShapeNode::new(shape);
    node.opacity = animated(opacity)?;
    node.fill_color = body.fill.as_ref().map(color_animated).transpose()?;
    node.fill_gradient = body.fill_gradient.as_ref().map(gradient);
    node.stroke_color = body.stroke.as_ref().map(color_animated).transpose()?;
    node.stroke_width = animated(&body.stroke_width)?;
    Ok(node)
}

fn path_command(command: &doc::PathCommand) -> PathCommand {
    match *command {
        doc::PathCommand::MoveTo { x, y } => PathCommand::MoveTo { x, y },
        doc::PathCommand::LineTo { x, y } => PathCommand::LineTo { x, y },
        doc::PathCommand::QuadTo { cx, cy, x, y } => PathCommand::QuadTo { cx, cy, x, y },
        doc::PathCommand::CubicTo { c1x, c1y, c2x, c2y, x, y } => PathCommand::CubicTo { c1x, c1y, c2x, c2y, x, y },
        doc::PathCommand::ArcTo {
            cx,
            cy,
            rx,
            ry,
            start_angle,
            sweep_angle,
        } => PathCommand::ArcTo {
            cx,
            cy,
            rx,
            ry,
            start_angle,
            sweep_angle,
        },
        doc::PathCommand::Close => PathCommand::Close,
    }
}

pub(crate) fn decode_video(record: &NodeRecord, cx: &mut ImportContext) -> Result<Box<dyn Element>, ImportError> {
    let NodeBody::Video(body) = &record.body else {
        return Err(mismatch(record));
    };
    Ok(Box::new(video_node(body, &record.opacity, cx)?))
}

/// A video node from its media body. File assets decode in place through
/// `AssetLoader::media_path`; embedded bytes go through a temporary file.
pub fn video_node(body: &doc::MediaBody, opacity: &Channel, cx: &ImportContext) -> Result<VideoNode, ImportError> {
    let path = cx.asset_path(&body.asset);
    let source = match path.and_then(|path| cx.assets.loader.media_path(path)) {
        Some(file) => VideoSource::Path(file),
        None => VideoSource::Bytes(cx.asset_bytes(&body.asset)?),
    };
    let mut node = VideoNode::new(source, cx.render_mode);
    if let Some(path) = path {
        node = node.with_source_path(path);
    }
    node.opacity = animated(opacity)?;
    node.object_fit = object_fit(body.fit);
    node.object_position = object_position(body.position);
    node.border_radius = animated(&body.border_radius)?;
    Ok(node)
}

pub(crate) fn decode_lottie(record: &NodeRecord, cx: &mut ImportContext) -> Result<Box<dyn Element>, ImportError> {
    let NodeBody::Lottie(body) = &record.body else {
        return Err(mismatch(record));
    };
    let images = body
        .images
        .iter()
        .map(|(id, asset)| Ok((id.clone(), image(asset, cx)?)))
        .collect::<Result<_, ImportError>>()?;
    let mut node = LottieNode::from_json(body.animation.clone(), images, &cx.assets)
        .map_err(|error| ImportError::Invalid(format!("lottie animation: {error}")))?;
    node.opacity = animated(&record.opacity)?;
    node.frame = animated(&body.frame)?;
    node.speed = body.speed;
    node.loop_anim = body.loop_animation;
    Ok(Box::new(node))
}

pub(crate) fn decode_spectrum(record: &NodeRecord, _cx: &mut ImportContext) -> Result<Box<dyn Element>, ImportError> {
    let NodeBody::SpectrumVisualizer(body) = &record.body else {
        return Err(mismatch(record));
    };
    let mut node = SpectrumVisualizerNode::new(
        body.track,
        body.bar_count,
        color(body.bar_color),
        body.bar_gap,
        body.smoothing,
        body.position.clone(),
        body.min_height,
        body.max_height,
        body.border_radius,
    );
    node.opacity = animated(&record.opacity)?;
    Ok(Box::new(node))
}
