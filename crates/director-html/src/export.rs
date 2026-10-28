//! Scene document form of compiled HTML boxes.

use crate::background_box::AxisRepeat;
use crate::box_edges::BoxEdge;
use crate::element::{HtmlElement, InlineLink};
use crate::style::{
    BackgroundFit, BackgroundLayer, BackgroundOrigin, BorderLine, FilterFn, OverflowWrap,
    PaintStyle, TextOverflow, TextTransform,
};
use director_core::node::TextNode;
use director_core::scene_export::{
    blend_mode_name, export_length, nodes, rgba, ElementExport, ExportContext, ExportError,
};
use director_scene::html as doc;
use director_scene::layout::Length;
use director_scene::{Channel, NodeBody};
use lightningcss::traits::ToCss;
use taffy::LengthPercentage;

pub(crate) fn html_body(element: &HtmlElement, cx: &mut ExportContext) -> Result<ElementExport, ExportError> {
    let content = match &element.content {
        None => None,
        Some(content) => {
            // HTML text rebuilds its stroke paragraph from the box paint.
            let ElementExport { opacity, body } = match content.as_any().downcast_ref::<TextNode>() {
                Some(text) => nodes::text_export(text),
                None => content.export_body(cx)?,
            };
            Some(Box::new(doc::HtmlContent { opacity, body }))
        }
    };
    Ok(ElementExport {
        opacity: Channel::still(element.base_paint.opacity),
        body: NodeBody::Html(Box::new(doc::HtmlBody {
            paint: paint(&element.base_paint, cx)?,
            content,
            animations: element
                .animations
                .iter()
                .map(|animation| animation.to_doc())
                .collect::<Result<_, _>>()?,
            start: element.start,
            end: element.end.is_finite().then_some(element.end),
            media_start: element.media_start,
            playback_rate: element.playback_rate,
            hidden: element.hidden,
            metrics: doc::HtmlMetrics {
                width: element.metrics.width,
                height: element.metrics.height,
                font_size: element.metrics.font_size,
            },
            paper: element.paper.as_ref().map(|paper| paper.to_doc(cx)),
            time_map: element.time_map.as_ref().map(|map| map.to_doc()),
            inline: element.inline.as_ref().map(|link| match link {
                InlineLink::Text(flow) => doc::InlineFlow::Text {
                    flow: flow.id,
                    boxes: element.base_paint.placeholders.clone(),
                },
                InlineLink::Box(flow, slot, sizing) => doc::InlineFlow::Box {
                    flow: flow.id,
                    slot: *slot as u32,
                    auto_width: sizing.auto_width,
                    auto_height: sizing.auto_height,
                    content_box: sizing.content_box,
                },
            }),
        })),
    })
}

fn paint(p: &PaintStyle, cx: &mut ExportContext) -> Result<doc::HtmlPaint, ExportError> {
    let NodeBody::Box(box_paint) = nodes::box_body(&p.box_node, cx)?.body else {
        unreachable!("box_body returns a box body");
    };
    Ok(doc::HtmlPaint {
        box_paint,
        visible: p.visible,
        transform: p
            .transform
            .to_css_string(Default::default())
            .map_err(|e| ExportError::Unrepresentable(format!("transform: {e}")))?,
        origin: pair(p.origin)?,
        origin_z: p.origin_z,
        color: rgba(p.color),
        font_size: p.font_size,
        font_family: p.font_family.clone(),
        font_weight: p.font_weight,
        font_style: p.font_style.clone(),
        letter_spacing: p.letter_spacing,
        word_spacing: p.word_spacing,
        font_stretch: p.font_stretch,
        font_variations: p.font_variations.clone(),
        text_indent: export_length(p.text_indent)?,
        tabular_nums: p.tabular_nums,
        line_height: p.line_height,
        align: nodes::text_align(p.align),
        preserve_space: p.preserve_space,
        no_wrap: p.no_wrap,
        pre_line: p.pre_line,
        overflow_wrap: match p.overflow_wrap {
            OverflowWrap::Normal => doc::OverflowWrap::Normal,
            OverflowWrap::BreakWord => doc::OverflowWrap::BreakWord,
            OverflowWrap::Anywhere => doc::OverflowWrap::Anywhere,
        },
        break_word: p.break_word,
        break_all: p.break_all,
        text_overflow: match p.text_overflow {
            TextOverflow::Clip => doc::TextOverflow::Clip,
            TextOverflow::Ellipsis => doc::TextOverflow::Ellipsis,
        },
        line_clamp: p.line_clamp,
        text_transform: match p.text_transform {
            TextTransform::None => doc::TextTransform::None,
            TextTransform::Uppercase => doc::TextTransform::Uppercase,
            TextTransform::Lowercase => doc::TextTransform::Lowercase,
            TextTransform::Capitalize => doc::TextTransform::Capitalize,
        },
        rtl: p.rtl,
        z_index: p.z_index,
        layers: p
            .layers
            .iter()
            .map(|layer| {
                Ok(match layer {
                    BackgroundLayer::Gradient(gradient) => doc::BackgroundLayer::Gradient(gradient.to_doc()?),
                    BackgroundLayer::Image(image) => doc::BackgroundLayer::Image {
                        asset: nodes::image_asset(image, cx)?,
                    },
                })
            })
            .collect::<Result<_, ExportError>>()?,
        background_position_x: axis_positions(&p.background_position_x),
        background_position_y: axis_positions(&p.background_position_y),
        background_size: p
            .background_size
            .iter()
            .map(|fit| {
                Ok(match fit {
                    BackgroundFit::Auto => doc::BackgroundSize::Auto,
                    BackgroundFit::Cover => doc::BackgroundSize::Cover,
                    BackgroundFit::Contain => doc::BackgroundSize::Contain,
                    BackgroundFit::Lengths(width, height) => doc::BackgroundSize::Lengths {
                        width: width.map(export_length).transpose()?,
                        height: height.map(export_length).transpose()?,
                    },
                })
            })
            .collect::<Result<_, ExportError>>()?,
        background_repeat: p
            .background_repeat
            .iter()
            .map(|(x, y)| [repeat(*x), repeat(*y)])
            .collect(),
        background_origin: match p.background_origin {
            BackgroundOrigin::PaddingBox => doc::BackgroundOrigin::PaddingBox,
            BackgroundOrigin::BorderBox => doc::BackgroundOrigin::BorderBox,
            BackgroundOrigin::ContentBox => doc::BackgroundOrigin::ContentBox,
        },
        background_clip: match p.background_clip {
            BoxEdge::BorderBox => doc::BackgroundClip::BorderBox,
            BoxEdge::PaddingBox => doc::BackgroundClip::PaddingBox,
            BoxEdge::ContentBox => doc::BackgroundClip::ContentBox,
            BoxEdge::Text => doc::BackgroundClip::Text,
        },
        mask: p.mask.as_ref().map(|mask| mask.to_doc()).transpose()?,
        mask_luminance: p.mask_luminance,
        decoration: doc::TextLines {
            underline: p.decoration.underline,
            overline: p.decoration.overline,
            line_through: p.decoration.line_through,
        },
        decoration_style: {
            use crate::decoration_marks::DecorationStyle as S;
            match p.decoration_style {
                S::Solid => doc::DecorationStyle::Solid,
                S::Double => doc::DecorationStyle::Double,
                S::Dotted => doc::DecorationStyle::Dotted,
                S::Dashed => doc::DecorationStyle::Dashed,
                S::Wavy => doc::DecorationStyle::Wavy,
            }
        },
        decoration_thickness: {
            use crate::decoration_value::DecorationThickness as T;
            match p.decoration_thickness {
                T::Auto => doc::DecorationThickness::Auto,
                T::Px(px) => doc::DecorationThickness::Px(px),
                T::FromFont => doc::DecorationThickness::FromFont,
            }
        },
        decoration_color: p.decoration_color.map(rgba),
        text_underline_offset: p.text_underline_offset,
        object_position: nodes::object_position(p.object_position),
        corners: [
            pair(p.corners[0])?,
            pair(p.corners[1])?,
            pair(p.corners[2])?,
            pair(p.corners[3])?,
        ],
        border_colors: p.border_colors.map(rgba),
        border_lines: p.border_lines.map(border_line),
        perspective: p.perspective,
        perspective_origin: pair(p.perspective_origin)?,
        backface_hidden: p.backface_hidden,
        preserve_3d: p.preserve_3d,
        filters: p.filters.iter().map(filter).collect(),
        blend_mode: blend_mode_name(p.blend_mode).to_string(),
        shadows: p
            .shadows
            .iter()
            .map(|shadow| doc::HtmlShadow {
                inset: shadow.inset,
                color: rgba(shadow.color),
                x: shadow.x,
                y: shadow.y,
                blur: shadow.blur,
                spread: shadow.spread,
            })
            .collect(),
        text_shadows: p.text_shadows.iter().map(nodes::text_shadow).collect(),
        text_stroke_width: p.text_stroke_width,
        text_stroke_color: p.text_stroke_color.map(rgba),
        stroke_over_fill: p.stroke_over_fill,
    })
}

fn pair(value: (LengthPercentage, LengthPercentage)) -> Result<[Length; 2], ExportError> {
    Ok([export_length(value.0)?, export_length(value.1)?])
}

fn axis_positions(list: &[crate::style::AxisPos]) -> Vec<doc::AxisPosition> {
    list.iter()
        .map(|axis| doc::AxisPosition {
            percent: axis.percent,
            length: axis.length,
        })
        .collect()
}

fn repeat(value: AxisRepeat) -> doc::BackgroundRepeat {
    match value {
        AxisRepeat::Repeat => doc::BackgroundRepeat::Repeat,
        AxisRepeat::NoRepeat => doc::BackgroundRepeat::NoRepeat,
        AxisRepeat::Space => doc::BackgroundRepeat::Space,
        AxisRepeat::Round => doc::BackgroundRepeat::Round,
    }
}

fn border_line(value: BorderLine) -> doc::BorderLine {
    match value {
        BorderLine::Solid => doc::BorderLine::Solid,
        BorderLine::Dashed => doc::BorderLine::Dashed,
        BorderLine::Dotted => doc::BorderLine::Dotted,
        BorderLine::Double => doc::BorderLine::Double,
        BorderLine::Groove => doc::BorderLine::Groove,
        BorderLine::Ridge => doc::BorderLine::Ridge,
        BorderLine::Inset => doc::BorderLine::Inset,
        BorderLine::Outset => doc::BorderLine::Outset,
    }
}

fn filter(value: &FilterFn) -> doc::CssFilter {
    match *value {
        FilterFn::Blur(radius) => doc::CssFilter::Blur { radius },
        FilterFn::Brightness(amount) => doc::CssFilter::Brightness { amount },
        FilterFn::Contrast(amount) => doc::CssFilter::Contrast { amount },
        FilterFn::Saturate(amount) => doc::CssFilter::Saturate { amount },
        FilterFn::Grayscale(amount) => doc::CssFilter::Grayscale { amount },
        FilterFn::HueRotate(degrees) => doc::CssFilter::HueRotate { degrees },
        FilterFn::DropShadow { x, y, blur, color } => doc::CssFilter::DropShadow {
            x,
            y,
            blur,
            color: rgba(color),
        },
        FilterFn::Invert(amount) => doc::CssFilter::Invert { amount },
        FilterFn::Sepia(amount) => doc::CssFilter::Sepia { amount },
        FilterFn::Opacity(amount) => doc::CssFilter::Opacity { amount },
    }
}
