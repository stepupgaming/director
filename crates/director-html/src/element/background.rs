//! Box shadows, background colors, gradient and image layers, and `background-clip: text`.

use super::HtmlElement;
use crate::background_box::{place_axis, AxisRepeat};
use crate::background_tiles::{place_tiles, PlacedTile};
use crate::box_edges::{edge_insets, BoxEdge};
use crate::style::{
    resolve, AxisPos, BackgroundFit, BackgroundLayer, BackgroundOrigin, LengthParts, PaintStyle,
};
use director_core::{Element, RenderError};
use skia_safe::{Canvas, FilterMode, Matrix, MipmapMode, RRect, Rect, SamplingOptions, TileMode};
use taffy::style::Style;

/// Outer box shadows, drawn outside the border box from last to first.
pub(super) fn paint_outer_shadows(canvas: &Canvas, paint: &PaintStyle, outer: RRect) {
    for shadow in paint.shadows.iter().rev().filter(|s| !s.inset) {
        let mut shadow_paint = skia_safe::Paint::default();
        shadow_paint.set_color4f(shadow.color.to_color4f(), None);
        shadow_paint.set_anti_alias(true);
        shadow_paint.set_image_filter(skia_safe::image_filters::blur(
            (shadow.blur, shadow.blur),
            skia_safe::TileMode::Decal,
            None,
            None,
        ));
        let mut rr = outer;
        rr.outset((shadow.spread, shadow.spread));
        rr.offset((shadow.x, shadow.y));
        canvas.save();
        canvas.clip_rrect(outer, skia_safe::ClipOp::Difference, true);
        canvas.draw_rrect(rr, &shadow_paint);
        canvas.restore();
    }
}

/// The background color and layers, clipped to the corner radii and `background-clip`.
pub(super) fn paint_background(
    canvas: &Canvas,
    element: &HtmlElement,
    rect: Rect,
    outer: RRect,
) -> Result<(), RenderError> {
    let mut bg = element.paint.box_node.clone();
    bg.clip_path = None;
    bg.overflow = "visible".into();
    bg.border_width = director_core::animation::Animated::new(0.0);
    canvas.save();
    canvas.clip_rrect(outer, skia_safe::ClipOp::Intersect, true);
    if let Some(clip) = background_clip_rect(rect, &element.style, element.paint.background_clip) {
        canvas.clip_rect(clip, skia_safe::ClipOp::Intersect, false);
    }
    let clip_text = element.paint.background_clip == BoxEdge::Text;
    if clip_text {
        canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default());
    }
    bg.render(canvas, rect, 1.0, &mut |_| {})?;
    let gradient_rect = inset_rect(
        rect,
        &element.style,
        origin_edge(element.paint.background_origin),
    );
    if gradient_rect.width() > 0.0 && gradient_rect.height() > 0.0 {
        for (index, layer) in element.paint.layers.iter().enumerate().rev() {
            let (size, position, repeat) = layer_background(&element.paint, index);
            match layer {
                BackgroundLayer::Gradient(gradient) => {
                    let tile_rect = gradient_tile(gradient_rect, size, position);
                    let custom = (tile_rect.left() - gradient_rect.left()).abs() > 0.5
                        || (tile_rect.top() - gradient_rect.top()).abs() > 0.5
                        || (tile_rect.width() - gradient_rect.width()).abs() > 0.5
                        || (tile_rect.height() - gradient_rect.height()).abs() > 0.5;
                    let repeats = repeat.0 != AxisRepeat::NoRepeat
                        || repeat.1 != AxisRepeat::NoRepeat;
                    let shader = if custom && repeats {
                        tiled_gradient(gradient, tile_rect, repeat)
                    } else if custom {
                        gradient.shader(tile_rect)
                    } else {
                        gradient.shader(gradient_rect)
                    };
                    if let Some(shader) = shader {
                        let mut p = skia_safe::Paint::default();
                        p.set_shader(shader);
                        p.set_anti_alias(true);
                        let dest = if custom && !repeats { tile_rect } else { rect };
                        canvas.draw_rect(dest, &p);
                    }
                }
                BackgroundLayer::Image(image) => {
                    paint_background_image(
                        canvas,
                        rect,
                        &element.style,
                        &element.paint,
                        image,
                        size,
                        position,
                        repeat,
                    );
                }
            }
        }
    }
    if clip_text {
        paint_text_clip_mask(canvas, element, rect);
        canvas.restore();
    }
    canvas.restore();
    Ok(())
}

/// Inset box shadows, drawn inside the border box from last to first.
pub(super) fn paint_inset_shadows(canvas: &Canvas, paint: &PaintStyle, rect: Rect, outer: RRect) {
    for shadow in paint.shadows.iter().rev().filter(|s| s.inset) {
        canvas.save();
        canvas.clip_rrect(outer, skia_safe::ClipOp::Intersect, true);
        let mut hole = outer;
        hole.inset((shadow.spread, shadow.spread));
        hole.offset((shadow.x, shadow.y));
        let extent = rect.width().max(rect.height())
            + shadow.x.abs()
            + shadow.y.abs()
            + 6.0 * shadow.blur
            + shadow.spread.abs();
        let mut coverage = rect;
        coverage.outset((extent, extent));
        let mut path = skia_safe::PathBuilder::new();
        path.set_fill_type(skia_safe::PathFillType::EvenOdd);
        path.add_rect(coverage, None, None);
        path.add_rrect(hole, None, None);
        let mut shadow_paint = skia_safe::Paint::default();
        shadow_paint.set_color4f(shadow.color.to_color4f(), None);
        shadow_paint.set_anti_alias(true);
        shadow_paint.set_image_filter(skia_safe::image_filters::blur(
            (shadow.blur, shadow.blur),
            skia_safe::TileMode::Decal,
            None,
            None,
        ));
        canvas.draw_path(&path.detach(), &shadow_paint);
        canvas.restore();
    }
}

fn origin_edge(origin: BackgroundOrigin) -> BoxEdge {
    match origin {
        BackgroundOrigin::BorderBox => BoxEdge::BorderBox,
        BackgroundOrigin::PaddingBox => BoxEdge::PaddingBox,
        BackgroundOrigin::ContentBox => BoxEdge::ContentBox,
    }
}

fn inset_rect(rect: Rect, layout: &Style, edge: BoxEdge) -> Rect {
    let width = rect.width();
    let insets = edge_insets(
        edge,
        layout.border.left.value(),
        layout.border.top.value(),
        layout.border.right.value(),
        layout.border.bottom.value(),
        resolve(layout.padding.left, width),
        resolve(layout.padding.top, width),
        resolve(layout.padding.right, width),
        resolve(layout.padding.bottom, width),
    );
    let left = rect.left + insets.left;
    let top = rect.top + insets.top;
    let right = (rect.right - insets.right).max(left);
    let bottom = (rect.bottom - insets.bottom).max(top);
    Rect::new(left, top, right, bottom)
}

fn background_clip_rect(rect: Rect, layout: &Style, clip: BoxEdge) -> Option<Rect> {
    if clip == BoxEdge::BorderBox || clip == BoxEdge::Text {
        return None;
    }
    Some(inset_rect(rect, layout, clip))
}

fn paint_text_clip_mask(canvas: &Canvas, element: &HtmlElement, rect: Rect) {
    let Some(content) = &element.content else {
        return;
    };
    let Some(node) = content
        .as_any()
        .downcast_ref::<director_core::node::TextNode>()
    else {
        return;
    };
    let content_rect = element.content_rect(rect);
    let paragraph = crate::compiler::glyph_mask(node, &element.paint, content_rect.width());
    let mut mask = skia_safe::Paint::default();
    mask.set_blend_mode(skia_safe::BlendMode::DstIn);
    canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default().paint(&mask));
    paragraph.paint(canvas, (content_rect.x(), content_rect.y()));
    canvas.restore();
}

fn layer_background(
    paint: &PaintStyle,
    index: usize,
) -> (
    BackgroundFit,
    (AxisPos, AxisPos),
    (AxisRepeat, AxisRepeat),
) {
    let origin = AxisPos {
        percent: 0.0,
        length: 0.0,
    };
    (
        cycled(&paint.background_size, index, BackgroundFit::Auto),
        (
            cycled(&paint.background_position_x, index, origin),
            cycled(&paint.background_position_y, index, origin),
        ),
        cycled(
            &paint.background_repeat,
            index,
            (AxisRepeat::Repeat, AxisRepeat::Repeat),
        ),
    )
}

fn cycled<T: Copy>(values: &[T], index: usize, fallback: T) -> T {
    if values.is_empty() {
        fallback
    } else {
        values[index % values.len()]
    }
}

fn gradient_tile(area: Rect, size: BackgroundFit, position: (AxisPos, AxisPos)) -> Rect {
    let (tw, th) = match size {
        BackgroundFit::Lengths(width, height) => {
            let across = |value: Option<taffy::LengthPercentage>, basis: f32| {
                value
                    .map(|value| resolve(value, basis))
                    .filter(|value| value.is_finite() && *value > 0.0)
                    .unwrap_or(basis)
            };
            (across(width, area.width()), across(height, area.height()))
        }
        _ => (area.width(), area.height()),
    };
    let x = area.left() + (area.width() - tw) * position.0.percent + position.0.length;
    let y = area.top() + (area.height() - th) * position.1.percent + position.1.length;
    Rect::from_xywh(x, y, tw.max(1.0), th.max(1.0))
}

/// A sized gradient repeats as a 2D image tile, as CSS does. A gradient
/// shader's own repeat mode runs along the gradient line instead, which turns
/// a radial dot into concentric rings and a diagonal tile into long stripes.
fn tiled_gradient(
    gradient: &crate::gradient::NativeGradient,
    tile: Rect,
    repeat: (AxisRepeat, AxisRepeat),
) -> Option<skia_safe::Shader> {
    let bounds = Rect::from_wh(tile.width(), tile.height());
    let mut p = skia_safe::Paint::default();
    p.set_shader(gradient.shader(bounds)?);
    p.set_anti_alias(true);
    let mut recorder = skia_safe::PictureRecorder::new();
    recorder.begin_recording(bounds, false).draw_rect(bounds, &p);
    let picture = recorder.finish_recording_as_picture(Some(&bounds))?;
    let mode = |axis: AxisRepeat| {
        if axis == AxisRepeat::NoRepeat {
            TileMode::Decal
        } else {
            TileMode::Repeat
        }
    };
    Some(picture.to_shader(
        (mode(repeat.0), mode(repeat.1)),
        FilterMode::Linear,
        &Matrix::translate((tile.left(), tile.top())),
        &bounds,
    ))
}

#[allow(clippy::too_many_arguments)]
fn paint_background_image(
    canvas: &Canvas,
    rect: Rect,
    layout: &Style,
    paint: &PaintStyle,
    image: &skia_safe::Image,
    size: BackgroundFit,
    position: (AxisPos, AxisPos),
    repeat: (AxisRepeat, AxisRepeat),
) {
    let iw = image.width() as f32;
    let ih = image.height() as f32;
    if iw <= 0.0 || ih <= 0.0 {
        return;
    }
    let (inset_l, inset_t, inset_r, inset_b) = match paint.background_origin {
        BackgroundOrigin::BorderBox => (0.0, 0.0, 0.0, 0.0),
        BackgroundOrigin::PaddingBox => (
            layout.border.left.value(),
            layout.border.top.value(),
            layout.border.right.value(),
            layout.border.bottom.value(),
        ),
        BackgroundOrigin::ContentBox => (
            layout.border.left.value() + resolve(layout.padding.left, rect.width()),
            layout.border.top.value() + resolve(layout.padding.top, rect.width()),
            layout.border.right.value() + resolve(layout.padding.right, rect.width()),
            layout.border.bottom.value() + resolve(layout.padding.bottom, rect.width()),
        ),
    };
    let area_w = (rect.width() - inset_l - inset_r).max(0.0);
    let area_h = (rect.height() - inset_t - inset_b).max(0.0);
    if area_w <= 0.0 || area_h <= 0.0 {
        return;
    }
    let (w, h) = fitted_background(size, iw, ih, area_w, area_h);
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let (repeat_x, repeat_y) = repeat;
    let shader_repeat = matches!(repeat_x, AxisRepeat::Repeat | AxisRepeat::NoRepeat)
        && matches!(repeat_y, AxisRepeat::Repeat | AxisRepeat::NoRepeat);
    if !shader_repeat {
        paint_placed_tiles(
            canvas, image, rect, inset_l, inset_t, area_w, area_h, w, h, position, repeat,
        );
        return;
    }
    let x = rect.left + inset_l + position.0.percent * (area_w - w) + position.0.length;
    let y = rect.top + inset_t + position.1.percent * (area_h - h) + position.1.length;
    let tile_x = if matches!(repeat_x, AxisRepeat::Repeat) {
        TileMode::Repeat
    } else {
        TileMode::Decal
    };
    let tile_y = if matches!(repeat_y, AxisRepeat::Repeat) {
        TileMode::Repeat
    } else {
        TileMode::Decal
    };
    // Skia image shaders map image space onto the canvas. Scale the tile to the
    // fitted size, then move its origin to the background position.
    let local = Matrix::scale_translate((w / iw, h / ih), (x, y));
    let Some(shader) = image.to_shader(
        (tile_x, tile_y),
        SamplingOptions::new(FilterMode::Nearest, MipmapMode::None),
        &local,
    ) else {
        return;
    };
    let mut p = skia_safe::Paint::default();
    p.set_shader(shader);
    canvas.draw_rect(rect, &p);
}

fn fitted_background(fit: BackgroundFit, iw: f32, ih: f32, area_w: f32, area_h: f32) -> (f32, f32) {
    match fit {
        BackgroundFit::Auto => (iw, ih),
        BackgroundFit::Cover => {
            let scale = (area_w / iw).max(area_h / ih);
            (iw * scale, ih * scale)
        }
        BackgroundFit::Contain => {
            let scale = (area_w / iw).min(area_h / ih);
            (iw * scale, ih * scale)
        }
        BackgroundFit::Lengths(width, height) => match (width, height) {
            (None, None) => (iw, ih),
            (Some(width), None) => {
                let w = resolve(width, area_w);
                (w, if iw > 0.0 { w * ih / iw } else { ih })
            }
            (None, Some(height)) => {
                let h = resolve(height, area_h);
                (if ih > 0.0 { h * iw / ih } else { iw }, h)
            }
            (Some(width), Some(height)) => (resolve(width, area_w), resolve(height, area_h)),
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_placed_tiles(
    canvas: &Canvas,
    image: &skia_safe::Image,
    rect: Rect,
    inset_l: f32,
    inset_t: f32,
    area_w: f32,
    area_h: f32,
    tile_w: f32,
    tile_h: f32,
    position: (AxisPos, AxisPos),
    repeat: (AxisRepeat, AxisRepeat),
) {
    let tiles = explicit_background_tiles(
        area_w,
        area_h,
        tile_w,
        tile_h,
        position.0.percent,
        position.0.length,
        position.1.percent,
        position.1.length,
        repeat.0,
        repeat.1,
    );
    if tiles.is_empty() {
        return;
    }
    canvas.save();
    canvas.clip_rect(rect, skia_safe::ClipOp::Intersect, false);
    let sampling = SamplingOptions::new(FilterMode::Nearest, MipmapMode::None);
    let mut tile_paint = skia_safe::Paint::default();
    tile_paint.set_anti_alias(false);
    let origin_x = rect.left + inset_l;
    let origin_y = rect.top + inset_t;
    for tile in tiles {
        let dst = Rect::from_xywh(origin_x + tile.x, origin_y + tile.y, tile.w, tile.h);
        canvas.draw_image_rect_with_sampling_options(image, None, dst, sampling, &tile_paint);
    }
    canvas.restore();
}

#[allow(clippy::too_many_arguments)]
fn explicit_background_tiles(
    area_w: f32,
    area_h: f32,
    tile_w: f32,
    tile_h: f32,
    pos_x_percent: f32,
    pos_x_length: f32,
    pos_y_percent: f32,
    pos_y_length: f32,
    repeat_x: AxisRepeat,
    repeat_y: AxisRepeat,
) -> Vec<PlacedTile> {
    let repeat_axis =
        matches!(repeat_x, AxisRepeat::Repeat) || matches!(repeat_y, AxisRepeat::Repeat);
    if !repeat_axis {
        let placed = place_tiles(
            0.0,
            0.0,
            area_w,
            area_h,
            tile_w,
            tile_h,
            pos_x_percent,
            pos_x_length,
            pos_y_percent,
            pos_y_length,
            repeat_x,
            repeat_y,
        );
        if !placed.is_empty() {
            return placed;
        }
    }
    let mut xs = axis_spans(area_w, tile_w, pos_x_percent, pos_x_length, repeat_x);
    let mut ys = axis_spans(area_h, tile_h, pos_y_percent, pos_y_length, repeat_y);
    if xs.is_empty() || ys.is_empty() {
        return Vec::new();
    }
    while xs.len() * ys.len() > 4096 {
        if xs.len() >= ys.len() && xs.len() > 1 {
            xs.pop();
        } else if ys.len() > 1 {
            ys.pop();
        } else {
            break;
        }
    }
    let mut tiles = Vec::with_capacity(xs.len() * ys.len());
    for (y, h) in ys {
        for (x, w) in &xs {
            tiles.push(PlacedTile {
                x: *x,
                y,
                w: *w,
                h,
            });
        }
    }
    tiles
}

fn axis_spans(
    area: f32,
    tile: f32,
    pos_percent: f32,
    pos_length: f32,
    repeat: AxisRepeat,
) -> Vec<(f32, f32)> {
    let placed = place_axis(area, tile, pos_percent, pos_length, repeat);
    if placed.count == 0 {
        return Vec::new();
    }
    if matches!(repeat, AxisRepeat::Repeat) {
        return repeat_spans(placed.origin, placed.tile, area);
    }
    let mut spans = Vec::with_capacity(placed.count.min(4096) as usize);
    let step = placed.tile + placed.gap;
    for i in 0..placed.count {
        spans.push((placed.origin + i as f32 * step, placed.tile));
        if spans.len() == 4096 {
            break;
        }
    }
    spans
}

fn repeat_spans(origin: f32, tile: f32, area: f32) -> Vec<(f32, f32)> {
    if tile <= 0.0 || !tile.is_finite() || !origin.is_finite() || !area.is_finite() || area <= 0.0
    {
        return Vec::new();
    }
    let start = ((-origin) / tile).floor();
    let end = ((area - origin) / tile).ceil();
    if !start.is_finite() || !end.is_finite() {
        return Vec::new();
    }
    let mut i = start as i64 - 1;
    let last = end as i64 + 1;
    let mut spans = Vec::new();
    while i <= last && spans.len() < 4096 {
        let x = origin + i as f32 * tile;
        if x < area && x + tile > 0.0 {
            spans.push((x, tile));
        }
        i += 1;
    }
    spans
}

#[cfg(test)]
mod background_tile_tests {
    use super::{explicit_background_tiles, AxisRepeat};

    #[test]
    fn huge_space_grid_is_capped_and_nonempty() {
        let tiles = explicit_background_tiles(
            100_000.0,
            100_000.0,
            1.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            AxisRepeat::Space,
            AxisRepeat::Space,
        );
        assert!(!tiles.is_empty());
        assert!(tiles.len() <= 4096);
    }

    #[test]
    fn repeat_across_space_covers_the_repeat_axis() {
        let tiles = explicit_background_tiles(
            64.0,
            64.0,
            30.0,
            30.0,
            0.0,
            0.0,
            0.0,
            0.0,
            AxisRepeat::Repeat,
            AxisRepeat::Space,
        );
        assert_eq!(tiles.len(), 6);
        assert!(tiles.iter().any(|tile| tile.x == 0.0 && tile.y == 0.0));
        assert!(tiles.iter().any(|tile| tile.x == 30.0 && tile.y == 34.0));
    }
}
