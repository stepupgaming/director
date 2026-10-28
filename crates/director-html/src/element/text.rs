//! Text decoration lines painted over the content paragraph.

use super::HtmlElement;
use skia_safe::{Canvas, Rect};

pub(super) fn paint_text_lines(canvas: &Canvas, element: &HtmlElement, rect: Rect) {
    let lines = element.paint.decoration;
    if !lines.underline && !lines.overline && !lines.line_through {
        return;
    }
    let Some(text) = element
        .content
        .as_ref()
        .and_then(|content| content.as_any().downcast_ref::<director_core::node::TextNode>())
    else {
        return;
    };
    let content = element.content_rect(rect);
    let count = text.get_total_char_count();
    if count == 0 {
        return;
    }
    let auto = (element.paint.font_size / 16.0).max(1.0).round();
    let requested = element.paint.decoration_thickness;
    let spans = {
        let _fonts = text.font_collection.lock();
        let mut guard = match text.paragraph.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        let Some(paragraph) = guard.as_mut() else {
            return;
        };
        let layout_width = if content.width() < 1.0 {
            f32::INFINITY
        } else {
            content.width()
        };
        paragraph.layout(layout_width);
        let thickness = match requested {
            crate::decoration_value::DecorationThickness::Auto => auto,
            crate::decoration_value::DecorationThickness::Px(px) => px,
            crate::decoration_value::DecorationThickness::FromFont => {
                font_underline_thickness(paragraph).unwrap_or(auto)
            }
        };
        let metrics = paragraph.get_line_metrics();
        let boxes = paragraph.get_rects_for_range(
            0..count,
            skia_safe::textlayout::RectHeightStyle::Tight,
            skia_safe::textlayout::RectWidthStyle::Tight,
        );
        let mut spans = Vec::new();
        for text_box in boxes {
            let glyph = text_box.rect;
            if glyph.width() <= 0.0 {
                continue;
            }
            let mid = (glyph.top + glyph.bottom) * 0.5;
            let line = metrics.iter().find(|line| {
                let top = (line.baseline - line.ascent) as f32;
                let bottom = (line.baseline + line.descent) as f32;
                mid >= top - 0.5 && mid <= bottom + 0.5
            });
            let (under, over, strike) = if let Some(line) = line {
                let baseline = line.baseline as f32;
                let top = baseline - line.ascent as f32;
                (
                    baseline.floor() + 1.0,
                    top.floor() - thickness,
                    (baseline - line.ascent as f32 * 0.35).floor(),
                )
            } else {
                (
                    glyph.bottom.floor() + 1.0,
                    glyph.top.floor() - thickness,
                    ((glyph.top + glyph.bottom - thickness) * 0.5).floor(),
                )
            };
            spans.push((
                content.left + glyph.left,
                glyph.width().max(thickness),
                content.top + under,
                content.top + over,
                content.top + strike,
            ));
        }
        (spans, thickness)
    };
    let (spans, thickness) = spans;
    let mut paint = skia_safe::Paint::default();
    paint.set_anti_alias(false);
    let ink = element.paint.decoration_color.unwrap_or(element.paint.color);
    paint.set_color4f(ink.to_color4f(), None);
    for (x, w, under, over, strike) in spans {
        if lines.underline {
            fill_decoration(
                canvas,
                &paint,
                element.paint.decoration_style,
                x,
                under + element.paint.text_underline_offset,
                w,
                thickness,
            );
        }
        if lines.overline {
            fill_decoration(
                canvas,
                &paint,
                element.paint.decoration_style,
                x,
                over,
                w,
                thickness,
            );
        }
        if lines.line_through {
            fill_decoration(
                canvas,
                &paint,
                element.paint.decoration_style,
                x,
                strike,
                w,
                thickness,
            );
        }
    }
}

fn font_underline_thickness(paragraph: &skia_safe::textlayout::Paragraph) -> Option<f32> {
    let lines = paragraph.get_line_metrics();
    let line = lines.first()?;
    let end = line
        .end_including_newline
        .max(line.end_index)
        .max(line.start_index.saturating_add(1));
    let styles = line.get_style_metrics(line.start_index..end);
    let thickness = styles
        .first()
        .and_then(|(_, style)| style.font_metrics.underline_thickness())?;
    if thickness.is_finite() && thickness > 0.0 {
        Some(thickness)
    } else {
        None
    }
}

fn fill_decoration(
    canvas: &Canvas,
    paint: &skia_safe::Paint,
    style: crate::decoration_marks::DecorationStyle,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    for mark in crate::decoration_marks::marks(style, x, y, w, h) {
        match mark {
            crate::decoration_marks::Mark::Fill { x, y, w, h } => {
                canvas.draw_rect(Rect::from_xywh(x, y, w, h), paint);
            }
            crate::decoration_marks::Mark::Stroke { points, width } => {
                let mut path = skia_safe::PathBuilder::new();
                let mut drawn = false;
                for (px, py) in points {
                    if drawn {
                        path.line_to((px, py));
                    } else {
                        path.move_to((px, py));
                        drawn = true;
                    }
                }
                let mut stroke = paint.clone();
                stroke.set_style(skia_safe::paint::Style::Stroke);
                stroke.set_stroke_width(width);
                canvas.draw_path(&path.detach(), &stroke);
            }
        }
    }
}
