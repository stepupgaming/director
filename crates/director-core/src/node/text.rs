use crate::animation::Animated;
use crate::element::{Element, TextFit, TextShadow, TextSpan};
use crate::node::text_animator::{
    parse_text_anim_property, GlyphAnimState, TextAnimProperty, TextAnimator,
};
use crate::types::Color;
use skia_safe::{
    color_filters,
    image_filters,
    textlayout::{
        FontCollection, Paragraph, ParagraphBuilder, ParagraphStyle, RectHeightStyle,
        RectWidthStyle, StrutStyle, TextAlign, TextDirection, TextHeightBehavior, TextStyle,
    },
    Canvas, Paint, PaintStyle, Rect, TileMode,
};
use std::any::Any;
use std::fmt;
use std::sync::{Arc, Mutex};
use taffy::geometry::Size;
use taffy::style::{AvailableSpace, Style};
use tracing::warn;

/// Per-glyph position data extracted from SkParagraph.
#[derive(Clone, Debug)]
pub struct GlyphInfo {
    /// Character index in the source text
    pub char_index: usize,
    /// Bounding rectangle for this glyph
    pub rect: Rect,
    /// Text direction (LTR or RTL)
    pub direction: TextDirection,
}

/// A node for rendering rich text using Skia's native text layout engine (SkParagraph).
pub struct TextNode {
    pub spans: Vec<TextSpan>,
    pub default_font_size: Animated<f32>,
    pub default_color: Animated<Color>,
    pub opacity: Animated<f32>,

    // Skia Text Layout
    pub paragraph: Mutex<Option<Paragraph>>,
    pub font_collection: Arc<Mutex<FontCollection>>,

    // Layout & Styling
    pub fit_mode: TextFit,
    pub min_size: f32,
    pub max_size: f32,
    pub shadow: Option<TextShadow>,
    pub dirty_layout: bool,
    pub last_layout_rect: Rect,
    pub style: Style,

    // Text Alignment
    pub text_align: TextAlign,

    // Kinetic Typography
    /// Active per-glyph animators
    pub animators: Vec<TextAnimator>,
    /// Current animation time (set by update(), used by render())
    pub current_time: f64,
}

impl Clone for TextNode {
    fn clone(&self) -> Self {
        Self {
            spans: self.spans.clone(),
            default_font_size: self.default_font_size.clone(),
            default_color: self.default_color.clone(),
            opacity: self.opacity.clone(),
            paragraph: Mutex::new(None), // Must rebuild
            font_collection: self.font_collection.clone(),
            fit_mode: self.fit_mode.clone(),
            min_size: self.min_size,
            max_size: self.max_size,
            shadow: self.shadow.clone(),
            dirty_layout: true,
            last_layout_rect: Rect::default(),
            style: self.style.clone(),
            text_align: self.text_align,
            animators: self.animators.clone(),
            current_time: self.current_time,
        }
    }
}

impl fmt::Debug for TextNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextNode")
            .field("spans", &self.spans)
            .field("fit_mode", &self.fit_mode)
            .finish()
    }
}

impl TextNode {
    pub fn new(spans: Vec<TextSpan>, font_collection: Arc<Mutex<FontCollection>>) -> Self {
        let mut node = Self {
            spans,
            default_font_size: Animated::new(20.0),
            default_color: Animated::new(Color::WHITE),
            opacity: Animated::new(1.0),
            paragraph: Mutex::new(None),
            font_collection,
            fit_mode: TextFit::None,
            min_size: 10.0,
            max_size: 200.0,
            shadow: None,
            dirty_layout: true,
            last_layout_rect: Rect::default(),
            style: Style::default(),
            text_align: TextAlign::Left,
            animators: Vec::new(),
            current_time: 0.0,
        };
        node.init_paragraph();
        node
    }

    /// Builds the paragraph using current spans and style
    fn build_paragraph(&self) -> Paragraph {
        let mut paragraph_style = ParagraphStyle::new();
        paragraph_style.set_text_align(self.text_align);
        paragraph_style.set_text_direction(TextDirection::LTR);

        // --- Fix for Vertical Centering ---
        // 1. Disable "first ascent" which adds extra padding at the top
        paragraph_style.set_text_height_behavior(TextHeightBehavior::DisableAll);

        // 2. Configure StrutStyle to enforce consistent line heights
        let mut strut_style = StrutStyle::new();
        let default_font_families = vec!["Sans Serif"]; // Fallback

        // We use the default font size for the strut, or a fallback of 20.0 if not set.
        let font_size = self.default_font_size.current_value;

        strut_style.set_font_families(&default_font_families);
        strut_style.set_font_size(font_size);
        strut_style.set_height(1.2); // 1.2 Multiplier for nice leading
        strut_style.set_leading(0.0);
        strut_style.set_strut_enabled(true);
        strut_style.set_force_strut_height(true);
        strut_style.set_height_override(true); // Ensures the multiplier is strictly obeyed

        paragraph_style.set_strut_style(strut_style);
        // ----------------------------------

        let font_collection_guard = match self.font_collection.lock() {
            Ok(g) => g,
            Err(_) => {
                // Use fallback — build without custom font collection
                let mut builder = ParagraphBuilder::new(
                    &ParagraphStyle::new(),
                    &skia_safe::textlayout::FontCollection::new(),
                );
                for span in &self.spans {
                    builder.add_text(&span.text);
                }
                return builder.build();
            }
        };
        let mut builder = ParagraphBuilder::new(&paragraph_style, &*font_collection_guard);

        for span in &self.spans {
            let mut text_style = TextStyle::new();

            // 1. Font Size
            let size = span
                .font_size
                .unwrap_or(self.default_font_size.current_value);
            text_style.set_font_size(size);

            // 2. Font Family
            let mut families = vec![];
            if let Some(f) = &span.font_family {
                families.push(f.as_str());
            } else {
                families.push("Sans Serif");
                families.push("Arial");
            }
            text_style.set_font_families(&families);

            // 3. Color (Foreground) or Gradient Fill
            if let Some(gradient) = &span.fill_gradient {
                let skia_colors: Vec<skia_safe::Color> =
                    gradient.colors.iter().map(|c| c.to_skia()).collect();
                // Gradient coordinates will be remapped to the text layout rect at render time.
                // We build with placeholder coordinates here and apply the actual gradient
                // via a post-render overlay using SrcIn blend mode.
                let mut paint = Paint::default();
                paint.set_anti_alias(true);
                paint.set_color(self.default_color.current_value.to_skia());
                text_style.set_foreground_paint(&paint);
            } else {
                let color = span.color.unwrap_or(self.default_color.current_value);
                let mut paint = Paint::default();
                paint.set_color(color.to_skia());
                paint.set_anti_alias(true);
                text_style.set_foreground_paint(&paint);
            }

            // 4. Background
            if let Some(bg_color) = span.background_color {
                let mut bg_paint = Paint::default();
                bg_paint.set_color(bg_color.to_skia());
                bg_paint.set_anti_alias(true);
                text_style.set_background_paint(&bg_paint);
            }

            // 5. Weight / Slant
            if let Some(w) = span.font_weight {
                let weight = skia_safe::font_style::Weight::from(w as i32);
                let slant = if span.font_style.as_deref() == Some("italic") {
                    skia_safe::font_style::Slant::Italic
                } else {
                    skia_safe::font_style::Slant::Upright
                };
                text_style.set_font_style(skia_safe::FontStyle::new(
                    weight,
                    skia_safe::font_style::Width::NORMAL,
                    slant,
                ));
            }

            // 6. Stroke (Rich Text)
            if let Some(sw) = span.stroke_width {
                if sw > 0.0 {
                    let mut stroke_paint = Paint::default();
                    stroke_paint.set_style(PaintStyle::Stroke);
                    stroke_paint.set_stroke_width(sw);
                    stroke_paint.set_anti_alias(true);
                    if let Some(sc) = span.stroke_color {
                        stroke_paint.set_color(sc.to_skia());
                    } else {
                        stroke_paint.set_color(skia_safe::Color::BLACK);
                    }
                    text_style.set_foreground_paint(&stroke_paint);
                }
            }

            builder.push_style(&text_style);
            builder.add_text(&span.text);
            builder.pop();
        }

        builder.build()
    }

    pub fn init_paragraph(&mut self) {
        if let Ok(mut p) = self.paragraph.lock() {
            *p = Some(self.build_paragraph());
        }
        self.dirty_layout = true;
    }

    pub fn set_text_align(&mut self, align: TextAlign) {
        self.text_align = align;
        if let Ok(mut p) = self.paragraph.lock() {
            *p = Some(self.build_paragraph());
        }
        self.dirty_layout = true;
    }

    pub fn ensure_paragraph_ready(&self) {
        if let Ok(p_guard) = self.paragraph.lock() {
            if p_guard.is_none() {
                drop(p_guard);
                if let Ok(mut p_guard) = self.paragraph.lock() {
                    *p_guard = Some(self.build_paragraph());
                }
            }
        }
    }

    /// Returns the total number of characters across all spans.
    pub fn get_total_char_count(&self) -> usize {
        self.spans.iter().map(|s| s.text.chars().count()).sum()
    }

    /// Extracts per-glyph bounding boxes from the laid-out paragraph.
    ///
    /// Must be called after layout to get accurate positions.
    pub fn get_glyph_rects(&self, layout_width: f32) -> Vec<GlyphInfo> {
        self.ensure_paragraph_ready();
        let mut p_guard = match self.paragraph.lock() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let Some(paragraph) = p_guard.as_mut() else {
            return Vec::new();
        };

        // Ensure paragraph is laid out
        paragraph.layout(layout_width);

        let char_count = self.get_total_char_count();
        let mut glyphs = Vec::with_capacity(char_count);

        for i in 0..char_count {
            // Get bounding box for this single character
            let text_boxes = paragraph.get_rects_for_range(
                i..i + 1,
                RectHeightStyle::Tight,
                RectWidthStyle::Tight,
            );

            if let Some(text_box) = text_boxes.first() {
                glyphs.push(GlyphInfo {
                    char_index: i,
                    rect: text_box.rect,
                    direction: text_box.direct,
                });
            }
        }

        glyphs
    }

    /// Computes the animated state for all glyphs at the given time.
    pub fn compute_glyph_states(&self, time: f64) -> Vec<GlyphAnimState> {
        let char_count = self.get_total_char_count();
        let mut states: Vec<GlyphAnimState> = (0..char_count)
            .map(|_| GlyphAnimState::identity())
            .collect();

        for animator in &self.animators {
            // Apply this animator to affected glyphs
            for char_idx in animator.start_idx..animator.end_idx.min(char_count) {
                let glyph_offset = char_idx - animator.start_idx;
                let value = animator.value_for_glyph(glyph_offset, time);

                match animator.property {
                    TextAnimProperty::Opacity => states[char_idx].opacity = value,
                    TextAnimProperty::OffsetX => states[char_idx].offset_x = value,
                    TextAnimProperty::OffsetY => states[char_idx].offset_y = value,
                    TextAnimProperty::Scale => states[char_idx].scale = value,
                    TextAnimProperty::Rotation => states[char_idx].rotation = value,
                }
            }
        }

        states
    }

    /// Renders text with per-glyph animations applied.
    ///
    /// This is slower than the standard paragraph.paint() but enables
    /// kinetic typography effects like karaoke and wave animations.
    fn render_per_glyph(
        &self,
        canvas: &Canvas,
        rect: Rect,
        base_opacity: f32,
        paragraph: &mut Paragraph,
    ) {
        // Get glyph positions
        let char_count = self.get_total_char_count();
        if char_count == 0 {
            return;
        }

        // Compute animated states for all glyphs
        let states = self.compute_glyph_states(self.current_time);

        // Get the full text for drawing
        let full_text: String = self.spans.iter().map(|s| s.text.as_str()).collect();
        let chars: Vec<char> = full_text.chars().collect();

        // Pre-build shadow image filter and color filter if a shadow is configured.
        // These are reference-counted in Skia so reusing them per-glyph is cheap.
        let shadow_filter = self.shadow.as_ref().map(|shadow| {
            let blur = image_filters::blur(
                (shadow.blur, shadow.blur),
                TileMode::Decal,
                None,
                None,
            );
            let c = shadow.color;
            let matrix: [f32; 20] = [
                0.0, 0.0, 0.0, 0.0, c.r,
                0.0, 0.0, 0.0, 0.0, c.g,
                0.0, 0.0, 0.0, 0.0, c.b,
                0.0, 0.0, 0.0, 1.0, 0.0,
            ];
            let cf = color_filters::matrix_row_major(&matrix, color_filters::Clamp::Yes);
            (blur, cf, shadow.offset, shadow.blur, c.a)
        });

        // Get glyph rects
        for (i, _ch) in chars.iter().enumerate() {
            let state = states
                .get(i)
                .cloned()
                .unwrap_or_else(GlyphAnimState::identity);

            // Skip invisible glyphs
            if state.opacity <= 0.001 {
                continue;
            }

            // Get bounding box for this character
            let text_boxes = paragraph.get_rects_for_range(
                i..i + 1,
                RectHeightStyle::Tight,
                RectWidthStyle::Tight,
            );

            let Some(text_box) = text_boxes.first() else {
                continue;
            };

            let glyph_rect = text_box.rect;
            let center_x = glyph_rect.left + glyph_rect.width() / 2.0;
            let center_y = glyph_rect.top + glyph_rect.height() / 2.0;

            canvas.save();

            // Apply transforms relative to glyph center
            canvas.translate((
                rect.x() + center_x + state.offset_x,
                rect.y() + center_y + state.offset_y,
            ));

            if (state.rotation).abs() > 0.001 {
                canvas.rotate(state.rotation, None);
            }

            if (state.scale - 1.0).abs() > 0.001 {
                canvas.scale((state.scale, state.scale));
            }

            // Move back from center
            canvas.translate((-center_x, -center_y));

            // Draw per-glyph shadow if configured
            if let Some((blur, cf, offset, blur_radius, shadow_alpha)) = &shadow_filter {
                let glyph_opacity = base_opacity * state.opacity;
                if glyph_opacity > 0.001 {
                    // Inverse-transform the shadow offset so it stays in screen-space
                    // regardless of glyph rotation/scale.
                    let (shadow_dx, shadow_dy) =
                        if (state.rotation).abs() > 0.001 || (state.scale - 1.0).abs() > 0.001 {
                            let rad = -state.rotation.to_radians();
                            let cos_r = rad.cos();
                            let sin_r = rad.sin();
                            let inv_scale = 1.0 / state.scale.abs().max(0.001);
                            let dx = (offset.0 * cos_r - offset.1 * sin_r) * inv_scale;
                            let dy = (offset.0 * sin_r + offset.1 * cos_r) * inv_scale;
                            (dx, dy)
                        } else {
                            (offset.0, offset.1)
                        };

                    let mut shadow_paint = Paint::default();
                    shadow_paint.set_alpha_f(glyph_opacity * shadow_alpha);
                    shadow_paint.set_image_filter(blur.clone());
                    shadow_paint.set_color_filter(cf.clone());

                    let margin = blur_radius * 4.0;
                    let shadow_bounds = Rect::from_xywh(
                        glyph_rect.left - margin + shadow_dx.min(0.0),
                        glyph_rect.top - margin + shadow_dy.min(0.0),
                        glyph_rect.width() + margin * 2.0 + shadow_dx.abs(),
                        glyph_rect.height() + margin * 2.0 + shadow_dy.abs(),
                    );

                    canvas.save_layer(
                        &skia_safe::canvas::SaveLayerRec::default()
                            .bounds(&shadow_bounds)
                            .paint(&shadow_paint),
                    );
                    canvas.clip_rect(glyph_rect, skia_safe::ClipOp::Intersect, true);
                    paragraph.paint(canvas, (shadow_dx, shadow_dy));
                    canvas.restore(); // shadow layer
                }
            }

            // Create paint with combined opacity
            let final_opacity = base_opacity * state.opacity;
            if final_opacity < 1.0 {
                let mut paint = Paint::default();
                paint.set_alpha_f(final_opacity);

                // Draw single character paragraph
                // We use a layer for opacity
                let layer_bounds = Rect::from_xywh(
                    glyph_rect.left - 5.0,
                    glyph_rect.top - 5.0,
                    glyph_rect.width() + 10.0,
                    glyph_rect.height() + 10.0,
                );
                canvas.save_layer(
                    &skia_safe::canvas::SaveLayerRec::default()
                        .bounds(&layer_bounds)
                        .paint(&paint),
                );

                // Paint the full paragraph but clipped to this character
                // Note: This is a workaround - ideally we'd draw just one glyph
                canvas.clip_rect(glyph_rect, skia_safe::ClipOp::Intersect, true);
                paragraph.paint(canvas, (0.0, 0.0));

                canvas.restore(); // layer
            } else {
                canvas.clip_rect(glyph_rect, skia_safe::ClipOp::Intersect, true);
                paragraph.paint(canvas, (0.0, 0.0));
            }

            canvas.restore();
        }
    }
}

impl Element for TextNode {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn needs_measure(&self) -> bool {
        true
    }

    fn measure(
        &self,
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
    ) -> Size<f32> {
        self.ensure_paragraph_ready();
        let mut p_guard = match self.paragraph.lock() {
            Ok(g) => g,
            Err(_) => return Size { width: 0.0, height: 0.0 },
        };

        if let Some(paragraph) = p_guard.as_mut() {
            // Determine constraint
            let width_opt = match available_space.width {
                AvailableSpace::Definite(w) => w,
                AvailableSpace::MinContent => 1.0,
                AvailableSpace::MaxContent => f32::INFINITY,
            };

            let layout_width = known_dimensions.width.unwrap_or(width_opt);

            paragraph.layout(layout_width);

            let width = if known_dimensions.width.is_some() {
                layout_width
            } else {
                paragraph.longest_line()
            };

            Size {
                width: width.ceil(),
                height: paragraph.height().ceil(),
            }
        } else {
            Size::ZERO
        }
    }

    fn layout_style(&self) -> Style {
        self.style.clone()
    }

    fn update(&mut self, time: f64) -> bool {
        // Store time for render to use
        self.current_time = time;

        let old_size = self.default_font_size.current_value;
        let old_color = self.default_color.current_value;

        self.default_font_size.update(time);
        self.default_color.update(time);
        self.opacity.update(time);

        let mut changed = false;

        // Check if visual properties changed
        if (self.default_font_size.current_value - old_size).abs() > 0.001 {
            changed = true;
        }

        // Color::PartialEq is derived, compares float fields exactly.
        if self.default_color.current_value != old_color {
            changed = true;
        }

        // If we have animators, always mark as changed during animation
        if !self.animators.is_empty() {
            changed = true;
        }

        if changed {
            // Rebuild paragraph
            self.init_paragraph();
        }

        changed
    }

    fn post_layout(&mut self, rect: Rect) {
        if self.fit_mode == TextFit::None && !self.dirty_layout {
            return;
        }
        if self.last_layout_rect == rect && !self.dirty_layout {
            return;
        }

        self.last_layout_rect = rect;

        if self.fit_mode == TextFit::Shrink {
            let target_width = rect.width();
            let target_height = rect.height();

            let mut low = self.min_size;
            let mut high = self.max_size;
            let mut best_size = self.min_size;

            for _ in 0..5 {
                let mid = (low + high) / 2.0;

                self.default_font_size.current_value = mid;
                self.init_paragraph();

                let mut p_guard = match self.paragraph.lock() {
                    Ok(g) => g,
                    Err(_) => continue,
                };
                if let Some(p) = p_guard.as_mut() {
                    p.layout(target_width);
                    if p.height() <= target_height && p.longest_line() <= target_width + 1.0 {
                        best_size = mid;
                        low = mid;
                    } else {
                        high = mid;
                    }
                }
            }

            self.default_font_size.current_value = best_size;
            self.init_paragraph();
        }

        self.dirty_layout = false;
    }

    fn render(
        &self,
        canvas: &Canvas,
        rect: Rect,
        opacity: f32,
        draw_children: &mut dyn FnMut(&Canvas),
    ) -> Result<(), crate::RenderError> {
        let mut p_guard = match self.paragraph.lock() {
            Ok(g) => g,
            Err(_) => return Ok(()),
        };
        if let Some(paragraph) = p_guard.as_mut() {
            // Prevent SkParagraph from wrapping character-by-character at near-zero width.
            // If the layout rect is narrower than a single glyph, use natural (unwrapped) width.
            let layout_width = if rect.width() < 1.0 {
                f32::INFINITY
            } else {
                rect.width()
            };
            paragraph.layout(layout_width);

            canvas.save();

            let effective_opacity = self.opacity.current_value * opacity;

            // Draw shadow behind the text (full paragraph, not per-glyph).
            // Skip shadow when kinetic typography animators are active —
            // a static full-paragraph shadow behind individually-moving glyphs
            // creates a distracting visual disconnect.
            if self.animators.is_empty() {
                if let Some(shadow) = &self.shadow {
                    let mut shadow_paint = Paint::default();
                    shadow_paint.set_alpha_f(effective_opacity * shadow.color.a);
                    shadow_paint.set_image_filter(image_filters::blur(
                        (shadow.blur, shadow.blur),
                        TileMode::Decal,
                        None,
                        None,
                    ));

                    // Color matrix that replaces all source colors with the shadow color
                    // while preserving the original alpha shape.
                    let c = shadow.color;
                    let matrix: [f32; 20] = [
                        0.0, 0.0, 0.0, 0.0, c.r,
                        0.0, 0.0, 0.0, 0.0, c.g,
                        0.0, 0.0, 0.0, 0.0, c.b,
                        0.0, 0.0, 0.0, 1.0, 0.0,
                    ];
                    let cf = color_filters::matrix_row_major(&matrix, color_filters::Clamp::Yes);
                    shadow_paint.set_color_filter(cf);

                // Expand bounds so the blur isn't clipped
                let blur_margin = shadow.blur * 4.0;
                    let shadow_bounds = Rect::from_xywh(
                        rect.left - blur_margin + shadow.offset.0.min(0.0),
                        rect.top - blur_margin + shadow.offset.1.min(0.0),
                        rect.width() + blur_margin * 2.0 + shadow.offset.0.abs(),
                        rect.height() + blur_margin * 2.0 + shadow.offset.1.abs(),
                    );

                    canvas.save_layer(
                        &skia_safe::canvas::SaveLayerRec::default()
                            .bounds(&shadow_bounds)
                            .paint(&shadow_paint),
                    );
                    paragraph.paint(
                        canvas,
                        (rect.x() + shadow.offset.0, rect.y() + shadow.offset.1),
                    );
                    canvas.restore();
                }

                // Fast path: no animators, use standard paragraph.paint()
                if effective_opacity < 1.0 {
                    let mut paint = Paint::default();
                    paint.set_alpha_f(effective_opacity);
                    canvas.save_layer(
                        &skia_safe::canvas::SaveLayerRec::default()
                            .bounds(&rect)
                            .paint(&paint),
                    );
                    paragraph.paint(canvas, (rect.x(), rect.y()));
                    canvas.restore();
                } else {
                    paragraph.paint(canvas, (rect.x(), rect.y()));
                }

                // Apply gradient fill overlay if any span has fill_gradient.
                // This uses SrcIn blend mode so the gradient only appears where
                // text was drawn. Note: when mixed with non-gradient spans,
                // the entire text block receives the gradient.
                let has_gradient = self.spans.iter().any(|s| s.fill_gradient.is_some());
                if has_gradient {
                    if let Some(gradient) =
                        self.spans.iter().find_map(|s| s.fill_gradient.as_ref())
                    {
                        let skia_colors: Vec<skia_safe::Color> =
                            gradient.colors.iter().map(|c| c.to_skia()).collect();
                        let start = skia_safe::Point::new(
                            rect.left + gradient.start.0 * rect.width(),
                            rect.top + gradient.start.1 * rect.height(),
                        );
                        let end = skia_safe::Point::new(
                            rect.left + gradient.end.0 * rect.width(),
                            rect.top + gradient.end.1 * rect.height(),
                        );
                        if let Some(shader) = skia_safe::gradient_shader::linear(
                            (start, end),
                            skia_colors.as_slice(),
                            gradient.positions.as_deref(),
                            TileMode::Clamp,
                            None,
                            None,
                        ) {
                            let mut gradient_paint = Paint::default();
                            gradient_paint.set_shader(shader);
                            gradient_paint.set_blend_mode(skia_safe::BlendMode::SrcIn);
                            canvas.draw_rect(rect, &gradient_paint);
                        }
                    }
                }
            } else {
                // Per-glyph rendering path (kinetic typography)
                self.render_per_glyph(canvas, rect, effective_opacity, paragraph);
            }

            canvas.restore();
        }

        draw_children(canvas);
        Ok(())
    }

    fn animate_property(
        &mut self,
        property: &str,
        start: f32,
        target: f32,
        duration: f64,
        easing: &str,
    ) {
        match property {
            "font_size" | "size" => {
                let ease_fn = crate::node::parse_easing(easing);
                self.default_font_size
                    .add_segment(start, target, duration, ease_fn);
                self.dirty_layout = true;
            }
            "opacity" => {
                let ease_fn = crate::node::parse_easing(easing);
                self.opacity.add_segment(start, target, duration, ease_fn);
            }
            _ => {}
        }
    }

    fn add_text_animator(
        &mut self,
        start_idx: usize,
        end_idx: usize,
        property: String,
        start_val: f32,
        target_val: f32,
        duration: f64,
        easing: &str,
    ) {
        // Parse property name
        let Some(prop) = parse_text_anim_property(&property) else {
            warn!("Unknown text animation property: {}", property);
            return;
        };

        // Parse easing
        let ease_fn = crate::node::parse_easing(easing);

        // Create animator with default stagger of 0.0 (all at once)
        // Advanced stagger can be added via a separate API
        let animator = TextAnimator::new(
            start_idx, end_idx, prop, start_val, target_val, duration, ease_fn,
            0.0, // Default: no stagger
        );

        self.animators.push(animator);
    }

    fn add_text_animator_full(
        &mut self,
        start_idx: usize,
        end_idx: usize,
        property: String,
        start_val: f32,
        target_val: f32,
        duration: f64,
        easing: &str,
        stagger: f32,
    ) {
        let Some(prop) = parse_text_anim_property(&property) else {
            warn!("Unknown text animation property: {}", property);
            return;
        };
        let ease_fn = crate::node::parse_easing(easing);
        let animator = TextAnimator::new(
            start_idx, end_idx, prop, start_val, target_val, duration, ease_fn, stagger,
        );
        self.animators.push(animator);
    }

    fn animate_property_spring(
        &mut self,
        property: &str,
        start: Option<f32>,
        target: f32,
        config: crate::animation::SpringConfig,
    ) {
        if property == "opacity" {
            if let Some(s) = start {
                self.opacity.add_spring_with_start(s, target, config);
            } else {
                self.opacity.add_spring(target, config);
            }
        }
    }

    fn animate_color(
        &mut self,
        start: crate::types::Color,
        end: crate::types::Color,
        duration: f64,
        easing: &str,
    ) {
        let ease_fn = crate::node::parse_easing(easing);
        self.default_color.add_segment(start, end, duration, ease_fn);
        self.init_paragraph();
    }

    fn set_opacity(&mut self, opacity: f32) {
        self.opacity.current_value = opacity;
    }

    fn set_color(&mut self, color: crate::types::Color) {
        self.default_color = Animated::new(color);
        self.init_paragraph();
    }
}
