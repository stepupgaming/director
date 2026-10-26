//! Border sides, styles, and corner joins.

use crate::style::{BorderLine, PaintStyle};
use skia_safe::{Canvas, Rect};

pub(super) fn paint_borders(
    canvas: &Canvas,
    rect: Rect,
    paint: &PaintStyle,
    widths: [f32; 4],
    radii: &[skia_safe::Point; 4],
    outer: skia_safe::RRect,
) {
    let [top, right, bottom, left] = widths;
    if top + right + bottom + left <= 0.0 {
        return;
    }
    let lines = paint.border_lines;
    let uniform = lines.iter().all(|line| *line == lines[0])
        && widths.iter().all(|width| *width == widths[0])
        && paint
            .border_colors
            .iter()
            .all(|color| *color == paint.border_colors[0])
        && matches!(lines[0], BorderLine::Dashed | BorderLine::Dotted)
        && top > 0.0;
    if uniform {
        let mut stroke = outer;
        stroke.inset((top / 2.0, top / 2.0));
        let mut p = skia_safe::Paint::default();
        p.set_anti_alias(true);
        p.set_color4f(paint.border_colors[0].to_color4f(), None);
        p.set_style(skia_safe::PaintStyle::Stroke);
        p.set_stroke_width(top);
        let dotted = lines[0] == BorderLine::Dotted;
        p.set_path_effect(skia_safe::PathEffect::dash(
            &if dotted {
                [0.0, top * 2.0]
            } else {
                [top * 3.0, top * 3.0]
            },
            0.0,
        ));
        if dotted {
            p.set_stroke_cap(skia_safe::paint::Cap::Round);
        }
        canvas.draw_rrect(stroke, &p);
        return;
    }
    let inner_rect = Rect::new(
        left,
        top,
        (rect.right - right).max(left),
        (rect.bottom - bottom).max(top),
    );
    let inner_radii = [
        skia_safe::Point::new((radii[0].x - left).max(0.0), (radii[0].y - top).max(0.0)),
        skia_safe::Point::new((radii[1].x - right).max(0.0), (radii[1].y - top).max(0.0)),
        skia_safe::Point::new(
            (radii[2].x - right).max(0.0),
            (radii[2].y - bottom).max(0.0),
        ),
        skia_safe::Point::new((radii[3].x - left).max(0.0), (radii[3].y - bottom).max(0.0)),
    ];
    canvas.save();
    canvas.clip_rrect(outer, skia_safe::ClipOp::Intersect, true);
    canvas.clip_rrect(
        skia_safe::RRect::new_rect_radii(inner_rect, &inner_radii),
        skia_safe::ClipOp::Difference,
        true,
    );
    let center = (rect.width() / 2.0, rect.height() / 2.0);
    let polys = [
        [(0.0, 0.0), (rect.width(), 0.0), center],
        [(rect.width(), 0.0), (rect.width(), rect.height()), center],
        [(rect.width(), rect.height()), (0.0, rect.height()), center],
        [(0.0, rect.height()), (0.0, 0.0), center],
    ];
    for (i, points) in polys.into_iter().enumerate() {
        if lines[i] != BorderLine::Solid || widths[i] <= 0.0 {
            continue;
        }
        let mut path = skia_safe::PathBuilder::new();
        path.move_to(points[0]);
        for p in &points[1..] {
            path.line_to(*p);
        }
        path.close();
        let mut p = skia_safe::Paint::default();
        p.set_anti_alias(true);
        p.set_color4f(paint.border_colors[i].to_color4f(), None);
        canvas.draw_path(&path.detach(), &p);
    }
    let sides = [
        [(0.0, top / 2.0), (rect.width(), top / 2.0)],
        [
            (rect.width() - right / 2.0, 0.0),
            (rect.width() - right / 2.0, rect.height()),
        ],
        [
            (rect.width(), rect.height() - bottom / 2.0),
            (0.0, rect.height() - bottom / 2.0),
        ],
        [(left / 2.0, rect.height()), (left / 2.0, 0.0)],
    ];
    for (i, [start, end]) in sides.into_iter().enumerate() {
        if widths[i] <= 0.0 || !matches!(lines[i], BorderLine::Dashed | BorderLine::Dotted) {
            continue;
        }
        let mut path = skia_safe::PathBuilder::new();
        path.move_to(start);
        path.line_to(end);
        let mut p = skia_safe::Paint::default();
        p.set_anti_alias(true);
        p.set_color4f(paint.border_colors[i].to_color4f(), None);
        p.set_style(skia_safe::PaintStyle::Stroke);
        p.set_stroke_width(widths[i]);
        let dotted = lines[i] == BorderLine::Dotted;
        p.set_path_effect(skia_safe::PathEffect::dash(
            &if dotted {
                [0.0, widths[i] * 2.0]
            } else {
                [widths[i] * 3.0, widths[i] * 3.0]
            },
            0.0,
        ));
        if dotted {
            p.set_stroke_cap(skia_safe::paint::Cap::Round);
        }
        canvas.draw_path(&path.detach(), &p);
    }
    paint_styled_borders(canvas, rect, paint, widths);
    canvas.restore();
}

fn paint_styled_borders(canvas: &Canvas, rect: Rect, paint: &PaintStyle, widths: [f32; 4]) {
    let polys = side_polygons(rect);
    for (i, points) in polys.into_iter().enumerate() {
        if widths[i] <= 0.0 {
            continue;
        }
        let color = paint.border_colors[i];
        match paint.border_lines[i] {
            BorderLine::Double => paint_double(canvas, rect, i, widths[i], color),
            BorderLine::Inset => fill_poly(canvas, points, bevel_color(color, i, true), true),
            BorderLine::Outset => fill_poly(canvas, points, bevel_color(color, i, false), true),
            BorderLine::Groove => paint_halves(canvas, rect, points, i, widths[i], color, true),
            BorderLine::Ridge => paint_halves(canvas, rect, points, i, widths[i], color, false),
            BorderLine::Solid | BorderLine::Dashed | BorderLine::Dotted => {}
        }
    }
}

fn side_polygons(rect: Rect) -> [[(f32, f32); 3]; 4] {
    let (x0, y0, x1, y1) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let center = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
    [
        [(x0, y0), (x1, y0), center],
        [(x1, y0), (x1, y1), center],
        [(x1, y1), (x0, y1), center],
        [(x0, y1), (x0, y0), center],
    ]
}

/// Outer and inner stripe thicknesses. Under 3px, `inner == 0` is one solid band.
/// `ceil(width / 3)` on each stripe leaves the remainder as the middle gap.
/// 8px is 3 + 2 + 3, so the empty rows are the ones that start at y = 3.
fn double_stripes(width: f32) -> (f32, f32) {
    if width < 3.0 {
        return (width, 0.0);
    }
    let stripe = (width / 3.0).ceil();
    if stripe * 2.0 < width {
        return (stripe, stripe);
    }
    let stripe = (width / 3.0).floor().max(1.0);
    if stripe * 2.0 < width {
        (stripe, stripe)
    } else {
        (width, 0.0)
    }
}

fn paint_double(
    canvas: &Canvas,
    rect: Rect,
    side: usize,
    width: f32,
    color: director_core::types::Color,
) {
    let (outer, inner) = double_stripes(width);
    fill_rect(canvas, edge_band(rect, side, 0.0, outer), color);
    if inner > 0.0 {
        fill_rect(
            canvas,
            edge_band(rect, side, width - inner, width),
            color,
        );
    }
}

fn paint_halves(
    canvas: &Canvas,
    rect: Rect,
    points: [(f32, f32); 3],
    side: usize,
    width: f32,
    color: director_core::types::Color,
    groove: bool,
) {
    let half = width * 0.5;
    // Groove is inset on the outside and outset on the inside. Ridge swaps that.
    fill_clipped_poly(
        canvas,
        points,
        edge_band(rect, side, 0.0, half),
        bevel_color(color, side, groove),
    );
    fill_clipped_poly(
        canvas,
        points,
        edge_band(rect, side, half, width),
        bevel_color(color, side, !groove),
    );
}

fn edge_band(rect: Rect, side: usize, from: f32, to: f32) -> Rect {
    let (x0, y0, x1, y1) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    match side {
        0 => Rect::new(x0, y0 + from, x1, y0 + to),
        1 => Rect::new(x1 - to, y0, x1 - from, y1),
        2 => Rect::new(x0, y1 - to, x1, y1 - from),
        _ => Rect::new(x0 + from, y0, x0 + to, y1),
    }
}

/// Top and left are lighter for outset and darker for inset. The other sides swap.
fn bevel_color(
    color: director_core::types::Color,
    side: usize,
    inset: bool,
) -> director_core::types::Color {
    let lighten = matches!(side, 0 | 3) != inset;
    let toward = if lighten { 1.0 } else { 0.0 };
    director_core::types::Color {
        r: color.r + (toward - color.r) * 0.5,
        g: color.g + (toward - color.g) * 0.5,
        b: color.b + (toward - color.b) * 0.5,
        a: color.a,
    }
}

fn fill_rect(canvas: &Canvas, band: Rect, color: director_core::types::Color) {
    if band.width() <= 0.0 || band.height() <= 0.0 {
        return;
    }
    let mut p = skia_safe::Paint::default();
    p.set_anti_alias(false);
    p.set_color4f(color.to_color4f(), None);
    canvas.draw_rect(band, &p);
}

fn fill_clipped_poly(
    canvas: &Canvas,
    points: [(f32, f32); 3],
    band: Rect,
    color: director_core::types::Color,
) {
    if band.width() <= 0.0 || band.height() <= 0.0 {
        return;
    }
    canvas.save();
    canvas.clip_rect(band, skia_safe::ClipOp::Intersect, false);
    fill_poly(canvas, points, color, false);
    canvas.restore();
}

fn fill_poly(
    canvas: &Canvas,
    points: [(f32, f32); 3],
    color: director_core::types::Color,
    anti_alias: bool,
) {
    let mut path = skia_safe::PathBuilder::new();
    path.move_to(points[0]);
    path.line_to(points[1]);
    path.line_to(points[2]);
    path.close();
    let mut p = skia_safe::Paint::default();
    p.set_anti_alias(anti_alias);
    p.set_color4f(color.to_color4f(), None);
    canvas.draw_path(&path.detach(), &p);
}
