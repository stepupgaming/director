use crate::background_box::{place_axis, AxisRepeat};
use crate::background_tiles::{place_tiles, PlacedTile};
use crate::box_edges::{edge_insets, BoxEdge};
use crate::{
    animation::Animation,
    shader::PaperPaint,
    style::{
        canvas_transform, perspective_matrix, resolve, shows_backface, BackgroundFit,
        BackgroundOrigin, BorderLine, CanvasTransform, LengthParts, Metrics, PaintStyle,
    },
};
use director_core::{Element, RenderError};
use skia_safe::{
    color_filters, Canvas, FilterMode, Matrix, MipmapMode, Rect, SamplingOptions, TileMode,
};
use std::any::Any;
use taffy::{
    geometry::Size,
    style::{AvailableSpace, Style},
};

/// Native CSS grouping adapter. Content uses Director's existing text/image/SVG/video nodes.
#[derive(Debug, Clone)]
pub struct HtmlElement {
    pub style: Style,
    pub paint: PaintStyle,
    pub content: Option<Box<dyn Element>>,
    pub start: f64,
    pub end: f64,
    pub media_start: f64,
    pub playback_rate: f64,
    pub hidden: bool,
    pub metrics: Metrics,
    pub animations: Vec<Animation>,
    pub base_style: Style,
    pub base_paint: PaintStyle,
    pub paper: Option<PaperPaint>,
    time: f64,
    baseline_offset: f32,
}
impl HtmlElement {
    pub fn new(
        style: Style,
        paint: PaintStyle,
        content: Option<Box<dyn Element>>,
        metrics: Metrics,
    ) -> Self {
        Self {
            base_style: style.clone(),
            base_paint: paint.clone(),
            style,
            paint,
            content,
            metrics,
            start: 0.0,
            end: f64::INFINITY,
            media_start: 0.0,
            playback_rate: 1.0,
            hidden: false,
            animations: Vec::new(),
            paper: None,
            time: 0.0,
            baseline_offset: 0.0,
        }
    }
    fn content_rect(&self, rect: Rect) -> Rect {
        let left = resolve(self.style.padding.left, rect.width()) + self.style.border.left.value();
        let top = resolve(self.style.padding.top, rect.width()) + self.style.border.top.value();
        let right =
            resolve(self.style.padding.right, rect.width()) + self.style.border.right.value();
        let bottom =
            resolve(self.style.padding.bottom, rect.width()) + self.style.border.bottom.value();
        let mut content = Rect::from_xywh(
            left,
            top,
            (rect.width() - left - right).max(0.0),
            (rect.height() - top - bottom).max(0.0),
        );
        if self.paint.no_wrap {
            if let Some(text) = &self.content {
                if text.kind() == "text" {
                    let measured = text.measure(
                        Size {
                            width: None,
                            height: None,
                        },
                        Size {
                            width: AvailableSpace::MaxContent,
                            height: AvailableSpace::MaxContent,
                        },
                    );
                    let width = measured.width.max(content.width());
                    let offset = match self.paint.align {
                        skia_safe::textlayout::TextAlign::Center => (content.width() - width) / 2.0,
                        skia_safe::textlayout::TextAlign::Right => content.width() - width,
                        _ => 0.0,
                    };
                    content = Rect::from_xywh(
                        content.left + offset,
                        content.top,
                        width,
                        content.height(),
                    );
                }
            }
        }
        content.offset((0.0, self.baseline_offset));
        content
    }
}
impl Element for HtmlElement {
    fn kind(&self) -> &'static str {
        "html"
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn layout_style(&self) -> Style {
        self.style.clone()
    }
    fn set_layout_style(&mut self, style: Style) {
        self.base_style = style.clone();
        self.style = style;
    }
    fn needs_measure(&self) -> bool {
        self.content.as_ref().is_some_and(|c| c.needs_measure())
    }
    fn measure(&self, known: Size<Option<f32>>, available: Size<AvailableSpace>) -> Size<f32> {
        if let Some(text) = self
            .content
            .as_ref()
            .and_then(|c| c.as_any().downcast_ref::<director_core::node::TextNode>())
        {
            if let Some(paragraph) = text.paragraph.lock().unwrap().as_mut() {
                let width = if self.paint.no_wrap {
                    f32::INFINITY
                } else {
                    known.width.unwrap_or(match available.width {
                        AvailableSpace::Definite(w) => w,
                        AvailableSpace::MinContent => 1.0,
                        AvailableSpace::MaxContent => f32::INFINITY,
                    })
                };
                paragraph.layout(width);
                return Size {
                    width: known.width.unwrap_or(paragraph.longest_line()),
                    height: self
                        .paint
                        .line_height
                        .map(|height| {
                            self.paint.font_size * height * paragraph.line_number() as f32
                        })
                        .unwrap_or(paragraph.height()),
                };
            }
        }
        self.content
            .as_ref()
            .map(|c| c.measure(known, available))
            .unwrap_or(Size::ZERO)
    }
    fn post_layout(&mut self, rect: Rect) {
        self.baseline_offset = 0.0;
        let rect = self.content_rect(Rect::from_wh(rect.width(), rect.height()));
        if let Some(content) = self.content.as_ref() {
            if let Some(text) = content
                .as_any()
                .downcast_ref::<director_core::node::TextNode>()
            {
                let style = skia_safe::FontStyle::new(
                    skia_safe::font_style::Weight::from(self.paint.font_weight as i32),
                    skia_safe::font_style::Width::NORMAL,
                    if self.paint.font_style == "italic" {
                        skia_safe::font_style::Slant::Italic
                    } else {
                        skia_safe::font_style::Slant::Upright
                    },
                );
                let faces = text
                    .font_collection
                    .lock()
                    .unwrap()
                    .find_typefaces(&[self.paint.font_family.as_str()], style);
                if let Some(face) = faces.first() {
                    let (_, metrics) =
                        skia_safe::Font::from_typeface(face.clone(), self.paint.font_size)
                            .metrics();
                    if let Some(paragraph) = text.paragraph.lock().unwrap().as_mut() {
                        paragraph.layout(rect.width());
                        let ascent = (-metrics.ascent).round();
                        let descent = metrics.descent.round();
                        let height = self
                            .paint
                            .line_height
                            .map(|h| self.paint.font_size * h)
                            .unwrap_or(ascent + descent + metrics.leading.round());
                        let css_baseline = ascent + (height - ascent - descent) / 2.0;
                        self.baseline_offset = css_baseline - paragraph.alphabetic_baseline();
                    }
                }
            }
        }
        if let Some(content) = &mut self.content {
            content.post_layout(rect);
        }
    }
    fn update(&mut self, time: f64) -> bool {
        self.time = time;
        let old_style = self.style.clone();
        self.style = self.base_style.clone();
        self.paint = self.base_paint.clone();
        for animation in &self.animations {
            animation.apply(time, &mut self.style, &mut self.paint);
        }
        if let Some(content) = &mut self.content {
            if let Some(text) = content
                .as_any_mut()
                .downcast_mut::<director_core::node::TextNode>()
            {
                if text.default_color.current_value != self.paint.color {
                    text.default_color = director_core::animation::Animated::new(self.paint.color);
                    crate::compiler::prepare_paragraph(text, &self.paint);
                }
            }
            content.update(self.media_start + (time - self.start).max(0.0) * self.playback_rate);
        }
        self.style != old_style
    }
    fn shares_3d(&self) -> bool {
        self.paint.preserve_3d
    }
    fn view_depth(&self, width: f32, height: f32) -> f32 {
        let origin = (
            resolve(self.paint.origin.0, width),
            resolve(self.paint.origin.1, height),
        );
        let center = (width / 2.0, height / 2.0);
        let v = (center.0 - origin.0, center.1 - origin.1);
        match canvas_transform(&self.paint.transform, self.metrics, (width, height)) {
            CanvasTransform::Projective(matrix) => {
                matrix.rc(2, 0) * v.0 + matrix.rc(2, 1) * v.1 + matrix.rc(2, 3)
            }
            CanvasTransform::Affine(_) => 0.0,
        }
    }
    fn render(
        &self,
        canvas: &Canvas,
        rect: Rect,
        opacity: f32,
        draw_children: &mut dyn FnMut(&Canvas),
    ) -> Result<(), RenderError> {
        if self.hidden
            || !self.paint.visible
            || self.time < self.start
            || self.time >= self.end
            || self.paint.opacity <= 0.0
            || (self.paint.backface_hidden
                && shows_backface(
                    &self.paint.transform,
                    self.metrics,
                    (rect.width(), rect.height()),
                ))
        {
            return Ok(());
        }
        canvas.save();
        let origin = (
            resolve(self.paint.origin.0, rect.width()),
            resolve(self.paint.origin.1, rect.height()),
        );
        let size = (rect.width(), rect.height());
        canvas.translate(origin);
        match canvas_transform(&self.paint.transform, self.metrics, size) {
            CanvasTransform::Affine(matrix) => {
                canvas.concat(&matrix);
            }
            CanvasTransform::Projective(matrix) => {
                canvas.concat_44(&matrix);
            }
        }
        canvas.translate((-origin.0, -origin.1));
        if let Some(points) = &self.paint.box_node.clip_path {
            if let Some(first) = points.first() {
                let mut path = skia_safe::Path::new();
                path.move_to((first.0 * rect.width(), first.1 * rect.height()));
                for p in &points[1..] {
                    path.line_to((p.0 * rect.width(), p.1 * rect.height()));
                }
                path.close();
                canvas.clip_path(&path, skia_safe::ClipOp::Intersect, true);
            }
        }
        let mut layer_paint = skia_safe::Paint::default();
        layer_paint.set_alpha_f(self.paint.opacity * opacity);
        layer_paint.set_blend_mode(self.paint.blend_mode);
        layer_paint.set_image_filter(self.paint.filter.clone());
        let layered = self.paint.opacity * opacity < 1.0
            || self.paint.filter.is_some()
            || self.paint.mask.is_some()
            || self.paint.blend_mode != skia_safe::BlendMode::SrcOver;
        if layered {
            canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default().paint(&layer_paint));
        }
        let mut bg = self.paint.box_node.clone();
        bg.clip_path = None;
        bg.overflow = "visible".into();
        bg.border_width = director_core::animation::Animated::new(0.0);
        let radii = self.paint.corners.map(|(x, y)| {
            skia_safe::Point::new(resolve(x, rect.width()), resolve(y, rect.height()))
        });
        let outer = skia_safe::RRect::new_rect_radii(rect, &radii);
        for shadow in self.paint.shadows.iter().rev().filter(|s| !s.inset) {
            let mut paint = skia_safe::Paint::default();
            paint.set_color4f(shadow.color.to_color4f(), None);
            paint.set_anti_alias(true);
            paint.set_image_filter(skia_safe::image_filters::blur(
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
            canvas.draw_rrect(rr, &paint);
            canvas.restore();
        }
        // Background is clipped to individual corner radii; content clipping follows overflow.
        canvas.save();
        canvas.clip_rrect(outer, skia_safe::ClipOp::Intersect, true);
        if let Some(clip) = content_box_background_clip(rect, &self.style, self.paint.background_clip)
        {
            canvas.clip_rect(clip, skia_safe::ClipOp::Intersect, false);
        }
        bg.render(canvas, rect, 1.0, &mut |_| {})?;
        for gradient in self.paint.gradient.iter().rev() {
            if let Some(shader) = gradient.shader(rect) {
                let mut p = skia_safe::Paint::default();
                p.set_shader(shader);
                p.set_anti_alias(true);
                canvas.draw_rect(rect, &p);
            }
        }
        paint_background_image(canvas, rect, &self.style, &self.paint);
        canvas.restore();
        for shadow in self.paint.shadows.iter().rev().filter(|s| s.inset) {
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
            let mut path = skia_safe::Path::new();
            path.set_fill_type(skia_safe::PathFillType::EvenOdd);
            path.add_rect(coverage, None);
            path.add_rrect(hole, None);
            let mut paint = skia_safe::Paint::default();
            paint.set_color4f(shadow.color.to_color4f(), None);
            paint.set_anti_alias(true);
            paint.set_image_filter(skia_safe::image_filters::blur(
                (shadow.blur, shadow.blur),
                skia_safe::TileMode::Decal,
                None,
                None,
            ));
            canvas.draw_path(&path, &paint);
            canvas.restore();
        }
        canvas.save();
        if self.paint.box_node.overflow == "hidden" {
            canvas.clip_rrect(outer, skia_safe::ClipOp::Intersect, true);
        }
        if let Some(points) = &bg.clip_path {
            let mut path = skia_safe::Path::new();
            if let Some(first) = points.first() {
                path.move_to((first.0 * rect.width(), first.1 * rect.height()));
                for p in &points[1..] {
                    path.line_to((p.0 * rect.width(), p.1 * rect.height()));
                }
                path.close();
                canvas.clip_path(&path, skia_safe::ClipOp::Intersect, true);
            }
        }
        let mut content_error = Ok(());
        if let Some(content) = &self.content {
            content_error = content.render(canvas, self.content_rect(rect), 1.0, &mut |_| {});
        }
        if content_error.is_ok() {
            paint_text_lines(canvas, self, rect);
        }
        if content_error.is_ok() {
            if let Some(paper) = &self.paper {
                content_error = paper.draw(canvas, self.content_rect(rect));
            }
        }
        if content_error.is_ok() {
            if let Some(distance) = self.paint.perspective {
                let origin = (
                    resolve(self.paint.perspective_origin.0, rect.width()),
                    resolve(self.paint.perspective_origin.1, rect.height()),
                );
                canvas.save();
                canvas.translate(origin);
                canvas.concat_44(&perspective_matrix(distance));
                canvas.translate((-origin.0, -origin.1));
                draw_children(canvas);
                canvas.restore();
            } else {
                draw_children(canvas);
            }
        }
        canvas.restore();
        let widths = [
            self.style.border.top.value(),
            self.style.border.right.value(),
            self.style.border.bottom.value(),
            self.style.border.left.value(),
        ];
        paint_borders(canvas, rect, &self.paint, widths, &radii, outer);
        if let Some(mask) = &self.paint.mask {
            let mut p = skia_safe::Paint::default();
            p.set_blend_mode(skia_safe::BlendMode::DstIn);
            if let Some(shader) = mask.shader(rect) {
                p.set_shader(shader);
                if self.paint.mask_luminance {
                    let matrix = [
                        0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                        0.2126, 0.7152, 0.0722, 0.0, 0.0,
                    ];
                    p.set_color_filter(color_filters::matrix_row_major(
                        &matrix,
                        color_filters::Clamp::Yes,
                    ));
                }
                canvas.draw_rect(rect, &p);
            }
        }
        if layered {
            canvas.restore();
        }
        canvas.restore();
        content_error
    }
    fn animate_property(&mut self, _: &str, _: f32, _: f32, _: f64, _: &str) -> Result<(), String> {
        Err("HTML animations are authored in CSS or captured at compile time".into())
    }
    fn set_opacity(&mut self, opacity: f32) {
        self.paint.opacity = opacity;
    }
    fn get_audio(&self, time: f64, n: usize, rate: u32) -> Option<Vec<f32>> {
        if time < self.start || time >= self.end {
            return None;
        }
        self.content.as_ref()?.get_audio(
            self.media_start + (time - self.start) * self.playback_rate,
            n,
            rate,
        )
    }
}

fn paint_borders(
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
        && lines[0] != BorderLine::Solid
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
        let mut path = skia_safe::Path::new();
        path.move_to(points[0]);
        for p in &points[1..] {
            path.line_to(*p);
        }
        path.close();
        let mut p = skia_safe::Paint::default();
        p.set_anti_alias(true);
        p.set_color4f(paint.border_colors[i].to_color4f(), None);
        canvas.draw_path(&path, &p);
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
        if widths[i] <= 0.0 || lines[i] == BorderLine::Solid {
            continue;
        }
        let mut path = skia_safe::Path::new();
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
        canvas.draw_path(&path, &p);
    }
    canvas.restore();
}

fn content_box_background_clip(rect: Rect, layout: &Style, clip: BoxEdge) -> Option<Rect> {
    if clip != BoxEdge::ContentBox {
        return None;
    }
    let width = rect.width();
    let insets = edge_insets(
        BoxEdge::ContentBox,
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
    Some(Rect::new(left, top, right, bottom))
}

fn paint_background_image(canvas: &Canvas, rect: Rect, layout: &Style, paint: &PaintStyle) {
    let Some(image) = &paint.background_image else {
        return;
    };
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
    let (w, h) = fitted_background(paint.background_size, iw, ih, area_w, area_h);
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let (repeat_x, repeat_y) = paint.background_repeat;
    let shader_repeat = matches!(repeat_x, AxisRepeat::Repeat | AxisRepeat::NoRepeat)
        && matches!(repeat_y, AxisRepeat::Repeat | AxisRepeat::NoRepeat);
    if !shader_repeat {
        paint_placed_tiles(
            canvas, image, rect, inset_l, inset_t, area_w, area_h, w, h, paint,
        );
        return;
    }
    let x = rect.left + inset_l + paint.background_position.0.percent * (area_w - w)
        + paint.background_position.0.length;
    let y = rect.top + inset_t + paint.background_position.1.percent * (area_h - h)
        + paint.background_position.1.length;
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
    paint: &PaintStyle,
) {
    let tiles = explicit_background_tiles(
        area_w,
        area_h,
        tile_w,
        tile_h,
        paint.background_position.0.percent,
        paint.background_position.0.length,
        paint.background_position.1.percent,
        paint.background_position.1.length,
        paint.background_repeat.0,
        paint.background_repeat.1,
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

fn paint_text_lines(canvas: &Canvas, element: &HtmlElement, rect: Rect) {
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
    let thickness = element.paint.decoration_thickness.unwrap_or_else(|| {
        (element.paint.font_size / 16.0).max(1.0).round()
    });
    let spans = {
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
        spans
    };
    let mut paint = skia_safe::Paint::default();
    paint.set_anti_alias(false);
    paint.set_color4f(element.paint.color.to_color4f(), None);
    for (x, w, under, over, strike) in spans {
        if lines.underline {
            fill_decoration(
                canvas,
                &paint,
                element.paint.decoration_style,
                x,
                under,
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
                let mut path = skia_safe::Path::new();
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
                canvas.draw_path(&path, &stroke);
            }
        }
    }
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
