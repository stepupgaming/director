use crate::{
    animation::Animation,
    shader::PaperPaint,
    style::{
        canvas_transform, perspective_matrix, resolve, shows_backface, CanvasTransform,
        LengthParts, Metrics, PaintStyle,
    },
};
use director_core::{Element, RenderError};
use skia_safe::{color_filters, Canvas, Matrix, Rect, M44};
use std::any::Any;
use taffy::{
    geometry::Size,
    style::{AvailableSpace, Style},
};

mod background;
mod borders;
mod text;

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
    fn ellipsizes(&self) -> bool {
        if self.paint.line_clamp.is_some() {
            return true;
        }
        self.paint.no_wrap
            && matches!(
                self.paint.text_overflow,
                crate::style::TextOverflow::Ellipsis
            )
            && self.style.overflow.x == taffy::style::Overflow::Hidden
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
        if !self.ellipsizes() && self.paint.no_wrap {
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
        } else if let Some(text) = &self.content {
            if let Some(node) = text
                .as_any()
                .downcast_ref::<director_core::node::TextNode>()
            {
                let used = crate::compiler::layout_paragraph(node, &self.paint, content.width());
                if self.paint.line_clamp.is_none() && used.is_finite() && used > content.width() {
                    // A wider paragraph keeps its alignment edge, so centred text stays centred.
                    let offset = match self.paint.align {
                        skia_safe::textlayout::TextAlign::Center => (content.width() - used) / 2.0,
                        skia_safe::textlayout::TextAlign::Right => content.width() - used,
                        _ => 0.0,
                    };
                    content = Rect::from_xywh(
                        content.left + offset,
                        content.top,
                        used,
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
    fn varies_with_time(&self) -> bool {
        self.content
            .as_ref()
            .is_some_and(|content| content.varies_with_time())
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
    fn export_layout_style(&self) -> Style {
        self.base_style.clone()
    }
    fn export_body(
        &self,
        cx: &mut director_core::scene_export::ExportContext,
    ) -> Result<director_core::scene_export::ElementExport, director_core::scene_export::ExportError> {
        crate::export::html_body(self, cx)
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
            let width = if self.paint.no_wrap {
                f32::INFINITY
            } else {
                known.width.unwrap_or(match available.width {
                    AvailableSpace::Definite(w) => w,
                    // A 1px min-content pass would collapse a soft break to one character.
                    AvailableSpace::MinContent if crate::style::soft_wrap(&self.paint) => {
                        f32::INFINITY
                    }
                    AvailableSpace::MinContent => 1.0,
                    AvailableSpace::MaxContent => f32::INFINITY,
                })
            };
            if !self.paint.no_wrap {
                crate::compiler::layout_paragraph(text, &self.paint, width);
            }
            if let Some(paragraph) = text.paragraph.lock().unwrap().as_mut() {
                if self.paint.no_wrap {
                    paragraph.layout(width);
                }
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
        let oz = self.paint.origin_z;
        let center = (width / 2.0, height / 2.0);
        let v = (center.0 - origin.0, center.1 - origin.1, -oz);
        match canvas_transform(&self.paint.transform, self.metrics, (width, height)) {
            CanvasTransform::Projective(matrix) => {
                matrix.rc(2, 0) * v.0
                    + matrix.rc(2, 1) * v.1
                    + matrix.rc(2, 2) * v.2
                    + matrix.rc(2, 3)
                    + oz
            }
            CanvasTransform::Affine(_) => 0.0,
        }
    }
    fn local_view_matrix(&self, width: f32, height: f32) -> Option<skia_safe::M44> {
        let origin_x = resolve(self.paint.origin.0, width);
        let origin_y = resolve(self.paint.origin.1, height);
        let transform = canvas_transform(&self.paint.transform, self.metrics, (width, height));
        let matrix = match transform {
            CanvasTransform::Affine(_) if self.paint.origin_z == 0.0 => return None,
            CanvasTransform::Affine(matrix) => m44_from_affine(&matrix),
            CanvasTransform::Projective(matrix) => matrix,
        };
        Some(origin_view_matrix(
            origin_x,
            origin_y,
            self.paint.origin_z,
            &matrix,
        ))
    }
    fn local_transform(&self, width: f32, height: f32) -> Option<Matrix> {
        // A 3D transform contributes its planar part; perspective is dropped.
        let matrix = match canvas_transform(&self.paint.transform, self.metrics, (width, height)) {
            CanvasTransform::Affine(matrix) => matrix,
            CanvasTransform::Projective(m) => Matrix::new_all(
                m.rc(0, 0),
                m.rc(0, 1),
                m.rc(0, 3),
                m.rc(1, 0),
                m.rc(1, 1),
                m.rc(1, 3),
                0.0,
                0.0,
                1.0,
            ),
        };
        if matrix.is_identity() {
            return None;
        }
        let origin = (
            resolve(self.paint.origin.0, width),
            resolve(self.paint.origin.1, height),
        );
        let mut about_origin = Matrix::translate(origin);
        about_origin.pre_concat(&matrix);
        about_origin.pre_translate((-origin.0, -origin.1));
        Some(about_origin)
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
        {
            return Ok(());
        }
        canvas.save();
        let origin = (
            resolve(self.paint.origin.0, rect.width()),
            resolve(self.paint.origin.1, rect.height()),
        );
        let size = (rect.width(), rect.height());
        let transform = canvas_transform(&self.paint.transform, self.metrics, size);
        if self.paint.origin_z == 0.0 {
            canvas.translate(origin);
            match transform {
                CanvasTransform::Affine(matrix) => {
                    canvas.concat(&matrix);
                }
                CanvasTransform::Projective(matrix) => {
                    canvas.concat_44(&matrix);
                }
            }
            canvas.translate((-origin.0, -origin.1));
        } else {
            canvas.concat_44(&skia_safe::M44::translate(origin.0, origin.1, self.paint.origin_z));
            match transform {
                CanvasTransform::Affine(matrix) => {
                    canvas.concat_44(&m44_from_affine(&matrix));
                }
                CanvasTransform::Projective(matrix) => {
                    canvas.concat_44(&matrix);
                }
            }
            canvas.concat_44(&skia_safe::M44::translate(
                -origin.0,
                -origin.1,
                -self.paint.origin_z,
            ));
        }
        // Facing comes from the matrix accumulated through preserve-3d ancestors,
        // not this element's own transform alone.
        if self.paint.backface_hidden && shows_backface(&canvas.local_to_device()) {
            canvas.restore();
            return Ok(());
        }
        if let Some(points) = &self.paint.box_node.clip_path {
            if let Some(first) = points.first() {
                let mut path = skia_safe::PathBuilder::new();
                path.move_to((first.0 * rect.width(), first.1 * rect.height()));
                for p in &points[1..] {
                    path.line_to((p.0 * rect.width(), p.1 * rect.height()));
                }
                path.close();
                canvas.clip_path(&path.detach(), skia_safe::ClipOp::Intersect, true);
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
        let radii = self.paint.corners.map(|(x, y)| {
            skia_safe::Point::new(resolve(x, rect.width()), resolve(y, rect.height()))
        });
        let outer = skia_safe::RRect::new_rect_radii(rect, &radii);
        background::paint_outer_shadows(canvas, &self.paint, outer);
        background::paint_background(canvas, self, rect, outer)?;
        background::paint_inset_shadows(canvas, &self.paint, rect, outer);
        canvas.save();
        if self.paint.box_node.overflow == "hidden" {
            canvas.clip_rrect(outer, skia_safe::ClipOp::Intersect, true);
        }
        let mut content_error = Ok(());
        if let Some(content) = &self.content {
            content_error = content.render(canvas, self.content_rect(rect), 1.0, &mut |_| {});
        }
        if content_error.is_ok() {
            text::paint_text_lines(canvas, self, rect);
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
        borders::paint_borders(canvas, rect, &self.paint, widths, &radii, outer);
        paint_mask(canvas, &self.paint, rect);
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

fn origin_view_matrix(origin_x: f32, origin_y: f32, origin_z: f32, matrix: &M44) -> M44 {
    let to_origin = M44::translate(origin_x, origin_y, origin_z);
    let from_origin = M44::translate(-origin_x, -origin_y, -origin_z);
    M44::concat(&M44::concat(&to_origin, matrix), &from_origin)
}

fn m44_from_affine(matrix: &Matrix) -> skia_safe::M44 {
    skia_safe::M44::row_major(&[
        matrix.scale_x(),
        matrix.skew_x(),
        0.0,
        matrix.translate_x(),
        matrix.skew_y(),
        matrix.scale_y(),
        0.0,
        matrix.translate_y(),
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ])
}

/// `mask-image` multiplies the painted layer by the mask's alpha or luminance.
fn paint_mask(canvas: &Canvas, paint: &PaintStyle, rect: Rect) {
    if let Some(mask) = &paint.mask {
        let mut p = skia_safe::Paint::default();
        p.set_blend_mode(skia_safe::BlendMode::DstIn);
        if let Some(shader) = mask.shader(rect) {
            p.set_shader(shader);
            if paint.mask_luminance {
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
}
