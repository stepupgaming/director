//! Compiled HTML boxes from their scene document form.
//!
//! [`register_import`] adds the HTML kinds to an [`ImportRegistry`]. The `html`
//! decoder inverts `export::html_body`: it rebuilds the [`PaintStyle`], the
//! content element, the CSS animations and the Paper shader.
//! The `html_video` and `html_capture` decoders live in `capture.rs` and the
//! `svg_smil` decoder in `smil.rs`, beside their elements.

use crate::animation::Animation;
use crate::background_box::AxisRepeat;
use crate::box_edges::BoxEdge;
use crate::element::{AtomicSizing, HtmlElement, InlineFlow, InlineLink};
use crate::gradient::NativeGradient;
use crate::shader::PaperPaint;
use crate::style::{
    compile_filters, AxisPos, BackgroundFit, BackgroundLayer, BackgroundOrigin, BorderLine, FilterFn, Metrics,
    OverflowWrap, PaintStyle, Shadow, TextLines, TextOverflow, TextTransform,
};
use director_core::element::Element;
use director_core::node::TextNode;
use director_core::scene_import::{blend_mode, color, length, nodes, ImportContext, ImportError, ImportRegistry};
use director_scene::html as doc;
use director_scene::layout::Length;
use director_scene::{Channel, NodeBody, NodeRecord};
use lightningcss::properties::transform::TransformList;
use lightningcss::traits::Parse;
use taffy::LengthPercentage;

/// Register the HTML kinds with `registry`.
pub fn register_import(registry: &mut ImportRegistry) {
    registry.register("html", decode_html);
    registry.register("html_video", crate::capture::decode_video);
    registry.register("html_capture", crate::capture::decode_capture);
    registry.register("svg_smil", crate::smil::decode);
}

pub(crate) fn mismatch(record: &NodeRecord) -> ImportError {
    ImportError::Invalid(format!("a `{}` record holds a different body", record.kind))
}

fn decode_html(record: &NodeRecord, cx: &mut ImportContext) -> Result<Box<dyn Element>, ImportError> {
    let NodeBody::Html(body) = &record.body else {
        return Err(mismatch(record));
    };
    let mut paint = paint(&body.paint, cx)?;
    paint.opacity = match record.opacity.keyframes.as_slice() {
        [still] => still.value,
        _ => return Err(ImportError::Invalid("html opacity animates through CSS animations".to_string())),
    };
    if let Some(doc::InlineFlow::Text { boxes, .. }) = &body.inline {
        paint.placeholders = boxes.clone();
    }
    let content = match &body.content {
        None => None,
        Some(content) => Some(match &content.body {
            NodeBody::Text(text) => {
                let mut node: TextNode = nodes::text_node(text, &content.opacity, cx)?;
                crate::compiler::prepare_paragraph(&mut node, &paint);
                Box::new(node) as Box<dyn Element>
            }
            NodeBody::HtmlVideo(video) => {
                let timing = format!(
                    "{}|{:?}|{}|{}|{}",
                    body.start, body.end, body.media_start, body.playback_rate, body.hidden
                );
                crate::capture::decode_box_video(video, &content.opacity, timing, cx)?
            }
            other => cx.element(other, &content.opacity)?,
        }),
    };
    let metrics = Metrics {
        width: body.metrics.width,
        height: body.metrics.height,
        font_size: body.metrics.font_size,
    };
    let mut element = HtmlElement::new(taffy::Style::default(), paint, content, metrics);
    element.animations = body.animations.iter().map(Animation::from_doc).collect::<Result<_, _>>()?;
    element.start = body.start;
    element.end = body.end.unwrap_or(f64::INFINITY);
    element.media_start = body.media_start;
    element.playback_rate = body.playback_rate;
    element.hidden = body.hidden;
    element.paper = body.paper.as_ref().map(|paper| PaperPaint::from_doc(paper, cx)).transpose()?;
    element.time_map = body.time_map.as_deref().map(crate::animation::TimeMap::from_doc).transpose()?;
    // The text and its boxes share one flow.
    let mut flow = |id: u32| cx.shared(format!("html-inline-flow-{id}"), |_| Ok(InlineFlow::new(id)));
    element.inline = match &body.inline {
        None => None,
        Some(doc::InlineFlow::Text { flow: id, .. }) => Some(InlineLink::Text(flow(*id)?)),
        Some(doc::InlineFlow::Box { flow: id, slot, auto_width, auto_height, content_box }) => Some(InlineLink::Box(
            flow(*id)?,
            *slot as usize,
            AtomicSizing { auto_width: *auto_width, auto_height: *auto_height, content_box: *content_box },
        )),
    };
    Ok(Box::new(element))
}

fn paint(p: &doc::HtmlPaint, cx: &ImportContext) -> Result<PaintStyle, ImportError> {
    let filters: Vec<FilterFn> = p.filters.iter().map(filter).collect();
    Ok(PaintStyle {
        box_node: nodes::box_node(&p.box_paint, &Channel::still(1.0))?,
        opacity: 1.0,
        visible: p.visible,
        transform: TransformList::parse_string(&p.transform)
            .map_err(|e| ImportError::Invalid(format!("transform `{}`: {e:?}", p.transform)))?,
        origin: pair(p.origin),
        origin_z: p.origin_z,
        color: color(p.color),
        font_size: p.font_size,
        font_family: p.font_family.clone(),
        font_weight: p.font_weight,
        font_style: p.font_style.clone(),
        letter_spacing: p.letter_spacing,
        word_spacing: p.word_spacing,
        font_stretch: p.font_stretch,
        font_variations: p.font_variations.clone(),
        text_indent: length(p.text_indent),
        tabular_nums: p.tabular_nums,
        line_height: p.line_height,
        align: nodes::text_align(p.align),
        preserve_space: p.preserve_space,
        no_wrap: p.no_wrap,
        pre_line: p.pre_line,
        overflow_wrap: match p.overflow_wrap {
            doc::OverflowWrap::Normal => OverflowWrap::Normal,
            doc::OverflowWrap::BreakWord => OverflowWrap::BreakWord,
            doc::OverflowWrap::Anywhere => OverflowWrap::Anywhere,
        },
        break_word: p.break_word,
        break_all: p.break_all,
        text_overflow: match p.text_overflow {
            doc::TextOverflow::Clip => TextOverflow::Clip,
            doc::TextOverflow::Ellipsis => TextOverflow::Ellipsis,
        },
        line_clamp: p.line_clamp,
        text_transform: match p.text_transform {
            doc::TextTransform::None => TextTransform::None,
            doc::TextTransform::Uppercase => TextTransform::Uppercase,
            doc::TextTransform::Lowercase => TextTransform::Lowercase,
            doc::TextTransform::Capitalize => TextTransform::Capitalize,
        },
        rtl: p.rtl,
        z_index: p.z_index,
        layers: p
            .layers
            .iter()
            .map(|layer| {
                Ok(match layer {
                    doc::BackgroundLayer::Gradient(gradient) => {
                        BackgroundLayer::Gradient(NativeGradient::from_doc(gradient)?)
                    }
                    doc::BackgroundLayer::Image { asset } => BackgroundLayer::Image(nodes::image(asset, cx)?),
                })
            })
            .collect::<Result<_, ImportError>>()?,
        background_position_x: axis_positions(&p.background_position_x),
        background_position_y: axis_positions(&p.background_position_y),
        background_size: p
            .background_size
            .iter()
            .map(|fit| match fit {
                doc::BackgroundSize::Auto => BackgroundFit::Auto,
                doc::BackgroundSize::Cover => BackgroundFit::Cover,
                doc::BackgroundSize::Contain => BackgroundFit::Contain,
                doc::BackgroundSize::Lengths { width, height } => {
                    BackgroundFit::Lengths(width.map(length), height.map(length))
                }
            })
            .collect(),
        background_repeat: p.background_repeat.iter().map(|[x, y]| (repeat(*x), repeat(*y))).collect(),
        background_origin: match p.background_origin {
            doc::BackgroundOrigin::PaddingBox => BackgroundOrigin::PaddingBox,
            doc::BackgroundOrigin::BorderBox => BackgroundOrigin::BorderBox,
            doc::BackgroundOrigin::ContentBox => BackgroundOrigin::ContentBox,
        },
        background_clip: match p.background_clip {
            doc::BackgroundClip::BorderBox => BoxEdge::BorderBox,
            doc::BackgroundClip::PaddingBox => BoxEdge::PaddingBox,
            doc::BackgroundClip::ContentBox => BoxEdge::ContentBox,
            doc::BackgroundClip::Text => BoxEdge::Text,
        },
        mask: p.mask.as_ref().map(NativeGradient::from_doc).transpose()?,
        mask_luminance: p.mask_luminance,
        decoration: TextLines {
            underline: p.decoration.underline,
            overline: p.decoration.overline,
            line_through: p.decoration.line_through,
        },
        decoration_style: {
            use crate::decoration_marks::DecorationStyle as S;
            match p.decoration_style {
                doc::DecorationStyle::Solid => S::Solid,
                doc::DecorationStyle::Double => S::Double,
                doc::DecorationStyle::Dotted => S::Dotted,
                doc::DecorationStyle::Dashed => S::Dashed,
                doc::DecorationStyle::Wavy => S::Wavy,
            }
        },
        decoration_thickness: {
            use crate::decoration_value::DecorationThickness as T;
            match p.decoration_thickness {
                doc::DecorationThickness::Auto => T::Auto,
                doc::DecorationThickness::Px(px) => T::Px(px),
                doc::DecorationThickness::FromFont => T::FromFont,
            }
        },
        decoration_color: p.decoration_color.map(color),
        text_underline_offset: p.text_underline_offset,
        object_position: nodes::object_position(p.object_position),
        corners: p.corners.map(pair),
        border_colors: p.border_colors.map(color),
        border_lines: p.border_lines.map(border_line),
        perspective: p.perspective,
        perspective_origin: pair(p.perspective_origin),
        backface_hidden: p.backface_hidden,
        preserve_3d: p.preserve_3d,
        filter: compile_filters(&filters),
        filters,
        blend_mode: blend_mode(&p.blend_mode)?,
        shadows: p
            .shadows
            .iter()
            .map(|shadow| Shadow {
                inset: shadow.inset,
                color: color(shadow.color),
                x: shadow.x,
                y: shadow.y,
                blur: shadow.blur,
                spread: shadow.spread,
            })
            .collect(),
        text_shadows: p.text_shadows.iter().map(nodes::text_shadow).collect(),
        text_stroke_width: p.text_stroke_width,
        text_stroke_color: p.text_stroke_color.map(color),
        stroke_over_fill: p.stroke_over_fill,
        text: None,
        placeholders: Vec::new(),
    })
}

fn pair(value: [Length; 2]) -> (LengthPercentage, LengthPercentage) {
    (length(value[0]), length(value[1]))
}

fn axis_positions(list: &[doc::AxisPosition]) -> Vec<AxisPos> {
    list.iter()
        .map(|axis| AxisPos {
            percent: axis.percent,
            length: axis.length,
        })
        .collect()
}

fn repeat(value: doc::BackgroundRepeat) -> AxisRepeat {
    match value {
        doc::BackgroundRepeat::Repeat => AxisRepeat::Repeat,
        doc::BackgroundRepeat::NoRepeat => AxisRepeat::NoRepeat,
        doc::BackgroundRepeat::Space => AxisRepeat::Space,
        doc::BackgroundRepeat::Round => AxisRepeat::Round,
    }
}

fn border_line(value: doc::BorderLine) -> BorderLine {
    match value {
        doc::BorderLine::Solid => BorderLine::Solid,
        doc::BorderLine::Dashed => BorderLine::Dashed,
        doc::BorderLine::Dotted => BorderLine::Dotted,
        doc::BorderLine::Double => BorderLine::Double,
        doc::BorderLine::Groove => BorderLine::Groove,
        doc::BorderLine::Ridge => BorderLine::Ridge,
        doc::BorderLine::Inset => BorderLine::Inset,
        doc::BorderLine::Outset => BorderLine::Outset,
    }
}

fn filter(value: &doc::CssFilter) -> FilterFn {
    match *value {
        doc::CssFilter::Blur { radius } => FilterFn::Blur(radius),
        doc::CssFilter::Brightness { amount } => FilterFn::Brightness(amount),
        doc::CssFilter::Contrast { amount } => FilterFn::Contrast(amount),
        doc::CssFilter::Saturate { amount } => FilterFn::Saturate(amount),
        doc::CssFilter::Grayscale { amount } => FilterFn::Grayscale(amount),
        doc::CssFilter::HueRotate { degrees } => FilterFn::HueRotate(degrees),
        doc::CssFilter::DropShadow { x, y, blur, color: c } => FilterFn::DropShadow {
            x,
            y,
            blur,
            color: color(c),
        },
        doc::CssFilter::Invert { amount } => FilterFn::Invert(amount),
        doc::CssFilter::Sepia { amount } => FilterFn::Sepia(amount),
        doc::CssFilter::Opacity { amount } => FilterFn::Opacity(amount),
    }
}
