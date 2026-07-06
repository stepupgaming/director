use crate::animation::Animated;
use crate::element::Element;
use crate::types::Color;
use keyframe::CanTween;
use skia_safe::{
    textlayout::{
        FontCollection, Paragraph, ParagraphBuilder, ParagraphStyle, StrutStyle, TextAlign,
        TextDirection, TextHeightBehavior, TextShadow, TextStyle,
    },
    Canvas, Paint, Point, Rect,
};
use std::any::Any;
use std::fmt;
use std::sync::{Arc, Mutex};
use taffy::geometry::Size;
use taffy::style::{AvailableSpace, Style};

/// A single timed word within a caption.
#[derive(Clone, Debug)]
pub struct CaptionWord {
    pub text: String,
    pub start_sec: f64,
    pub end_sec: f64,
}

/// A single timed caption line.
#[derive(Clone, Debug)]
pub struct CaptionLine {
    pub text: String,
    pub words: Vec<CaptionWord>,
    pub start_sec: f64,
    pub end_sec: f64,
}

/// Where to position the caption within the layout rect.
#[derive(Clone, Debug, PartialEq)]
pub enum CaptionPosition {
    Bottom,
    Top,
    Center,
}

impl CaptionPosition {
    pub fn from_str(s: &str) -> Self {
        match s {
            "top" => Self::Top,
            "center" => Self::Center,
            _ => Self::Bottom,
        }
    }
}

impl Default for CaptionPosition {
    fn default() -> Self {
        Self::Bottom
    }
}

/// Style configuration for caption rendering.
///
/// Default configuration targets a TikTok/Instagram/Remotion look: no
/// background pill, bold-scale-up pop on the active word, smooth color
/// transitions, and a strong shadow for legibility on varied backgrounds.
#[derive(Clone, Debug)]
pub struct CaptionStyle {
    pub font_size: f32,
    pub color: Color,                  // future words
    pub highlight_color: Color,        // active word
    pub spoken_color: Color,           // already-spoken words
    pub background_color: Color,       // optional pill (default transparent)
    pub background_padding: f32,       // used when pill is enabled
    pub position: CaptionPosition,
    pub text_align: TextAlign,
    pub font_family: String,
    pub bottom_margin: f32,
    pub top_margin: f32,
    pub line_spacing: f32,

    // Per-word animation/visual states
    pub active_scale: f32,             // peak scale on active word
    pub future_opacity: f32,           // opacity of un-spoken words
    pub spoken_opacity: f32,           // opacity of past words
    pub active_opacity: f32,           // opacity of currently spoken word
    pub word_transition_duration: f64, // color transition in/out of active
    pub scale_transition_duration: f64,// scale ramp in/out of peak
    pub fade_in_duration: f64,
    pub fade_out_duration: f64,

    // Legibility shadow / outline
    pub text_shadow_enabled: bool,
    pub text_shadow_color: Color,
    pub text_shadow_blur: f32,
    pub text_shadow_offset_x: f32,
    pub text_shadow_offset_y: f32,
    pub outline_enabled: bool,
    pub outline_thickness: f32,
    pub auto_adapt_shadow: bool,       // sample bg luminance and flip shadow color
}

impl Default for CaptionStyle {
    fn default() -> Self {
        Self {
            font_size: 36.0,
            color: Color::WHITE,
            highlight_color: Color::new(1.0, 0.85, 0.1, 1.0),   // warm yellow
            spoken_color: Color::new(0.85, 0.85, 0.85, 1.0),
            background_color: Color::new(0.0, 0.0, 0.0, 0.0),   // no pill by default
            background_padding: 12.0,
            position: CaptionPosition::Bottom,
            text_align: TextAlign::Center,
            font_family: "Roboto".to_string(),
            bottom_margin: 60.0,
            top_margin: 40.0,
            line_spacing: 8.0,

            active_scale: 1.18,
            future_opacity: 0.85,
            spoken_opacity: 0.55,
            active_opacity: 1.0,
            word_transition_duration: 0.18,
            scale_transition_duration: 0.14,
            fade_in_duration: 0.25,
            fade_out_duration: 0.25,

            text_shadow_enabled: true,
            text_shadow_color: Color::new(0.0, 0.0, 0.0, 0.95),
            text_shadow_blur: 0.0,
            text_shadow_offset_x: 0.0,
            text_shadow_offset_y: 2.0,
            outline_enabled: false,
            outline_thickness: 2.0,
            auto_adapt_shadow: true,
        }
    }
}

/// Visual state of a single word at a given time.
#[derive(Clone, Debug)]
struct WordVisualState {
    color: Color,
    font_size: f32,
    opacity: f32,
    is_active: bool,
}

/// A node for rendering timed caption text (SRT/WebVTT subtitles) with
/// word-by-word highlighting, scale-up pop on the active word, smooth color
/// transitions, and shadow-based legibility for any video background.
pub struct CaptionNode {
    pub lines: Vec<CaptionLine>,
    pub style: CaptionStyle,
    pub opacity: Animated<f32>,
    pub font_collection: Arc<Mutex<FontCollection>>,
    pub style_cache: Style,
    cached_time: Mutex<f64>,
    cached_active_hash: Mutex<u64>,
}

impl Clone for CaptionNode {
    fn clone(&self) -> Self {
        Self {
            lines: self.lines.clone(),
            style: self.style.clone(),
            opacity: self.opacity.clone(),
            font_collection: self.font_collection.clone(),
            style_cache: self.style_cache.clone(),
            cached_time: Mutex::new(0.0),
            cached_active_hash: Mutex::new(0),
        }
    }
}

impl fmt::Debug for CaptionNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CaptionNode")
            .field("style", &self.style)
            .field("line_count", &self.lines.len())
            .finish()
    }
}

impl CaptionNode {
    pub fn new(lines: Vec<CaptionLine>, font_collection: Arc<Mutex<FontCollection>>) -> Self {
        Self {
            lines,
            style: CaptionStyle::default(),
            opacity: Animated::new(1.0),
            font_collection,
            style_cache: Style::default(),
            cached_time: Mutex::new(0.0),
            cached_active_hash: Mutex::new(0),
        }
    }

    pub fn active_lines(&self, time_sec: f64) -> Vec<&CaptionLine> {
        self.lines
            .iter()
            .filter(|line| time_sec >= line.start_sec && time_sec < line.end_sec)
            .collect()
    }

    fn build_base_paragraph_style(&self) -> ParagraphStyle {
        let mut ps = ParagraphStyle::new();
        ps.set_text_align(self.style.text_align);
        ps.set_text_direction(TextDirection::LTR);
        ps.set_text_height_behavior(TextHeightBehavior::DisableAll);
        ps.set_replace_tab_characters(true);

        let mut ss = StrutStyle::new();
        ss.set_font_families(&[self.style.font_family.as_str()]);
        ss.set_font_size(self.style.font_size);
        ss.set_height(1.15);
        ss.set_leading(0.0);
        ss.set_strut_enabled(true);
        ss.set_force_strut_height(false);
        ss.set_height_override(true);
        ps.set_strut_style(ss);

        ps
    }

    /// Compute the per-word visual state at `time`. Includes smooth color
    /// and scale transitions across word boundaries.
    fn compute_word_state(
        &self,
        word: &CaptionWord,
        time: f64,
    ) -> WordVisualState {
        let start = word.start_sec;
        let end = word.end_sec;
        let word_dur = (end - start).max(0.001);
        let t_dur = self.style.word_transition_duration.max(0.001);
        let s_dur = self.style.scale_transition_duration.max(0.001);
        let peak = self.style.active_scale;

        let is_active = time >= start && time < end;
        let is_past = time >= end;
        let is_future = time < start;

        // Color state: future=base, active=highlight, past=spoken
        let color = if is_future {
            self.style.color
        } else if is_past {
            let transition = (word_dur * 0.3).min(t_dur).max(0.001);
            if time < end + transition {
                let p = ((time - end) / transition) as f32;
                Color::ease(self.style.highlight_color, self.style.spoken_color, p.clamp(0.0, 1.0))
            } else {
                self.style.spoken_color
            }
        } else {
            let enter_transition = (word_dur * 0.35).min(t_dur).max(0.001);
            if time < start + enter_transition {
                let p = ((time - start) / enter_transition) as f32;
                Color::ease(self.style.color, self.style.highlight_color, p.clamp(0.0, 1.0))
            } else {
                self.style.highlight_color
            }
        };

        // Scale state: triangular ramp up at start, ramp down at end
        let scale = if is_future {
            1.0
        } else if is_past {
            1.0
        } else if time < start + s_dur {
            let p = ((time - start) / s_dur) as f32;
            1.0 + (peak - 1.0) * p.clamp(0.0, 1.0).min(1.0)
        } else if time > end - s_dur {
            let p = ((end - time) / s_dur) as f32;
            1.0 + (peak - 1.0) * p.clamp(0.0, 1.0)
        } else {
            peak
        };

        let opacity = if is_future {
            self.style.future_opacity
        } else if is_past {
            self.style.spoken_opacity
        } else {
            self.style.active_opacity
        };

        WordVisualState {
            color,
            font_size: self.style.font_size * scale,
            opacity: opacity.clamp(0.0, 1.0),
            is_active,
        }
    }

    fn line_opacity(&self, line: &CaptionLine, time: f64) -> f32 {
        let fi = self.style.fade_in_duration;
        let fo = self.style.fade_out_duration;

        let age = time - line.start_sec;
        let time_since_end = time - line.end_sec;

        let in_opacity = if age >= 0.0 && age < fi {
            (age / fi) as f32
        } else if age >= fi {
            1.0
        } else {
            0.0
        };

        let out_opacity = if time_since_end > 0.0 && time_since_end < fo {
            1.0 - (time_since_end / fo) as f32
        } else if time_since_end >= fo {
            0.0
        } else {
            1.0
        };

        in_opacity.min(out_opacity)
    }

    /// Sample the canvas's existing pixel area to estimate background
    /// luminance, returning a value in 0.0..=1.0 (higher = brighter).
    /// Returns None if pixels aren't accessible (only happens for some canvas
    /// types like Document or recorder canvases).
    fn sample_bg_luminance(canvas: &Canvas, rect: Rect) -> Option<f32> {
        let pixmap = canvas.peek_pixels()?;
        let pw = pixmap.width() as i32;
        let ph = pixmap.height() as i32;
        if pw <= 0 || ph <= 0 {
            return None;
        }

        let info = pixmap.info();
        let bpp = match info.color_type() {
            skia_safe::ColorType::RGBA8888 | skia_safe::ColorType::BGRA8888 => 4,
            _ => return None,
        };
        let row_bytes = pixmap.row_bytes();
        let base = pixmap.addr() as *const u8;

        let sample_count = 16u32;
        let mut sum = 0.0f64;
        let mut count = 0u32;

        for k in 0..sample_count {
            // Use a grid spread across the caption rect area + outside
            let ratio_x = ((k * 37 + 11) % sample_count) as f32 / sample_count as f32;
            let ratio_y = ((k * 53 + 7) % sample_count) as f32 / sample_count as f32;
            let x = rect.left as i32 + (ratio_x * rect.width() as f32) as i32;
            let y = rect.top as i32 + (ratio_y * rect.height() as f32) as i32;

            if x < 0 || y < 0 || x >= pw || y >= ph {
                continue;
            }

            let row_offset = y as usize * row_bytes;
            let px_offset = row_offset + x as usize * bpp;

            unsafe {
                let b = *base.add(px_offset) as f32;
                let g = *base.add(px_offset + 1) as f32;
                let r = *base.add(px_offset + 2) as f32;
                // Rec.709 luma approximation
                let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                sum += luma as f64;
                count += 1;
            }
        }

        if count == 0 {
            return None;
        }

        Some(((sum / count as f64) / 255.0) as f32)
    }

    /// Build a per-word TextStyle. Adds shadow/outline as configured, and
    /// applies per-word color and font_size.
    fn make_word_text_style(
        &self,
        state: &WordVisualState,
        shadow_color_override: Option<Color>,
    ) -> TextStyle {
        let mut ts = TextStyle::new();
        ts.set_font_size(state.font_size);
        ts.set_font_families(&[
            self.style.font_family.as_str(),
            "Sans Serif",
            "Arial",
        ]);

        // Per-word foreground paint (color with opacity baked in so we don't
        // need a save_layer per word).
        let mut paint = Paint::default();
        let mut c = state.color;
        c.a *= state.opacity;
        paint.set_color(c.to_skia());
        paint.set_anti_alias(true);
        ts.set_foreground_paint(&paint);

        // Shadow for legibility on varied backgrounds
        if self.style.text_shadow_enabled {
            let mut sc = shadow_color_override.unwrap_or(self.style.text_shadow_color);
            sc.a *= state.opacity;
            let shadow = TextShadow::new(
                sc.to_skia(),
                Point::new(
                    self.style.text_shadow_offset_x as f32,
                    self.style.text_shadow_offset_y as f32,
                ),
                self.style.text_shadow_blur as f64,
            );
            ts.add_shadow(shadow);
        }

        // Outline: 8-direction fake-outline for maximum legibility
        if self.style.outline_enabled {
            let r = self.style.outline_thickness;
            let mut sc = shadow_color_override.unwrap_or(self.style.text_shadow_color);
            sc.a *= state.opacity;
            let offsets = [
                (r, 0.0),
                (-r, 0.0),
                (0.0, r),
                (0.0, -r),
                (r * 0.707, r * 0.707),
                (r * 0.707, -r * 0.707),
                (-r * 0.707, r * 0.707),
                (-r * 0.707, -r * 0.707),
            ];
            for (dx, dy) in offsets {
                ts.add_shadow(TextShadow::new(
                    sc.to_skia(),
                    Point::new(dx, dy),
                    0.0,
                ));
            }
        }

        ts
    }

    fn build_word_paragraph(
        &self,
        line: &CaptionLine,
        time: f64,
        width: f32,
        shadow_override: Option<Color>,
    ) -> Paragraph {
        let paragraph_style = self.build_base_paragraph_style();

        let font_guard = match self.font_collection.lock() {
            Ok(g) => g,
            Err(_) => {
                let mut builder = ParagraphBuilder::new(
                    &ParagraphStyle::new(),
                    &skia_safe::textlayout::FontCollection::new(),
                );
                builder.add_text(&line.text);
                let mut p = builder.build();
                p.layout(width);
                return p;
            }
        };

        let mut builder = ParagraphBuilder::new(&paragraph_style, &*font_guard);

        let word_count = line.words.len();
        for (i, word) in line.words.iter().enumerate() {
            let state = self.compute_word_state(word, time);
            let ts = self.make_word_text_style(&state, shadow_override);
            builder.push_style(&ts);
            builder.add_text(&word.text);
            builder.pop();

            if i < word_count - 1 {
                builder.add_text(" ");
            }
        }

        builder.build()
    }

    fn active_hash(&self, time: f64) -> u64 {
        let mut h: u64 = 0;
        for line in &self.lines {
            if time >= line.start_sec && time < line.end_sec {
                if let Some(active_idx) = line.words.iter().position(|w| time >= w.start_sec && time < w.end_sec) {
                    h = h.wrapping_mul(31).wrapping_add(active_idx as u64);
                }
            }
        }
        h
    }
}

/// Compute the maximum number of overlapping caption lines at any time.
pub fn max_overlapping_lines(lines: &[CaptionLine]) -> usize {
    if lines.is_empty() {
        return 0;
    }
    let mut events: Vec<(f64, i32)> = Vec::new();
    for line in lines {
        events.push((line.start_sec, 1));
        events.push((line.end_sec, -1));
    }
    events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut count = 0;
    let mut max_count = 0;
    for (_, delta) in events {
        count += delta;
        if count > max_count {
            max_count = count;
        }
    }
    max_count as usize
}

/// Auto-populate word timing for a caption line by dividing its duration equally.
pub fn auto_time_words(line: &mut CaptionLine) {
    if !line.words.is_empty() {
        return;
    }
    let word_list: Vec<&str> = line.text.split_whitespace().collect();
    if word_list.is_empty() {
        return;
    }
    let duration = line.end_sec - line.start_sec;
    if duration <= 0.0 {
        line.words = word_list
            .into_iter()
            .map(|w| CaptionWord {
                text: w.to_string(),
                start_sec: line.start_sec,
                end_sec: line.end_sec,
            })
            .collect();
        return;
    }
    let word_duration = duration / word_list.len() as f64;
    line.words = word_list
        .into_iter()
        .enumerate()
        .map(|(i, w)| CaptionWord {
            text: w.to_string(),
            start_sec: line.start_sec + i as f64 * word_duration,
            end_sec: line.start_sec + (i + 1) as f64 * word_duration,
        })
        .collect();
}

impl Element for CaptionNode {
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
        let width = match available_space.width {
            AvailableSpace::Definite(w) => w,
            AvailableSpace::MinContent => 1.0,
            AvailableSpace::MaxContent => f32::INFINITY,
        };

        let layout_width = known_dimensions.width.unwrap_or(width);

        // Make sure text doesn't run past the canvas; reserve enough room for
        // active word scale (peak could be wider than the base text).
        let safe_width = (layout_width * 0.95).max(20.0);

        let single_height = self
            .lines
            .iter()
            .map(|line| {
                let fc = self.font_collection.lock().ok();
                let mut para = match fc {
                    Some(g) => ParagraphBuilder::new(
                        &self.build_base_paragraph_style(),
                        &*g,
                    )
                    .build(),
                    None => {
                        let mut b = ParagraphBuilder::new(
                            &ParagraphStyle::new(),
                            &skia_safe::textlayout::FontCollection::new(),
                        );
                        b.add_text(&line.text);
                        b.build()
                    }
                };
                para.layout(safe_width);
                para.height().ceil()
            })
            .fold(0.0f32, f32::max);

        let max_overlap = max_overlapping_lines(&self.lines).max(1) as f32;
        let stack_height = single_height * max_overlap + (max_overlap - 1.0) * self.style.line_spacing;

        let extra_margin = match self.style.position {
            CaptionPosition::Bottom => self.style.bottom_margin,
            CaptionPosition::Top => self.style.top_margin,
            CaptionPosition::Center => 0.0,
        };
        Size {
            width: layout_width,
            height: stack_height + extra_margin,
        }
    }

    fn layout_style(&self) -> Style {
        self.style_cache.clone()
    }

    fn set_layout_style(&mut self, style: Style) {
        self.style_cache = style;
    }

    fn update(&mut self, time: f64) -> bool {
        self.opacity.update(time);
        if let Ok(mut t) = self.cached_time.lock() {
            *t = time;
        }
        if let Ok(mut h) = self.cached_active_hash.lock() {
            let new_hash = self.active_hash(time);
            if *h != new_hash {
                *h = new_hash;
                return true;
            }
        }
        false
    }

    fn render(
        &self,
        canvas: &Canvas,
        rect: Rect,
        opacity: f32,
        draw_children: &mut dyn FnMut(&Canvas),
    ) -> Result<(), crate::RenderError> {
        let effective_opacity = self.opacity.current_value * opacity;
        if effective_opacity < 0.01 {
            draw_children(canvas);
            return Ok(());
        }

        let current_time = *self.cached_time.lock().unwrap_or_else(|e| e.into_inner());

        // Collect lines visible within the fade-in/out window
        let active: Vec<&CaptionLine> = self
            .lines
            .iter()
            .filter(|line| {
                current_time >= line.start_sec - self.style.fade_in_duration
                    && current_time < line.end_sec + self.style.fade_out_duration
            })
            .collect();

        if active.is_empty() {
            draw_children(canvas);
            return Ok(());
        }

        let line_opacities: Vec<f32> = active
            .iter()
            .map(|line| self.line_opacity(line, current_time))
            .collect();
        let max_line_opacity = line_opacities.iter().cloned().fold(0.0f32, f32::max);

        if max_line_opacity < 0.01 {
            draw_children(canvas);
            return Ok(());
        }

        // ── Optional bg luminance sampling for adaptive shadow color ──
        let shadow_override: Option<Color> = if self.style.auto_adapt_shadow
            && (self.style.text_shadow_enabled || self.style.outline_enabled)
        {
            // Sample a slightly larger area so we capture the actual video
            // behind the caption (not just empty text space).
            let sample_rect = Rect::from_xywh(
                rect.left,
                (rect.bottom - self.style.bottom_margin - 80.0).max(rect.top),
                rect.width(),
                80.0,
            );
            Self::sample_bg_luminance(canvas, sample_rect).map(|lum| {
                if lum > 0.55 {
                    // Bright bg → black shadow/outline
                    if self.style.text_shadow_color != Color::BLACK {
                        Color::BLACK
                    } else {
                        self.style.text_shadow_color
                    }
                } else if lum < 0.40 {
                    // Dark bg → white shadow/outline, soften drop offset
                    if self.style.text_shadow_color != Color::WHITE {
                        Color::WHITE
                    } else {
                        self.style.text_shadow_color
                    }
                } else {
                    // Mid → keep config
                    self.style.text_shadow_color
                }
            })
        } else {
            None
        };

        // ── Build paragraphs at canvas width ──
        // Use 95% of rect width to leave breathing room around the active
        // word's scale-up. The center-aligned text fills the layout width
        // and Skia handles horizontal centering internally.
        let layout_width = (rect.width() * 0.95).max(20.0);

        let mut total_height = 0.0f32;
        let mut paragraphs: Vec<(Paragraph, f32)> = Vec::with_capacity(active.len());
        for (line, lo) in active.iter().zip(line_opacities.iter()) {
            let mut para = self.build_word_paragraph(line, current_time, layout_width, shadow_override);
            para.layout(layout_width);
            total_height += para.height();
            paragraphs.push((para, *lo));
        }

        let spacing = (paragraphs.len().saturating_sub(1) as f32) * self.style.line_spacing;
        let stack_height = total_height + spacing;

        // ── Optional background pill (opt-in only, alpha=0 by default) ──
        let has_pill = self.style.background_color.a > 0.01;
        if has_pill {
            let bg_w = layout_width + self.style.background_padding * 2.0;
            let bg_h = stack_height + self.style.background_padding * 1.5;
            let bg_x = rect.left + (rect.width() - bg_w) / 2.0;

            let max_bg_y = rect.bottom - bg_h;
            let bg_y = match self.style.position {
                CaptionPosition::Bottom => {
                    (rect.bottom - bg_h - self.style.bottom_margin).clamp(rect.top, max_bg_y)
                }
                CaptionPosition::Top => (rect.top + self.style.top_margin).min(max_bg_y),
                CaptionPosition::Center => {
                    (rect.top + (rect.height() - bg_h) / 2.0).clamp(rect.top, max_bg_y)
                }
            };

            let bg_pill_opacity = effective_opacity * max_line_opacity;
            let mut bg_paint = Paint::default();
            bg_paint.set_color(self.style.background_color.to_skia());
            bg_paint.set_alpha_f(bg_pill_opacity);
            bg_paint.set_anti_alias(true);

            let radius = self.style.font_size * 0.2;
            let radii = [
                skia_safe::Point::new(radius, radius),
                skia_safe::Point::new(radius, radius),
                skia_safe::Point::new(radius, radius),
                skia_safe::Point::new(radius, radius),
            ];
            let pill_rect = Rect::from_xywh(bg_x, bg_y, bg_w, bg_h);
            let rrect = skia_safe::RRect::new_rect_radii(pill_rect, &radii);
            canvas.draw_rrect(rrect, &bg_paint);
        }

        // ── Position text vertically ──
        let text_origin_y = if has_pill {
            let bg_w = layout_width + self.style.background_padding * 2.0;
            let bg_h = stack_height + self.style.background_padding * 1.5;
            let bg_x = rect.left + (rect.width() - bg_w) / 2.0;
            let max_bg_y = rect.bottom - bg_h;
            let bg_y = match self.style.position {
                CaptionPosition::Bottom => {
                    (rect.bottom - bg_h - self.style.bottom_margin).clamp(rect.top, max_bg_y)
                }
                CaptionPosition::Top => (rect.top + self.style.top_margin).min(max_bg_y),
                CaptionPosition::Center => {
                    (rect.top + (rect.height() - bg_h) / 2.0).clamp(rect.top, max_bg_y)
                }
            };
            bg_y + self.style.background_padding * 0.75
        } else {
            let max_y = rect.bottom - stack_height;
            match self.style.position {
                CaptionPosition::Bottom => {
                    (rect.bottom - stack_height - self.style.bottom_margin).clamp(rect.top, max_y)
                }
                CaptionPosition::Top => (rect.top + self.style.top_margin).min(max_y),
                CaptionPosition::Center => {
                    (rect.top + (rect.height() - stack_height) / 2.0).clamp(rect.top, max_y)
                }
            }
        };

        let text_x = rect.left;

        canvas.save();
        canvas.clip_rect(rect, skia_safe::ClipOp::Intersect, true);

        let mut current_y = text_origin_y;
        for (para, line_opacity) in &paragraphs {
            let para_opacity = effective_opacity * line_opacity;
            if para_opacity < 0.99 {
                let mut text_paint = Paint::default();
                text_paint.set_alpha_f(para_opacity);
                let layer_bounds = Rect::from_xywh(
                    text_x,
                    current_y,
                    rect.width(),
                    para.height() + self.style.line_spacing + 4.0,
                );
                canvas.save_layer(
                    &skia_safe::canvas::SaveLayerRec::default()
                        .bounds(&layer_bounds)
                        .paint(&text_paint),
                );
                para.paint(canvas, (text_x, current_y));
                canvas.restore();
            } else {
                para.paint(canvas, (text_x, current_y));
            }
            current_y += para.height() + self.style.line_spacing;
        }

        canvas.restore();

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
        if property == "opacity" || property == "font_size" {
            let ease_fn = crate::node::parse_easing(easing);
            if property == "opacity" {
                self.opacity.add_segment(start, target, duration, ease_fn);
            }
        }
    }

    fn set_opacity(&mut self, opacity: f32) {
        self.opacity.current_value = opacity;
    }
}

/// Parse an SRT subtitle string into a Vec of CaptionLine.
pub fn parse_srt(content: &str) -> Vec<CaptionLine> {
    let mut lines = Vec::new();
    let blocks: Vec<&str> = content.trim().split("\n\n").collect();

    for block in blocks {
        let block = block.trim();
        if block.is_empty() {
            continue;
        }

        let parts: Vec<&str> = block.splitn(3, '\n').collect();
        if parts.len() < 3 {
            continue;
        }

        let time_line = parts[1].trim();
        let text = parts[2].trim().to_string();

        let (start_ms, end_ms) = match parse_srt_timestamp(time_line) {
            Some(t) => t,
            None => continue,
        };

        let mut line = CaptionLine {
            text: text.clone(),
            words: Vec::new(),
            start_sec: start_ms / 1000.0,
            end_sec: end_ms / 1000.0,
        };
        auto_time_words(&mut line);
        lines.push(line);
    }

    lines
}

fn parse_srt_timestamp(line: &str) -> Option<(f64, f64)> {
    let arrow = " --> ";
    let mut parts = line.split(arrow);
    let start_str = parts.next()?;
    let end_str = parts.next()?;

    let start_ms = parse_srt_time(start_str.trim())?;
    let end_ms = parse_srt_time(end_str.trim())?;

    Some((start_ms, end_ms))
}

fn parse_srt_time(s: &str) -> Option<f64> {
    let s = s.replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let hours: f64 = parts[0].parse().ok()?;
    let minutes: f64 = parts[1].parse().ok()?;
    let seconds: f64 = parts[2].parse().ok()?;

    Some(hours * 3600.0 * 1000.0 + minutes * 60.0 * 1000.0 + seconds * 1000.0)
}

/// Parse a WebVTT subtitle string (basic format) into a Vec of CaptionLine.
pub fn parse_vtt(content: &str) -> Vec<CaptionLine> {
    let body = if let Some(idx) = content.find("\n\n") {
        &content[idx + 2..]
    } else {
        content
    };
    parse_srt(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_srt_simple() {
        let input = "1\n00:00:01,000 --> 00:00:05,000\nHello world\n\n2\n00:00:05,000 --> 00:00:10,000\nThis is a test\n";
        let lines = parse_srt(input);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "Hello world");
        assert!((lines[0].start_sec - 1.0).abs() < 0.001);
        assert!((lines[0].end_sec - 5.0).abs() < 0.001);
        assert!(!lines[0].words.is_empty());
        assert_eq!(lines[0].words.len(), 2);
        assert_eq!(lines[1].text, "This is a test");
        assert!((lines[1].start_sec - 5.0).abs() < 0.001);
        assert!((lines[1].end_sec - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_parse_srt_multi_line() {
        let input = "1\n00:00:01,000 --> 00:00:05,000\nLine one\nLine two\n\n2\n00:00:05,000 --> 00:00:10,000\nLast line\n";
        let lines = parse_srt(input);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "Line one\nLine two");
        assert!(!lines[0].words.is_empty());
    }

    #[test]
    fn test_parse_srt_empty() {
        let lines = parse_srt("");
        assert_eq!(lines.len(), 0);
    }

    #[test]
    fn test_parse_vtt_with_header() {
        let input = "WEBVTT\n\n1\n00:00:01,000 --> 00:00:05,000\nHello from VTT\n\n2\n00:00:05,000 --> 00:00:10,000\nVTT line 2\n";
        let lines = parse_vtt(input);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "Hello from VTT");
    }

    #[test]
    fn test_caption_line_timing() {
        let mut lines = vec![
            CaptionLine {
                text: "First".to_string(),
                words: Vec::new(),
                start_sec: 0.0,
                end_sec: 5.0,
            },
            CaptionLine {
                text: "Second".to_string(),
                words: Vec::new(),
                start_sec: 5.0,
                end_sec: 10.0,
            },
        ];
        for line in &mut lines {
            auto_time_words(line);
        }
        let fc = Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()));
        let node = CaptionNode::new(lines, fc);
        assert_eq!(node.active_lines(-1.0).len(), 0);
        assert_eq!(
            node.active_lines(0.0)
                .first()
                .map(|l| l.text.as_str()),
            Some("First")
        );
        assert_eq!(
            node.active_lines(2.5)
                .first()
                .map(|l| l.text.as_str()),
            Some("First")
        );
        assert_eq!(
            node.active_lines(5.0)
                .first()
                .map(|l| l.text.as_str()),
            Some("Second")
        );
        assert_eq!(node.active_lines(10.0).len(), 0);
    }

    #[test]
    fn test_word_timing_auto() {
        let mut line = CaptionLine {
            text: "Hello world".to_string(),
            words: Vec::new(),
            start_sec: 0.0,
            end_sec: 4.0,
        };
        auto_time_words(&mut line);
        assert_eq!(line.words.len(), 2);
        assert!((line.words[0].start_sec - 0.0).abs() < 0.001);
        assert!((line.words[0].end_sec - 2.0).abs() < 0.001);
        assert!((line.words[1].start_sec - 2.0).abs() < 0.001);
        assert!((line.words[1].end_sec - 4.0).abs() < 0.001);
    }

    #[test]
    fn test_word_timing_preserves_explicit() {
        let mut line = CaptionLine {
            text: "Hello world".to_string(),
            words: vec![
                CaptionWord {
                    text: "Hello".to_string(),
                    start_sec: 0.0,
                    end_sec: 3.0,
                },
                CaptionWord {
                    text: "world".to_string(),
                    start_sec: 3.0,
                    end_sec: 4.0,
                },
            ],
            start_sec: 0.0,
            end_sec: 4.0,
        };
        auto_time_words(&mut line);
        assert_eq!(line.words.len(), 2);
        assert!((line.words[0].end_sec - 3.0).abs() < 0.001);
    }

    #[test]
    fn test_active_word_index() {
        let line = CaptionLine {
            text: "A B C".to_string(),
            words: vec![
                CaptionWord {
                    text: "A".to_string(),
                    start_sec: 0.0,
                    end_sec: 1.0,
                },
                CaptionWord {
                    text: "B".to_string(),
                    start_sec: 1.0,
                    end_sec: 2.0,
                },
                CaptionWord {
                    text: "C".to_string(),
                    start_sec: 2.0,
                    end_sec: 3.0,
                },
            ],
            start_sec: 0.0,
            end_sec: 3.0,
        };
        assert_eq!(line.words.iter().position(|w| 0.5 >= w.start_sec && 0.5 < w.end_sec), Some(0));
        assert_eq!(line.words.iter().position(|w| 1.5 >= w.start_sec && 1.5 < w.end_sec), Some(1));
        assert_eq!(line.words.iter().position(|w| 2.5 >= w.start_sec && 2.5 < w.end_sec), Some(2));
        assert_eq!(line.words.iter().position(|w| 3.0 >= w.start_sec && 3.0 < w.end_sec), None);
    }

    #[test]
    fn test_max_overlapping_lines() {
        let lines = vec![
            CaptionLine { text: "A".to_string(), words: Vec::new(), start_sec: 0.0, end_sec: 5.0 },
            CaptionLine { text: "B".to_string(), words: Vec::new(), start_sec: 2.0, end_sec: 7.0 },
            CaptionLine { text: "C".to_string(), words: Vec::new(), start_sec: 4.0, end_sec: 6.0 },
        ];
        assert_eq!(max_overlapping_lines(&lines), 3);
    }

    #[test]
    fn test_max_overlapping_lines_non_overlapping() {
        let lines = vec![
            CaptionLine { text: "A".to_string(), words: Vec::new(), start_sec: 0.0, end_sec: 2.0 },
            CaptionLine { text: "B".to_string(), words: Vec::new(), start_sec: 2.0, end_sec: 4.0 },
            CaptionLine { text: "C".to_string(), words: Vec::new(), start_sec: 4.0, end_sec: 6.0 },
        ];
        assert_eq!(max_overlapping_lines(&lines), 1);
    }

    #[test]
    fn test_multi_line_active_lines() {
        let lines = vec![
            CaptionLine { text: "First".to_string(), words: Vec::new(), start_sec: 0.0, end_sec: 10.0 },
            CaptionLine { text: "Second".to_string(), words: Vec::new(), start_sec: 3.0, end_sec: 7.0 },
        ];
        let fc = Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()));
        let node = CaptionNode::new(lines, fc);
        assert_eq!(node.active_lines(1.0).len(), 1);
        assert_eq!(node.active_lines(5.0).len(), 2);
        assert_eq!(node.active_lines(8.0).len(), 1);
        assert_eq!(node.active_lines(11.0).len(), 0);
    }

    // ── New behavior tests ─────────────────────────────────────────────

    /// Future word past `start` should ramp color from base to highlight
    /// over the entry transition. Inside the entry window, color should be
    /// a blend.
    #[test]
    fn test_future_word_color_ramps_to_highlight() {
        let mut line = CaptionLine {
            text: "alpha beta".to_string(),
            words: Vec::new(),
            start_sec: 0.0,
            end_sec: 4.0,
        };
        auto_time_words(&mut line);
        let fc = Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()));
        let mut node = CaptionNode::new(vec![line], fc);
        node.style.color = Color::new(1.0, 1.0, 1.0, 1.0);
        node.style.highlight_color = Color::new(1.0, 0.0, 0.0, 1.0);
        node.style.word_transition_duration = 0.5;
        let word = &node.lines[0].words[0];
        let before = node.compute_word_state(word, word.start_sec - 0.5);
        let mid = node.compute_word_state(word, word.start_sec + 0.25);
        let full = node.compute_word_state(word, word.start_sec + 0.6);
        // Before start: full base color (white)
        assert!((before.color.r - 1.0).abs() < 0.01);
        assert!(before.color.g > 0.95);
        assert!(before.color.b > 0.95);
        // Mid: blended toward highlight (different from before and full)
        assert!(mid.color.b < before.color.b);
        // After full entry: highlight (red, no green/blue)
        assert!(full.color.g < 0.05);
        assert!(full.color.b < 0.05);
        // The transition monotonically moves in OKLab distance
        let dist_mid = (mid.color.r - full.color.r).abs()
            + (mid.color.g - full.color.g).abs()
            + (mid.color.b - full.color.b).abs();
        assert!(dist_mid > 0.01, "mid should differ from full highlight");
    }

    /// When time is past `end`, color should ramp from highlight to spoken.
    #[test]
    fn test_spoken_word_color_ramps_to_spoken() {
        let mut line = CaptionLine {
            text: "alpha beta".to_string(),
            words: Vec::new(),
            start_sec: 0.0,
            end_sec: 4.0,
        };
        auto_time_words(&mut line);
        let fc = Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()));
        let mut node = CaptionNode::new(vec![line], fc);
        node.style.highlight_color = Color::new(1.0, 0.0, 0.0, 1.0);
        node.style.spoken_color = Color::new(0.5, 0.5, 0.5, 1.0);
        node.style.word_transition_duration = 0.5;
        let word = &node.lines[0].words[0];
        let post_mid = node.compute_word_state(word, word.end_sec + 0.25);
        let post_full = node.compute_word_state(
            word,
            word.end_sec + 0.6,
        );
        // Post mid should have moved away from red toward gray
        assert!(post_mid.color.g > 0.05);
        assert!(post_mid.color.g < 0.55);
        // After full transition: pure spoken_color
        assert!((post_full.color.r - 0.5).abs() < 0.05);
        assert!((post_full.color.g - 0.5).abs() < 0.05);
        assert!((post_full.color.b - 0.5).abs() < 0.05);
    }

    /// Active word gets scale > 1.0, future and spoken are 1.0.
    #[test]
    fn test_active_word_scales_above_one() {
        let mut line = CaptionLine {
            text: "alpha beta".to_string(),
            words: Vec::new(),
            start_sec: 0.0,
            end_sec: 4.0,
        };
        auto_time_words(&mut line);
        let fc = Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()));
        let mut node = CaptionNode::new(vec![line], fc);
        node.style.active_scale = 1.2;
        node.style.scale_transition_duration = 0.2;
        let word = &node.lines[0].words[0];
        let future = node.compute_word_state(word, word.start_sec - 0.5);
        let active = node.compute_word_state(word, word.start_sec + 0.4);
        let spoken = node.compute_word_state(word, word.end_sec + 1.0);
        assert!((future.font_size - node.style.font_size).abs() < 0.01);
        assert!(active.font_size > node.style.font_size * 1.05);
        assert!(active.font_size < node.style.font_size * 1.25);
        let base = node.style.font_size;
        assert!((spoken.font_size - base).abs() < 0.01);
    }

    /// Per-word opacity follows future/spoken/active levels.
    #[test]
    fn test_per_word_opacity_levels() {
        let mut line = CaptionLine {
            text: "alpha beta".to_string(),
            words: Vec::new(),
            start_sec: 0.0,
            end_sec: 4.0,
        };
        auto_time_words(&mut line);
        let fc = Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()));
        let mut node = CaptionNode::new(vec![line], fc);
        node.style.future_opacity = 0.7;
        node.style.spoken_opacity = 0.4;
        node.style.active_opacity = 1.0;
        let word = &node.lines[0].words[0];
        let future = node.compute_word_state(word, word.start_sec - 0.5);
        let active = node.compute_word_state(word, word.start_sec + 0.4);
        let spoken = node.compute_word_state(word, word.end_sec + 1.0);
        assert!((future.opacity - 0.7).abs() < 0.01);
        assert!((active.opacity - 1.0).abs() < 0.01);
        assert!((spoken.opacity - 0.4).abs() < 0.01);
    }

    /// Pill background should be transparent by default (no pill drawn).
    #[test]
    fn test_no_pill_by_default() {
        let fc = Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()));
        let node = CaptionNode::new(Vec::new(), fc);
        assert!(node.style.background_color.a < 0.01);
        assert!(!node.style.outline_enabled);
        // Default text shadow is enabled
        assert!(node.style.text_shadow_enabled);
        // Default active scale > 1
        assert!(node.style.active_scale > 1.0);
    }
}
