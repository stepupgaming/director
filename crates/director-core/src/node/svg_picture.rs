//! Records a resolved SVG tree as a Skia picture, so the canvas draws it at
//! any scale, on the GPU when the canvas has one, with no raster to upload.
//! Trees that use masks, patterns, embedded images or filter primitives
//! outside the supported set return `None` and keep the resvg raster.

use skia_safe::{
    canvas::SaveLayerRec, color_filters, gradient, image_filters, scalar, shaders, BlendMode, Canvas, ClipOp,
    Color4f, ImageFilter, Matrix, Paint, PaintCap, PaintJoin, PaintStyle, Path, PathBuilder,
    PathEffect, PathFillType, PathOp, Picture, PictureRecorder, Rect, TileMode,
};
use std::collections::HashMap;
use tiny_skia::PathSegment;
use usvg::filter::{ColorInterpolation, ColorMatrixKind, CompositeOperator, Filter, Input, Kind};
use usvg::{Group, Node, Tree};

/// The tree as a picture in its own `0..size` space, or `None` when it uses a
/// feature only the resvg raster draws.
pub fn record(tree: &Tree) -> Option<Picture> {
    if !supported(tree.root()) {
        return None;
    }
    let size = tree.size();
    let mut recorder = PictureRecorder::new();
    // The bounding hierarchy trims the cull rect to what the tree draws, so a
    // full-frame viewBox around a small drawing keeps small bounds.
    let canvas = recorder.begin_recording(Rect::from_wh(size.width(), size.height()), true);
    draw_group(canvas, tree.root());
    recorder.finish_recording_as_picture(None)
}

fn supported(group: &Group) -> bool {
    group.mask().is_none()
        && group.clip_path().is_none_or(|clip| clip.clip_path().is_none() && clip_path(clip.root()).is_some())
        && group.filters().iter().all(|filter| filter.primitives().iter().all(|p| primitive_supported(p.kind())))
        && group.children().iter().all(|node| match node {
            Node::Group(group) => supported(group),
            Node::Path(path) => {
                let pattern = |paint: &usvg::Paint| matches!(paint, usvg::Paint::Pattern(_));
                !path.fill().is_some_and(|fill| pattern(fill.paint()))
                    && !path.stroke().is_some_and(|stroke| pattern(stroke.paint()))
            }
            Node::Image(_) => false,
            Node::Text(text) => supported(text.flattened()),
        })
}

fn primitive_supported(kind: &Kind) -> bool {
    matches!(
        kind,
        Kind::GaussianBlur(_)
            | Kind::Offset(_)
            | Kind::Flood(_)
            | Kind::Merge(_)
            | Kind::Blend(_)
            | Kind::Composite(_)
            | Kind::ColorMatrix(_)
            | Kind::DropShadow(_)
    )
}

fn matrix(ts: tiny_skia::Transform) -> Matrix {
    Matrix::new_all(ts.sx, ts.kx, ts.tx, ts.ky, ts.sy, ts.ty, 0.0, 0.0, 1.0)
}

fn rect(rect: tiny_skia::NonZeroRect) -> Rect {
    Rect::new(rect.left(), rect.top(), rect.right(), rect.bottom())
}

fn draw_group(canvas: &Canvas, group: &Group) {
    canvas.save();
    canvas.concat(&matrix(group.transform()));
    if group.should_isolate() {
        if let Some(clip) = group.clip_path() {
            if let Some(path) = clip_path(clip.root()) {
                canvas.clip_path(&path.with_transform(&matrix(clip.transform())), ClipOp::Intersect, true);
            }
        }
        let mut paint = Paint::default();
        paint.set_alpha_f(group.opacity().get());
        paint.set_blend_mode(blend_mode(group.blend_mode()));
        let filter = group.filters().iter().try_fold(None, |inner: Option<ImageFilter>, filter| {
            let outer = filter_chain(filter)?;
            Some(Some(match inner {
                Some(inner) => image_filters::compose(outer, inner)?,
                None => outer,
            }))
        });
        if let Some(Some(filter)) = filter {
            paint.set_image_filter(filter);
        }
        let bounds = group.filters_bounding_box().map(rect);
        let mut layer = SaveLayerRec::default().paint(&paint);
        if let Some(bounds) = bounds.as_ref() {
            layer = layer.bounds(bounds);
        }
        canvas.save_layer(&layer);
        draw_children(canvas, group);
        canvas.restore();
    } else {
        draw_children(canvas, group);
    }
    canvas.restore();
}

fn draw_children(canvas: &Canvas, group: &Group) {
    for node in group.children() {
        match node {
            Node::Group(group) => draw_group(canvas, group),
            Node::Path(path) => draw_path(canvas, path),
            Node::Text(text) => draw_group(canvas, text.flattened()),
            Node::Image(_) => {}
        }
    }
}

fn skia_path(data: &tiny_skia::Path, rule: usvg::FillRule) -> Path {
    let mut path = PathBuilder::new();
    for segment in data.segments() {
        match segment {
            PathSegment::MoveTo(p) => path.move_to((p.x, p.y)),
            PathSegment::LineTo(p) => path.line_to((p.x, p.y)),
            PathSegment::QuadTo(c, p) => path.quad_to((c.x, c.y), (p.x, p.y)),
            PathSegment::CubicTo(c1, c2, p) => path.cubic_to((c1.x, c1.y), (c2.x, c2.y), (p.x, p.y)),
            PathSegment::Close => path.close(),
        };
    }
    path.set_fill_type(match rule {
        usvg::FillRule::NonZero => PathFillType::Winding,
        usvg::FillRule::EvenOdd => PathFillType::EvenOdd,
    });
    path.detach()
}

/// The union of a clip path's shapes, or `None` when it nests groups that
/// isolate or holds anything beyond paths.
fn clip_path(group: &Group) -> Option<Path> {
    let mut union: Option<Path> = None;
    for node in group.children() {
        let path = match node {
            Node::Path(path) if path.is_visible() => {
                let rule = path.fill().map_or(usvg::FillRule::NonZero, |fill| fill.rule());
                skia_path(path.data(), rule)
            }
            Node::Path(_) => continue,
            Node::Group(child) if !child.should_isolate() => clip_path(child)?.with_transform(&matrix(child.transform())),
            Node::Text(text) if !text.flattened().should_isolate() => {
                clip_path(text.flattened())?.with_transform(&matrix(text.flattened().transform()))
            }
            _ => return None,
        };
        union = Some(match union {
            Some(union) => union.op(&path, PathOp::Union)?,
            None => path,
        });
    }
    Some(union.unwrap_or_default())
}

fn draw_path(canvas: &Canvas, path: &usvg::Path) {
    if !path.is_visible() {
        return;
    }
    let anti_alias = path.rendering_mode().use_shape_antialiasing();
    let fill = || {
        let fill = path.fill()?;
        let bounds = path.data().bounds();
        if bounds.width() == 0.0 || bounds.height() == 0.0 {
            return None;
        }
        let mut paint = paint(fill.paint(), fill.opacity().get())?;
        paint.set_anti_alias(anti_alias);
        canvas.draw_path(&skia_path(path.data(), fill.rule()), &paint);
        Some(())
    };
    let stroke = || {
        let stroke = path.stroke()?;
        let mut paint = paint(stroke.paint(), stroke.opacity().get())?;
        paint.set_anti_alias(anti_alias);
        paint.set_style(PaintStyle::Stroke);
        paint.set_stroke_width(stroke.width().get());
        paint.set_stroke_miter(stroke.miterlimit().get());
        paint.set_stroke_cap(match stroke.linecap() {
            usvg::LineCap::Butt => PaintCap::Butt,
            usvg::LineCap::Round => PaintCap::Round,
            usvg::LineCap::Square => PaintCap::Square,
        });
        paint.set_stroke_join(match stroke.linejoin() {
            usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => PaintJoin::Miter,
            usvg::LineJoin::Round => PaintJoin::Round,
            usvg::LineJoin::Bevel => PaintJoin::Bevel,
        });
        if let Some(dashes) = stroke.dasharray() {
            paint.set_path_effect(PathEffect::dash(dashes, stroke.dashoffset()));
        }
        canvas.draw_path(&skia_path(path.data(), usvg::FillRule::NonZero), &paint);
        Some(())
    };
    if path.paint_order() == usvg::PaintOrder::FillAndStroke {
        fill();
        stroke();
    } else {
        stroke();
        fill();
    }
}

fn color(color: usvg::Color, alpha: f32) -> Color4f {
    Color4f::new(color.red as f32 / 255.0, color.green as f32 / 255.0, color.blue as f32 / 255.0, alpha)
}

fn paint(source: &usvg::Paint, opacity: f32) -> Option<Paint> {
    let ramp = |base: &usvg::BaseGradient| {
        let colors: Vec<Color4f> = base.stops().iter().map(|stop| color(stop.color(), stop.opacity().get() * opacity)).collect();
        let positions: Vec<scalar> = base.stops().iter().map(|stop| stop.offset().get()).collect();
        let mode = match base.spread_method() {
            usvg::SpreadMethod::Pad => TileMode::Clamp,
            usvg::SpreadMethod::Reflect => TileMode::Mirror,
            usvg::SpreadMethod::Repeat => TileMode::Repeat,
        };
        (colors, positions, mode, matrix(base.transform()))
    };
    let shader = match source {
        usvg::Paint::Color(c) => return Some(Paint::new(color(*c, opacity), None)),
        usvg::Paint::LinearGradient(lg) => {
            let (colors, positions, mode, local) = ramp(lg);
            let ramp = gradient::Gradient::new(
                gradient::Colors::new(&colors, Some(&positions), mode, None),
                gradient::Interpolation::default(),
            );
            shaders::linear_gradient(((lg.x1(), lg.y1()), (lg.x2(), lg.y2())), &ramp, Some(&local))?
        }
        usvg::Paint::RadialGradient(rg) => {
            let (colors, positions, mode, local) = ramp(rg);
            let ramp = gradient::Gradient::new(
                gradient::Colors::new(&colors, Some(&positions), mode, None),
                gradient::Interpolation::default(),
            );
            shaders::two_point_conical_gradient(
                ((rg.fx(), rg.fy()), 0.0),
                ((rg.cx(), rg.cy()), rg.r().get()),
                &ramp,
                Some(&local),
            )?
        }
        usvg::Paint::Pattern(_) => return None,
    };
    let mut paint = Paint::default();
    paint.set_shader(shader);
    Some(paint)
}

fn blend_mode(mode: usvg::BlendMode) -> BlendMode {
    match mode {
        usvg::BlendMode::Normal => BlendMode::SrcOver,
        usvg::BlendMode::Multiply => BlendMode::Multiply,
        usvg::BlendMode::Screen => BlendMode::Screen,
        usvg::BlendMode::Overlay => BlendMode::Overlay,
        usvg::BlendMode::Darken => BlendMode::Darken,
        usvg::BlendMode::Lighten => BlendMode::Lighten,
        usvg::BlendMode::ColorDodge => BlendMode::ColorDodge,
        usvg::BlendMode::ColorBurn => BlendMode::ColorBurn,
        usvg::BlendMode::HardLight => BlendMode::HardLight,
        usvg::BlendMode::SoftLight => BlendMode::SoftLight,
        usvg::BlendMode::Difference => BlendMode::Difference,
        usvg::BlendMode::Exclusion => BlendMode::Exclusion,
        usvg::BlendMode::Hue => BlendMode::Hue,
        usvg::BlendMode::Saturation => BlendMode::Saturation,
        usvg::BlendMode::Color => BlendMode::Color,
        usvg::BlendMode::Luminosity => BlendMode::Luminosity,
    }
}

/// A filter result and whether it holds linear RGB.
#[derive(Clone)]
struct Output {
    filter: ImageFilter,
    linear: bool,
}

impl Output {
    /// This result in linear RGB when `linear`, otherwise in sRGB.
    fn into_space(self, linear: bool) -> Option<ImageFilter> {
        if self.linear == linear {
            return Some(self.filter);
        }
        let convert = if linear { color_filters::srgb_to_linear_gamma() } else { color_filters::linear_to_srgb_gamma() };
        image_filters::color_filter(convert, self.filter, None)
    }
}

/// One SVG filter as a Skia image filter graph over the layer it applies to.
/// The filter region crops both the source and the output.
fn filter_chain(filter: &Filter) -> Option<ImageFilter> {
    let region = rect(filter.rect());
    let source = Output { filter: image_filters::crop(region, TileMode::Decal, None)?, linear: false };
    let mut results: HashMap<&str, Output> = HashMap::new();
    let mut last = source.clone();
    for primitive in filter.primitives() {
        let linear = primitive.color_interpolation() == ColorInterpolation::LinearRGB;
        let crop = rect(primitive.rect());
        let input = |input: &Input| -> Option<ImageFilter> {
            let result = match input {
                Input::SourceGraphic => source.clone(),
                Input::SourceAlpha => Output {
                    filter: image_filters::color_filter(alpha_only(), source.filter.clone(), None)?,
                    linear: false,
                },
                Input::Reference(name) => results.get(name.as_str()).unwrap_or(&last).clone(),
            };
            result.into_space(linear)
        };
        let filter = match primitive.kind() {
            Kind::GaussianBlur(blur) => image_filters::blur(
                (blur.std_dev_x().get(), blur.std_dev_y().get()),
                TileMode::Decal,
                input(blur.input())?,
                crop,
            )?,
            Kind::Offset(offset) => image_filters::offset((offset.dx(), offset.dy()), input(offset.input())?, crop)?,
            Kind::Flood(flood) => {
                let flood = Output {
                    filter: image_filters::shader(shaders::color(color(flood.color(), flood.opacity().get()).to_color()), crop)?,
                    linear: false,
                };
                flood.into_space(linear)?
            }
            Kind::Merge(merge) => {
                let inputs = merge.inputs().iter().map(|i| input(i).map(Some)).collect::<Option<Vec<_>>>()?;
                image_filters::merge(inputs, crop)?
            }
            Kind::Blend(blend) => image_filters::blend(
                blend_mode(blend.mode()),
                input(blend.input2())?,
                input(blend.input1())?,
                crop,
            )?,
            Kind::Composite(composite) => {
                let (background, foreground) = (input(composite.input2())?, input(composite.input1())?);
                let mode = match composite.operator() {
                    CompositeOperator::Over => BlendMode::SrcOver,
                    CompositeOperator::In => BlendMode::SrcIn,
                    CompositeOperator::Out => BlendMode::SrcOut,
                    CompositeOperator::Atop => BlendMode::SrcATop,
                    CompositeOperator::Xor => BlendMode::Xor,
                    CompositeOperator::Arithmetic { k1, k2, k3, k4 } => {
                        let filter = image_filters::arithmetic(k1, k2, k3, k4, true, background, foreground, crop)?;
                        results.insert(primitive.result(), Output { filter: filter.clone(), linear });
                        last = Output { filter, linear };
                        continue;
                    }
                };
                image_filters::blend(mode, background, foreground, crop)?
            }
            Kind::ColorMatrix(matrix) => {
                image_filters::color_filter(color_matrix(matrix.kind()), input(matrix.input())?, crop)?
            }
            Kind::DropShadow(shadow) => image_filters::drop_shadow(
                (shadow.dx(), shadow.dy()),
                (shadow.std_dev_x().get(), shadow.std_dev_y().get()),
                color(shadow.color(), shadow.opacity().get()),
                None,
                input(shadow.input())?,
                crop,
            )?,
            _ => return None,
        };
        results.insert(primitive.result(), Output { filter: filter.clone(), linear });
        last = Output { filter, linear };
    }
    image_filters::crop(region, TileMode::Decal, last.into_space(false)?)
}

fn alpha_only() -> skia_safe::ColorFilter {
    #[rustfmt::skip]
    let rows = [
        0.0, 0.0, 0.0, 0.0, 0.0,
        0.0, 0.0, 0.0, 0.0, 0.0,
        0.0, 0.0, 0.0, 0.0, 0.0,
        0.0, 0.0, 0.0, 1.0, 0.0,
    ];
    color_filters::matrix_row_major(&rows, None)
}

/// The `feColorMatrix` matrices from the SVG filter specification.
fn color_matrix(kind: &ColorMatrixKind) -> skia_safe::ColorFilter {
    let rows: [f32; 20] = match kind {
        ColorMatrixKind::Matrix(values) => values.as_slice().try_into().unwrap_or_default(),
        ColorMatrixKind::Saturate(s) => {
            let s = s.get();
            [
                0.213 + 0.787 * s,
                0.715 - 0.715 * s,
                0.072 - 0.072 * s,
                0.0,
                0.0,
                0.213 - 0.213 * s,
                0.715 + 0.285 * s,
                0.072 - 0.072 * s,
                0.0,
                0.0,
                0.213 - 0.213 * s,
                0.715 - 0.715 * s,
                0.072 + 0.928 * s,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
            ]
        }
        ColorMatrixKind::HueRotate(degrees) => {
            let (sin, cos) = degrees.to_radians().sin_cos();
            [
                0.213 + cos * 0.787 - sin * 0.213,
                0.715 - cos * 0.715 - sin * 0.715,
                0.072 - cos * 0.072 + sin * 0.928,
                0.0,
                0.0,
                0.213 - cos * 0.213 + sin * 0.143,
                0.715 + cos * 0.285 + sin * 0.140,
                0.072 - cos * 0.072 - sin * 0.283,
                0.0,
                0.0,
                0.213 - cos * 0.213 - sin * 0.787,
                0.715 - cos * 0.715 + sin * 0.715,
                0.072 + cos * 0.928 + sin * 0.072,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
            ]
        }
        ColorMatrixKind::LuminanceToAlpha => {
            let mut rows = [0.0; 20];
            rows[15..18].copy_from_slice(&[0.2125, 0.7154, 0.0721]);
            rows
        }
    };
    color_filters::matrix_row_major(&rows, None)
}

