use crate::{
    animation::{Animation, TimeMap},
    shader::PaperPaint,
    style::{
        canvas_transform, perspective_matrix, resolve, shows_backface, CanvasTransform,
        LengthParts, Metrics, PaintStyle,
    },
};
use director_core::{element::TextShadow, Element, RenderError};
use director_scene::html::InlinePlaceholder;
use skia_safe::{color_filters, Canvas, Matrix, Rect, M44};
use std::any::Any;
use std::sync::{Arc, Mutex};
use taffy::{
    geometry::Size,
    style::{AvailableSpace, Style},
};

/// Font size, letter spacing, line height, word spacing and variable font axes.
type TextSize = (f32, f32, Option<f32>, f32, Option<Vec<director_core::element::FontVariation>>);

mod background;
mod borders;
mod text;

/// Origins a text paragraph lays out for its atomic inline boxes, relative to
/// their shared parent, by slot, and the live sizes animated boxes publish.
#[derive(Debug)]
pub struct InlineFlow {
    /// Names the flow in the scene document.
    pub id: u32,
    slots: Mutex<Vec<(f32, f32)>>,
    /// Each animated box's margin box this frame, by slot. `None` keeps the
    /// compiled placeholder.
    sizes: Mutex<Vec<Option<InlinePlaceholder>>>,
}
impl InlineFlow {
    pub fn new(id: u32) -> Arc<Self> {
        Arc::new(Self {
            id,
            slots: Mutex::new(Vec::new()),
            sizes: Mutex::new(Vec::new()),
        })
    }
}

/// How an atomic inline box's border box follows its live text and edges.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AtomicSizing {
    /// The author's width is `auto`, so the box is as wide as its text.
    pub auto_width: bool,
    /// The author's height is `auto`, so the box is as tall as its text.
    pub auto_height: bool,
    /// The author's lengths size the content box.
    pub content_box: bool,
}

/// A box's part in an inline flow.
#[derive(Debug, Clone)]
pub enum InlineLink {
    /// The text whose U+FFFC placeholders place the boxes.
    Text(Arc<InlineFlow>),
    /// The box on one slot.
    Box(Arc<InlineFlow>, usize, AtomicSizing),
}

/// A length margin in px. `auto` and percents are 0 on an inline box.
pub(crate) fn margin_px(margin: taffy::style::LengthPercentageAuto) -> f32 {
    let raw = margin.into_raw();
    if raw.tag() == taffy::style::CompactLength::LENGTH_TAG {
        raw.value()
    } else {
        0.0
    }
}

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
    pub inline: Option<InlineLink>,
    /// The clock of this box's descendants. `None` passes its time through.
    pub time_map: Option<TimeMap>,
    time: f64,
    baseline_offset: f32,
    /// The text a `text` track replaced, and the track text on screen.
    own_spans: Option<Vec<director_core::element::TextSpan>>,
    shown_text: Option<String>,
    /// The animated font size, letter spacing, line height and word spacing
    /// on screen. `None` is the base.
    shown_size: Option<TextSize>,
    /// An animated atomic box's text metrics, once measured.
    atomic_text: Option<Option<(f32, f32, f32)>>,
    /// The live placeholders the text paragraph holds. `None` is the base.
    held_boxes: Option<Vec<InlinePlaceholder>>,
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
            inline: None,
            time_map: None,
            time: 0.0,
            baseline_offset: 0.0,
            own_spans: None,
            shown_text: None,
            shown_size: None,
            atomic_text: None,
            held_boxes: None,
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
    /// The CSS baseline of a first line of `text` and the line height, from the font metrics.
    fn line_box(&self, text: &director_core::node::TextNode) -> Option<(f32, f32)> {
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
        let face = faces.first()?;
        let (_, metrics) = skia_safe::Font::from_typeface(face.clone(), self.paint.font_size).metrics();
        let ascent = (-metrics.ascent).round();
        let descent = metrics.descent.round();
        let height = self
            .paint
            .line_height
            .map(|h| self.paint.font_size * h)
            .unwrap_or(ascent + descent + metrics.leading.round());
        Some((ascent + (height - ascent - descent) / 2.0, height))
    }
    /// Max-content width, height and last-line baseline of text content, as an
    /// `inline-block` sizes it.
    pub(crate) fn inline_text_metrics(&self) -> Option<(f32, f32, f32)> {
        let text = self
            .content
            .as_ref()?
            .as_any()
            .downcast_ref::<director_core::node::TextNode>()?;
        let size = self.measure(
            Size {
                width: None,
                height: None,
            },
            Size {
                width: AvailableSpace::MaxContent,
                height: AvailableSpace::MaxContent,
            },
        );
        let (baseline, line) = self.line_box(text)?;
        let lines = text
            .paragraph
            .lock()
            .unwrap()
            .as_ref()
            .map_or(1, |p| p.line_number().max(1));
        Some((size.width, size.height, baseline + (lines - 1) as f32 * line))
    }
    /// Sizes an atomic inline box from its live text metrics, padding and
    /// border, as `size_atomic` did at compile time, and returns its margin box
    /// as the text line sees it.
    fn place_atomic(&mut self, sizing: AtomicSizing, text: Option<(f32, f32, f32)>) -> InlinePlaceholder {
        use taffy::style::{Dimension, LengthPercentage};
        let edges = |s: &Style| {
            let edge = |p: LengthPercentage, b: LengthPercentage| resolve(p, 0.0) + resolve(b, 0.0);
            let top = edge(s.padding.top, s.border.top);
            (
                edge(s.padding.left, s.border.left) + edge(s.padding.right, s.border.right),
                top + edge(s.padding.bottom, s.border.bottom),
                top,
            )
        };
        let (base_x, base_y, _) = edges(&self.base_style);
        let (x, y, top) = edges(&self.style);
        let px = |d: Dimension| d.into_option().unwrap_or(0.0);
        let axis = |current: Dimension, base: Dimension, auto: bool, content: Option<f32>, base_edges: f32, edges: f32| {
            if current != base {
                // A width or height track writes the author's length.
                px(current) + if sizing.content_box { edges } else { 0.0 }
            } else if auto {
                content.unwrap_or(px(base) - base_edges) + edges
            } else if sizing.content_box {
                px(base) - base_edges + edges
            } else {
                px(base)
            }
        };
        let width = axis(self.style.size.width, self.base_style.size.width, sizing.auto_width, text.map(|t| t.0), base_x, x);
        let height = axis(self.style.size.height, self.base_style.size.height, sizing.auto_height, text.map(|t| t.1), base_y, y);
        // A box without text sits on its bottom margin edge.
        let baseline = text.map_or(height, |t| top + t.2);
        let margin = self.style.margin.map(margin_px);
        self.style.size = Size {
            width: taffy::prelude::length(width),
            height: taffy::prelude::length(height),
        };
        InlinePlaceholder {
            width: width + margin.left + margin.right,
            height: height + margin.top + margin.bottom,
            baseline: margin.top + baseline,
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
    fn moves_between(&self, t0: f64, t1: f64) -> bool {
        let edge = |t: f64| t0 < t && t <= t1;
        let media = |t: f64| self.media_start + (t - self.start).max(0.0) * self.playback_rate;
        let (m0, m1) = (media(t0), media(t1));
        edge(self.start)
            || edge(self.end)
            || self.animations.iter().any(|animation| animation.moves_between(t0, t1))
            || self.content.as_ref().is_some_and(|content| content.moves_between(m0.min(m1), m0.max(m1)))
    }
    fn child_time(&self, time: f64) -> f64 {
        self.time_map.as_ref().map_or(time, |map| map.at(time))
    }
    fn child_time_range(&self, t0: f64, t1: f64) -> (f64, f64) {
        self.time_map.as_ref().map_or((t0, t1), |map| map.range(t0, t1))
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
        let outer = rect;
        self.baseline_offset = 0.0;
        let rect = self.content_rect(Rect::from_wh(rect.width(), rect.height()));
        if let Some(content) = self.content.as_ref() {
            if let Some(text) = content
                .as_any()
                .downcast_ref::<director_core::node::TextNode>()
            {
                if let Some((css_baseline, _)) = self.line_box(text) {
                    if let Some(paragraph) = text.paragraph.lock().unwrap().as_mut() {
                        paragraph.layout(rect.width());
                        self.baseline_offset = css_baseline - paragraph.alphabetic_baseline();
                    }
                }
            }
        }
        if let Some(content) = &mut self.content {
            content.post_layout(rect);
        }
        if let Some(InlineLink::Text(flow)) = &self.inline {
            let content = self.content_rect(Rect::from_wh(outer.width(), outer.height()));
            let text = self
                .content
                .as_ref()
                .and_then(|c| c.as_any().downcast_ref::<director_core::node::TextNode>());
            // Lay out at the width `TextNode::render` uses.
            let mut boxes = text
                .and_then(|text| {
                    let mut paragraph = text.paragraph.lock().unwrap();
                    paragraph.as_mut().map(|p| {
                        p.layout(content.width());
                        p.get_rects_for_placeholders()
                    })
                })
                .unwrap_or_default();
            // A `text-indent` placeholder comes first.
            boxes.drain(..boxes.len().saturating_sub(self.paint.placeholders.len()));
            *flow.slots.lock().unwrap() = boxes
                .iter()
                .map(|b| {
                    (
                        outer.left + content.left + b.rect.left,
                        outer.top + content.top + b.rect.top,
                    )
                })
                .collect();
        }
    }
    fn layout_origin(&self) -> Option<(f32, f32)> {
        let Some(InlineLink::Box(flow, slot, _)) = &self.inline else {
            return None;
        };
        let (x, y) = *flow.slots.lock().unwrap().get(*slot)?;
        // The placeholder holds the margin box.
        Some((x + margin_px(self.style.margin.left), y + margin_px(self.style.margin.top)))
    }
    fn update(&mut self, time: f64) -> bool {
        self.time = time;
        let old_style = self.style.clone();
        self.style = self.base_style.clone();
        self.paint = self.base_paint.clone();
        let mut parts = None;
        for animation in &self.animations {
            animation.apply(time, &mut self.style, &mut self.paint, &mut parts);
        }
        if let Some(parts) = parts {
            self.paint.transform = parts.compose();
        }
        if self.animations.iter().any(|a| a.properties().any(crate::animation::is_filter)) {
            self.paint.filter = crate::style::compile_filters(&self.paint.filters);
        }
        let mut relayout = false;
        if let Some(content) = &mut self.content {
            if let Some(text) = content
                .as_any_mut()
                .downcast_mut::<director_core::node::TextNode>()
            {
                let recolor = text.default_color.current_value != self.paint.color;
                if recolor {
                    text.default_color = director_core::animation::Animated::new(self.paint.color);
                }
                let text_size =
                    |p: &PaintStyle| (p.font_size, p.letter_spacing, p.line_height, p.word_spacing, p.span_variations());
                let (base, size) = (text_size(&self.base_paint), text_size(&self.paint));
                let sized = (size != base).then(|| size.clone());
                if self.paint.text != self.shown_text || sized != self.shown_size {
                    let own = self.own_spans.get_or_insert_with(|| text.spans.clone());
                    text.spans = match &self.paint.text {
                        Some(shown) => crate::compiler::text_spans(shown, &self.paint),
                        // Spans at the element's own size, spacings and line height
                        // inherit the animated ones; spans that set their own keep them.
                        None => own
                            .iter()
                            .cloned()
                            .map(|mut span| {
                                if span.font_size == Some(base.0) {
                                    span.font_size = Some(size.0);
                                }
                                if span.letter_spacing == Some(base.1) {
                                    span.letter_spacing = Some(size.1);
                                }
                                if span.line_height == base.2 {
                                    span.line_height = size.2;
                                }
                                if span.word_spacing == Some(base.3) {
                                    span.word_spacing = Some(size.3);
                                }
                                if span.font_variations == base.4 {
                                    span.font_variations = size.4.clone();
                                }
                                span
                            })
                            .collect(),
                    };
                    self.shown_text = self.paint.text.clone();
                    self.shown_size = sized;
                    relayout = true;
                }
                if recolor || relayout {
                    crate::compiler::prepare_paragraph(text, &self.paint);
                    self.held_boxes = None;
                }
                // Text shadows paint from the node at draw time.
                let same = |a: &TextShadow, b: &TextShadow| a.color == b.color && a.blur == b.blur && a.offset == b.offset;
                let shadows = &self.paint.text_shadows;
                if text.shadows.len() != shadows.len() || text.shadows.iter().zip(shadows).any(|(a, b)| !same(a, b)) {
                    text.shadows = shadows.clone();
                }
            }
            content.update(self.media_start + (time - self.start).max(0.0) * self.playback_rate);
        }
        // An animated atomic box publishes its live margin box for its text line.
        if let Some(InlineLink::Box(flow, slot, sizing)) = self.inline.clone() {
            if !self.animations.is_empty() {
                if relayout || self.atomic_text.is_none() {
                    self.atomic_text = Some(self.inline_text_metrics());
                }
                let placeholder = self.place_atomic(sizing, self.atomic_text.flatten());
                let mut sizes = flow.sizes.lock().unwrap();
                if sizes.len() <= slot {
                    sizes.resize(slot + 1, None);
                }
                sizes[slot] = Some(placeholder);
            }
        }
        relayout || self.style != old_style
    }
    fn pre_layout(&mut self) -> bool {
        let Some(InlineLink::Text(flow)) = &self.inline else {
            return false;
        };
        let boxes: Vec<InlinePlaceholder> = {
            let sizes = flow.sizes.lock().unwrap();
            if sizes.is_empty() {
                return false;
            }
            let base = &self.base_paint.placeholders;
            base.iter().enumerate().map(|(i, b)| sizes.get(i).copied().flatten().unwrap_or(*b)).collect()
        };
        let changed = *self.held_boxes.as_ref().unwrap_or(&self.base_paint.placeholders) != boxes;
        self.paint.placeholders = boxes;
        if changed {
            if let Some(text) = self
                .content
                .as_mut()
                .and_then(|c| c.as_any_mut().downcast_mut::<director_core::node::TextNode>())
            {
                crate::compiler::prepare_paragraph(text, &self.paint);
            }
            self.held_boxes = Some(self.paint.placeholders.clone());
        }
        changed
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
