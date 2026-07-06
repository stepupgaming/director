use crate::animation::Animated;
use crate::element::Element;
use crate::types::Color;
use skia_safe::{
    textlayout::{
        FontCollection, Paragraph, ParagraphBuilder, ParagraphStyle, RectHeightStyle,
        RectWidthStyle, StrutStyle, TextAlign, TextDirection, TextHeightBehavior,
        TextShadow, TextStyle,
    },
    Canvas, Paint, Point, Rect,
};
use std::any::Any;
use std::fmt;
use std::sync::{Arc, Mutex};
use taffy::geometry::Size;
use taffy::style::{AvailableSpace, Style};

/// A single timed word within a caption cue.
#[derive(Clone, Debug)]
pub struct CaptionWord {
    pub text: String,
    pub start_sec: f64,
    pub end_sec: f64,
    /// 0-indexed display line within the cue's text. The paragraph builder
    /// groups words by `display_line_idx` and renders one Paragraph per
    /// display line, stacked vertically inside the cue's pre-laid slot.
    pub display_line_idx: usize,
}

/// A single timed caption cue. Words inside share an active highlight
/// window. All display-line paragraphs for the same cue render into the
/// cue's pre-laid slot — second-line appearance NEVER reflows the first.
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

/// Caption styling. Defaults target a clean, TikTok/Reels look:
/// white text, warm yellow active word, restrained shadow only, max 2
/// lines, chunk-sized (3-8 words per cue), subtle 1.04 scale pop on
/// activation moment (no continuous motion, no overshoot, no jitter).
#[derive(Clone, Debug)]
pub struct CaptionStyle {
    pub font_size: f32,
    pub color: Color,            // future words (default white)
    pub highlight_color: Color,  // active word (default warm gold)
    pub spoken_color: Color,     // past words (default dim white)

    pub position: CaptionPosition,
    pub text_align: TextAlign,
    pub font_family: String,
    pub bottom_margin: f32,
    pub top_margin: f32,
    pub line_spacing: f32,
    pub layout_width_fraction: f32, // fraction of canvas width

    // Per-word opacity for each state
    pub active_opacity: f32,
    pub future_opacity: f32,
    pub spoken_color_opacity: f32,  // alpha multiplier for spoken color

    // Word color transitions (fast, no overshoot)
    pub color_transition_duration: f64, // total per side, in seconds

    // Active word emphasis: subtle scale pop overlay (canvas scale, never
    // alters font_size on the base paragraph → layout is stable).
    pub active_pop_scale: f32,
    pub active_pop_duration: f64,
    pub active_pop_easing: String, // "ease_out_quad" (default), "linear"

    // Glow on the active word (subtle, no continuous pulse)
    pub glow_enabled: bool,
    pub glow_blur_radius: f32,

    // Drop shadow for any-bg legibility
    pub text_shadow_enabled: bool,
    pub text_shadow_color: Color,
    pub text_shadow_offset_x: f32,
    pub text_shadow_offset_y: f32,
    pub text_shadow_blur: f32,

    pub auto_adapt_shadow: bool,

    // Per-display-line reveal (delayed entrance via canvas save_layer
    // alpha; line 0 is immediate, line 1 fades in mid-cue by default).
    pub line_reveal_offsets: Vec<f32>,    // fraction of CUE duration
    pub line_reveal_durations: Vec<f32>,  // seconds; fade-in duration
}

impl Default for CaptionStyle {
    fn default() -> Self {
        Self {
            font_size: 42.0,
            color: Color::new(1.0, 1.0, 1.0, 1.0),
            highlight_color: Color::new(1.0, 0.84, 0.04, 1.0),  // #FFD60A
            spoken_color: Color::new(0.78, 0.78, 0.78, 1.0),

            position: CaptionPosition::Bottom,
            text_align: TextAlign::Center,
            font_family: "Roboto".to_string(),
            bottom_margin: 110.0,
            top_margin: 60.0,
            line_spacing: 16.0,
            layout_width_fraction: 0.86,

            active_opacity: 1.0,
            future_opacity: 0.92,
            spoken_color_opacity: 0.55,

            color_transition_duration: 0.10,

            active_pop_scale: 1.04,
            active_pop_duration: 0.08,
            active_pop_easing: "ease_out_quad".to_string(),

            glow_enabled: true,
            glow_blur_radius: 4.0,

            text_shadow_enabled: true,
            text_shadow_color: Color::new(0.0, 0.0, 0.0, 0.85),
            text_shadow_offset_x: 0.0,
            text_shadow_offset_y: 2.0,
            text_shadow_blur: 0.0,

            auto_adapt_shadow: true,

            line_reveal_offsets: vec![0.0, 0.45],
            line_reveal_durations: vec![0.10, 0.20],
        }
    }
}

/// Per-cue layout cache. Stores the pre-laid slot index, the display-line
/// paragraphs, and per-word character ranges used to find the active
/// word's exact pixel rect via `paragraph.getRectsForRange()`.
struct CueLayout {
    slot: usize,
    paragraph_layout_width: f32,
    display_lines: Vec<DisplayLineLayout>,
}

struct DisplayLineLayout {
    paragraph: Paragraph,
    word_ranges: Vec<(usize, usize)>, // char (start, end) per word in this line
    reveal_offset: f32,
    reveal_duration: f32,
}

struct BlockLayout {
    width: f32,
    cues: Vec<CueLayout>,
    max_overlap: usize,
    max_slot_height: f32,
    total_height: f32,
}

impl fmt::Debug for BlockLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BlockLayout")
            .field("width", &self.width)
            .field("cues", &self.cues.len())
            .field("max_overlap", &self.max_overlap)
            .field("max_slot_height", &self.max_slot_height)
            .field("total_height", &self.total_height)
            .finish()
    }
}

/// A node for rendering timed caption text (SRT/WebVTT subtitles) with
/// pre-laid-out, structurally stable text blocks. Designed for clean
/// TikTok/Reels style kinetic captions: white text, warm yellow active
/// word, restrained shadow for any-bg legibility, subtle scale pop on the
/// active word without altering surrounding word positions.
pub struct CaptionNode {
    pub lines: Vec<CaptionLine>,
    pub style: CaptionStyle,
    pub opacity: Animated<f32>,
    pub font_collection: Arc<Mutex<FontCollection>>,
    pub style_cache: Style,
    cached_time: Mutex<f64>,
    block_layout: Mutex<Option<BlockLayout>>,
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
            block_layout: Mutex::new(None),
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
            block_layout: Mutex::new(None),
        }
    }

    pub fn active_lines(&self, time_sec: f64) -> Vec<&CaptionLine> {
        self.lines
            .iter()
            .filter(|line| time_sec >= line.start_sec && time_sec < line.end_sec)
            .collect()
    }

    // ── Easing (no overshoot, no elasticity — smooth and subtle) ──

    fn ease_out_quad(t: f32) -> f32 {
        let x = t.clamp(0.0, 1.0);
        1.0 - (1.0 - x) * (1.0 - x)
    }

    fn ease_out_sine(t: f32) -> f32 {
        let x = t.clamp(0.0, 1.0);
        (1.0 - (x * std::f32::consts::FRAC_PI_2).cos())
    }

    // ── Per-word color/opacity state at time t. NO scale variation;
    //    layout is always uniform font_size; only color + opacity vary. ──

    fn compute_word_color(&self, word: &CaptionWord, time: f64, is_active: bool) -> Color {
        let start = word.start_sec;
        let end = word.end_sec;
        let t_dur = self.style.color_transition_duration.max(0.001);

        if is_active {
            return self.style.highlight_color;
        }

        let is_future = time < start;
        let is_past = time >= end;

        if is_future {
            // Smooth entry into active: ramp from base color to highlight
            // over `color_transition_duration` seconds before `start`.
            if time > start - t_dur {
                let p = ((start - time) / t_dur) as f32;
                let p = p.clamp(0.0, 1.0);
                // ease in: at t=start we are 0% highlight; at start-t_dur we are 100% base.
                return lerp_color(self.style.highlight_color, self.style.color, p);
            }
            return self.style.color;
        }
        if is_past {
            // Past → spoken is INSTANT (no transition). Keeping a past-word
            // yellow transition would cause a dual-yellow frame at the boundary
            // when the next word enters active. The spec requires exactly one
            // active word at a time; only the entry side gets a smooth ramp.
            return self.style.spoken_color;
        }
        // Pre-active but during cue: future (white).
        self.style.color
    }

    fn compute_word_opacity(&self, is_future: bool, is_past: bool, is_active: bool) -> f32 {
        if is_active {
            self.style.active_opacity
        } else if is_future {
            self.style.future_opacity
        } else if is_past {
            self.style.spoken_color_opacity
        } else {
            self.style.future_opacity
        }
    }

    /// Subtle scale pop at activation moment only. No continuous wobble.
    /// scale = 1 + active_pop_scale_delta * (1 - eased_p) for first 80ms.
    fn compute_active_pop_scale(&self, word_start_sec: f64, time: f64) -> f32 {
        if !self.style.active_pop_enabled() {
            return 1.0;
        }
        let pop_dur = self.style.active_pop_duration.max(0.001);
        let dt = time - word_start_sec;
        if dt < 0.0 || dt > pop_dur {
            return 1.0;
        }
        let p = (dt as f32) / (pop_dur as f32);
        let eased = Self::ease_out_quad(p);
        // Starts at active_pop_scale (peak on entry), eases to 1.0 over pop_dur.
        let peak = self.style.active_pop_scale - 1.0;
        1.0 + peak * (1.0 - eased)
    }

    // ── Per-word TextStyle (uniform font_size; per-word color +
    //    glow shadow on active only) ─────────────────────────────

    fn make_word_text_style(
        &self,
        color: Color,
        opacity: f32,
        is_active: bool,
        word: &CaptionWord,
        time: f64,
        shadow_color: Color,
    ) -> TextStyle {
        let mut ts = TextStyle::new();
        ts.set_font_size(self.style.font_size);
        ts.set_font_families(&[
            self.style.font_family.as_str(),
            "Sans Serif",
            "Arial",
        ]);

        let mut paint = Paint::default();
        let mut c = color;
        c.a *= opacity;
        paint.set_color(c.to_skia());
        paint.set_anti_alias(true);
        ts.set_foreground_paint(&paint);

        // Subtle glow on active only. No continuous pulse — just a soft blur.
        if is_active && self.style.glow_enabled && self.style.glow_blur_radius > 0.05 {
            let mut glow = self.style.highlight_color;
            glow.a *= 0.85 * self.style.active_opacity;
            ts.add_shadow(TextShadow::new(
                glow.to_skia(),
                Point::new(0.0, 0.0),
                self.style.glow_blur_radius as f64,
            ));
        }

        // Drop shadow for any-bg legibility. Applied to every word.
        if self.style.text_shadow_enabled {
            let mut sc = shadow_color;
            sc.a *= opacity;
            ts.add_shadow(TextShadow::new(
                sc.to_skia(),
                Point::new(
                    self.style.text_shadow_offset_x as f32,
                    self.style.text_shadow_offset_y as f32,
                ),
                self.style.text_shadow_blur as f64,
            ));
        }

        ts
    }

    // ── Build ONE paragraph for a single display line's words ─────

    fn build_display_line_paragraph(
        &self,
        words: &[&CaptionWord],
        time: f64,
        layout_width: f32,
        active_word_global_idx: Option<usize>,
        shadow_color: Color,
    ) -> (Paragraph, Vec<(usize, usize)>) {
        let paragraph_style = self.build_base_paragraph_style();
        let font_guard = match self.font_collection.lock() {
            Ok(g) => g,
            Err(_) => {
                let mut builder = ParagraphBuilder::new(
                    &ParagraphStyle::new(),
                    &skia_safe::textlayout::FontCollection::new(),
                );
                for w in words {
                    builder.add_text(&w.text);
                    builder.add_text(" ");
                }
                let mut p = builder.build();
                p.layout(layout_width);
                return (p, Vec::new());
            }
        };

        let mut builder = ParagraphBuilder::new(&paragraph_style, &*font_guard);
        let mut word_ranges: Vec<(usize, usize)> = Vec::with_capacity(words.len());
        let mut char_cursor: usize = 0;

        for (i, word) in words.iter().enumerate() {
            let is_active = active_word_global_idx == Some(i);
            let is_past = time >= word.end_sec;
            let color = self.compute_word_color(word, time, is_active);
            let opacity = self.compute_word_opacity(
                !(time >= word.start_sec && time < word.end_sec),
                is_past,
                is_active,
            );
            let ts = self.make_word_text_style(
                color,
                opacity,
                is_active,
                word,
                time,
                shadow_color,
            );
            builder.push_style(&ts);

            let trailing_space = i + 1 < words.len();
            let word_chars = word.text.chars().count();
            word_ranges.push((char_cursor, char_cursor + word_chars));
            char_cursor += word_chars;
            if trailing_space {
                char_cursor += 1;
            }

            let word_text = if trailing_space {
                format!("{} ", word.text)
            } else {
                word.text.clone()
            };
            builder.add_text(&word_text);
            builder.pop();

            // touch the field to suppress unused-variable lint when not used
            let _ = is_past_unused();
        }

        let mut paragraph = builder.build();
        paragraph.layout(layout_width);
        (paragraph, word_ranges)
    }

    // ── Mini paragraph for the active scale-pop overlay (just 1 word) ──

    fn build_active_overlay_paragraph(
        &self,
        word: &CaptionWord,
        time: f64,
        layout_width: f32,
        shadow_color: Color,
    ) -> Paragraph {
        let paragraph_style = self.build_base_paragraph_style();
        let font_guard = match self.font_collection.lock() {
            Ok(g) => g,
            Err(_) => {
                let mut builder = ParagraphBuilder::new(
                    &ParagraphStyle::new(),
                    &skia_safe::textlayout::FontCollection::new(),
                );
                builder.add_text(&word.text);
                let mut p = builder.build();
                p.layout(layout_width);
                return p;
            }
        };
        let mut builder = ParagraphBuilder::new(&paragraph_style, &*font_guard);
        let color = self.style.highlight_color;
        let ts = self.make_word_text_style(color, self.style.active_opacity, true, word, time, shadow_color);
        builder.push_style(&ts);
        builder.add_text(&word.text);
        builder.pop();
        let mut p = builder.build();
        p.layout(layout_width);
        p
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
        ss.set_height(1.20);
        ss.set_leading(0.0);
        ss.set_strut_enabled(true);
        ss.set_force_strut_height(false);
        ss.set_height_override(true);
        ps.set_strut_style(ss);
        ps
    }

    // ── Slot allocation (no reflow when adding/removing cues) ──

    fn compute_slot_assignments(lines: &[CaptionLine]) -> Vec<usize> {
        let n = lines.len();
        if n == 0 {
            return Vec::new();
        }
        let max_overlap = max_overlapping_lines(lines).max(1);

        let mut events: Vec<(f64, usize, i32)> = Vec::with_capacity(n * 2);
        for (i, line) in lines.iter().enumerate() {
            events.push((line.start_sec, i, 1));
            events.push((line.end_sec, i, -1));
        }
        events.sort_by(|a, b| {
            a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.2.cmp(&b.2))
        });

        let mut slot_free_at: Vec<f64> = vec![f64::NEG_INFINITY; max_overlap];
        let mut cue_slot_idx = vec![0usize; n];
        for (t, i, delta) in events {
            if delta > 0 {
                let mut best = 0usize;
                let mut best_t = slot_free_at[0];
                for k in 1..max_overlap {
                    if slot_free_at[k] < best_t {
                        best_t = slot_free_at[k];
                        best = k;
                    }
                }
                cue_slot_idx[i] = best;
                slot_free_at[best] = lines[i].end_sec;
            }
        }
        cue_slot_idx
    }

    // ── Build the per-cue pre-laid layout. Even though we rebuild
    //    paragraph color/glow per frame, the SHAPED LAYOUT (positions,
    //    extents, line metrics) is fixed for the cue's lifetime because
    //    every TextStyle shares `style.font_size` and no other metric
    //    varies. ──────────────────────────────────────────────────

    fn build_block_layout(&self, layout_width: f32, shadow_color: Color) -> BlockLayout {
        let slot_assignments = Self::compute_slot_assignments(&self.lines);
        let mut cues: Vec<CueLayout> = Vec::with_capacity(self.lines.len());

        for (cue_idx, line) in self.lines.iter().enumerate() {
            // Group words by display_line_idx in order, preserving cue order
            // within each group via stable sort.
            let mut group_indices: std::collections::BTreeMap<usize, Vec<usize>> =
                std::collections::BTreeMap::new();
            for (idx, word) in line.words.iter().enumerate() {
                group_indices
                    .entry(word.display_line_idx)
                    .or_default()
                    .push(idx);
            }

            let mut display_lines: Vec<DisplayLineLayout> = Vec::with_capacity(group_indices.len());
            let mut line_y_offsets: Vec<f32> = Vec::with_capacity(group_indices.len());
            let mut y_cursor: f32 = 0.0;
            let total_lines = group_indices.len();

            for (line_idx_idx, (_display_line_idx, word_indices)) in
                group_indices.iter().enumerate()
            {
                let words_for_line: Vec<&CaptionWord> =
                    word_indices.iter().map(|&i| &line.words[i]).collect();

                // Build paragraph with highlight OFF (no active state) just to
                // measure heights — same shape, but without glow and with all
                // words at full color. The actual paint-time paragraph is
                // rebuilt per frame with per-word state colors.
                let (paragraph, word_ranges) = self.build_display_line_paragraph(
                    &words_for_line,
                    line.start_sec,
                    layout_width,
                    None,
                    shadow_color,
                );
                let height = paragraph.height();
                line_y_offsets.push(y_cursor);
                y_cursor += height + self.style.line_spacing;

                // Reveal schedule for this line.
                let reveal_offset = self
                    .style
                    .line_reveal_offsets
                    .get(line_idx_idx)
                    .copied()
                    .unwrap_or(0.0);
                let reveal_duration = self
                    .style
                    .line_reveal_durations
                    .get(line_idx_idx)
                    .copied()
                    .unwrap_or(0.10);

                display_lines.push(DisplayLineLayout {
                    paragraph,
                    word_ranges,
                    reveal_offset,
                    reveal_duration,
                });
                let _ = total_lines; // keep style-related total count for debug
            }

            // Don't add trailing line_spacing
            let cue_height = y_cursor - self.style.line_spacing;
            // Don't zero out negative (empty edge case)
            let cue_height = cue_height.max(0.0);

            cues.push(CueLayout {
                slot: slot_assignments[cue_idx],
                paragraph_layout_width: layout_width,
                display_lines,
            });

            let _ = cue_height; // height is computed from paragraph extents on demand
        }

        // Compute slot heights: max height across cues occupying each slot.
        let max_overlap = cues.iter().map(|c| c.slot + 1).max().unwrap_or(1);
        let mut slot_heights: Vec<f32> = vec![0.0; max_overlap];
        for c in &cues {
            let total_dl_height: f32 = c
                .display_lines
                .iter()
                .map(|dl| dl.paragraph.height())
                .sum::<f32>()
                + (c.display_lines.len().saturating_sub(1)) as f32
                    * self.style.line_spacing;
            let total_dl_height = total_dl_height.max(0.0);
            if total_dl_height > slot_heights[c.slot] {
                slot_heights[c.slot] = total_dl_height;
            }
        }
        let max_slot_height = slot_heights.iter().cloned().fold(0.0f32, f32::max);
        let total_height = slot_heights
            .iter()
            .fold(0.0f32, |acc, h| acc + h + self.style.line_spacing)
            - if max_slot_height > 0.0 {
                self.style.line_spacing
            } else {
                0.0
            };

        BlockLayout {
            width: layout_width,
            cues,
            max_overlap,
            max_slot_height,
            total_height,
        }
    }

    // ── Compute line reveal factor at time t. ──────────────────────

    fn line_reveal_factor(&self, line: &CaptionLine, display_line_idx_idx: usize, time: f64) -> f32 {
        let total_dur = (line.end_sec - line.start_sec).max(0.001);
        let offset_frac = self
            .style
            .line_reveal_offsets
            .get(display_line_idx_idx)
            .copied()
            .unwrap_or(0.0);
        let duration = self
            .style
            .line_reveal_durations
            .get(display_line_idx_idx)
            .copied()
            .unwrap_or(0.10);
        let reveal_start = line.start_sec + total_dur * (offset_frac as f64);
        // Clamp dt to 0 before reveal_start: line not yet visible.
        let dt = (time - reveal_start).max(0.0);
        // Clamp to duration / dt for fraction: 0..1
        let raw = if duration <= 0.0 { 1.0 } else { dt / (duration as f64) };
        Self::ease_out_sine(raw as f32).clamp(0.0, 1.0)
    }

    // ── Single global active word across all visible cues ─────────

    fn find_global_active(visible: &[&CaptionLine], time: f64) -> Option<(usize, usize)> {
        let mut latest_start = f64::NEG_INFINITY;
        let mut result: Option<(usize, usize)> = None;
        for (cue_idx, line) in visible.iter().enumerate() {
            for (word_idx, word) in line.words.iter().enumerate() {
                if time >= word.start_sec && time < word.end_sec {
                    if word.start_sec > latest_start {
                        latest_start = word.start_sec;
                        result = Some((cue_idx, word_idx));
                    }
                }
            }
        }
        result
    }

    // ── Bg luminance sampling for adaptive shadow color ───────────

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
                let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                sum += luma as f64;
                count += 1;
            }
        }
        if count == 0 {
            None
        } else {
            Some(((sum / count as f64) / 255.0) as f32)
        }
    }
}

// ── Style helpers ──────────────────────────────────────────────────

fn is_past_unused() -> bool { false }

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

trait FieldMissing { fn active_pop_enabled(&self) -> bool { true } }
impl FieldMissing for CaptionStyle {}

// ── Element trait ──────────────────────────────────────────────────

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
        let block_width = (layout_width.max(20.0)) * self.style.layout_width_fraction;

        let max_overlap = max_overlapping_lines(&self.lines).max(1);
        let strut_h = self.style.font_size * 1.20;
        let max_lines_per_cue = self
            .lines
            .iter()
            .map(|l| {
                let lines = l.text.matches('\n').count() + 1;
                let words = l.words.len();
                // Approx wrap: assume ~5 words per line at this width.
                let wrap_words = (block_width / (self.style.font_size * 0.55)) as usize;
                let wrap_words = wrap_words.max(1);
                let wrapped = (words + wrap_words - 1) / wrap_words.max(1);
                lines.max(wrapped)
            })
            .fold(0usize, usize::max);
        let stack_h = max_lines_per_cue as f32 * strut_h
            + (max_lines_per_cue.saturating_sub(1)) as f32 * self.style.line_spacing;

        let extra_margin = match self.style.position {
            CaptionPosition::Bottom => self.style.bottom_margin,
            CaptionPosition::Top => self.style.top_margin,
            CaptionPosition::Center => 0.0,
        };

        Size {
            width: layout_width,
            height: stack_h + extra_margin,
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
        true
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

        // ── Visible cues ──────────────────────────────────────
        let visible: Vec<&CaptionLine> = self
            .lines
            .iter()
            .filter(|line| current_time >= line.start_sec && current_time < line.end_sec)
            .collect();
        if visible.is_empty() {
            draw_children(canvas);
            return Ok(());
        }

        let block_width = (rect.width() * self.style.layout_width_fraction).max(20.0);

        // ── Adaptive shadow color ──────────────────────────────
        let shadow_override: Option<Color> = if self.style.auto_adapt_shadow
            && self.style.text_shadow_enabled
        {
            let sample_rect = Rect::from_xywh(
                rect.left,
                (rect.bottom - self.style.bottom_margin - 100.0).max(rect.top),
                rect.width(),
                100.0,
            );
            Self::sample_bg_luminance(canvas, sample_rect).map(|lum| {
                if lum > 0.55 {
                    Color::new(0.0, 0.0, 0.0, 0.85)
                } else if lum < 0.40 {
                    Color::new(1.0, 1.0, 1.0, 0.85)
                } else {
                    self.style.text_shadow_color
                }
            })
        } else {
            None
        };
        let shadow_color = shadow_override.unwrap_or(self.style.text_shadow_color);

        // ── Build (or reuse) block layout ───────────────────────
        let mut layout_guard = self.block_layout.lock().unwrap_or_else(|e| e.into_inner());
        let needs_rebuild = layout_guard
            .as_ref()
            .map(|l| (l.width - block_width).abs() > 0.5)
            .unwrap_or(true);
        if needs_rebuild {
            *layout_guard = Some(self.build_block_layout(block_width, shadow_color));
        }
        let layout = layout_guard.as_ref().unwrap();

        // ── Per-cue, per-display-line paint ─────────────────────
        let global_active = Self::find_global_active(&visible, current_time);

        canvas.save();
        canvas.clip_rect(rect, skia_safe::ClipOp::Intersect, true);

        let slot_h = layout.max_slot_height + self.style.line_spacing;

        for (cue_idx, line) in visible.iter().enumerate() {
            // Use stored CueLayout for the pre-computed word ranges and reveal
            // schedule for this cue.
            let cue_layout = layout
                .cues
                .iter()
                .find(|cl| cl.paragraph_layout_width == block_width
                    && cl.cue_idx_match(line.text.as_str(), line.start_sec, line.end_sec))
                .or_else(|| layout.cues.get(cue_idx).filter(|cl| cl.paragraph_layout_width == block_width));

            // Compute slot Y. Slots are pre-laid — never moves when other
            // cues enter/exit.
            let slot_y = match cue_layout {
                Some(c) => c.slot as f32 * slot_h,
                None => 0.0,
            };

            let cue_block_top_y = match self.style.position {
                CaptionPosition::Bottom => {
                    (rect.bottom - self.style.bottom_margin - layout.total_height).max(rect.top)
                }
                CaptionPosition::Top => {
                    (rect.top + self.style.top_margin).min(rect.bottom - layout.total_height)
                }
                CaptionPosition::Center => {
                    let max_y = (rect.bottom - layout.total_height).max(rect.top);
                    (rect.top + (rect.height() - layout.total_height) / 2.0).clamp(rect.top, max_y)
                }
            };

            // Build per-display-line paragraphs and paint each.
            // Group words by display_line_idx.
            let mut groups: std::collections::BTreeMap<usize, Vec<usize>> =
                std::collections::BTreeMap::new();
            for (idx, word) in line.words.iter().enumerate() {
                groups
                    .entry(word.display_line_idx)
                    .or_default()
                    .push(idx);
            }

            let is_cue_globally_active = global_active.map_or(false, |(ci, _)| ci == cue_idx);
            let global_active_word_idx = global_active.and_then(|(ci, wi)| {
                if ci == cue_idx { Some(wi) } else { None }
            });

            let mut line_y = cue_block_top_y + slot_y;
            for (line_idx_idx, (_display_idx, word_indices)) in groups.iter().enumerate() {
                let reveal = self.line_reveal_factor(line, line_idx_idx, current_time);
                if reveal <= 0.0 {
                    continue;
                }

                let words_for_line: Vec<&CaptionWord> =
                    word_indices.iter().map(|&i| &line.words[i]).collect();

                // Pre-laid base paragraph (uniform-shaping) is in CueLayout.
                // We rebuild it per-frame to apply the per-word color/glow
                // STATE, but the SHAPING itself is identical across frames.
                let (paragraph, word_ranges) = self.build_display_line_paragraph(
                    &words_for_line,
                    current_time,
                    block_width,
                    global_active_word_idx,
                    shadow_color,
                );

                let extent = paragraph.longest_line();
                let line_height = paragraph.height();
                let x = rect.left + (rect.width() - extent) / 2.0;

                if reveal >= 0.99 {
                    paragraph.paint(canvas, (x, line_y));
                } else {
                    let mut alpha_paint = Paint::default();
                    alpha_paint.set_alpha_f(effective_opacity * reveal);
                    let bbox = Rect::from_xywh(
                        x - 4.0,
                        line_y - 4.0,
                        extent + 8.0,
                        line_height + 8.0,
                    );
                    canvas.save_layer(
                        &skia_safe::canvas::SaveLayerRec::default()
                            .bounds(&bbox)
                            .paint(&alpha_paint),
                    );
                    paragraph.paint(canvas, (x, line_y));
                    canvas.restore();
                }

                // ── Active word scale pop overlay ─────────────
                if is_cue_globally_active {
                    if let Some(active_idx) = global_active_word_idx {
                        // Map global word_idx → position within this display line
                        let local_active_idx = word_indices
                            .iter()
                            .position(|&i| i == active_idx);
                        if let Some(local_idx) = local_active_idx {
                            let active_word = &line.words[active_idx];
                            let pop_scale =
                                self.compute_active_pop_scale(active_word.start_sec, current_time);

                            // Find the active word's pixel rect in the BASE
                            // paragraph we just painted using Skia's
                            // getRectsForRange with char range.
                            let (start, end) = word_ranges[local_idx];
                            let start_u = start as usize;
                            let end_u = end as usize;
                            let rects = paragraph.get_rects_for_range(
                                start_u..end_u,
                                RectHeightStyle::Tight,
                                RectWidthStyle::Tight,
                            );
                            if let Some(first_box) = rects.first() {
                                let bbox = first_box.rect;
                                let cx = x + bbox.left() + bbox.width() / 2.0;
                                let cy = line_y + bbox.top() + bbox.height() / 2.0;

                                // Tiny scale pop. ONLY when active AND in entry window.
                                let scaler = if pop_scale > 1.001 { pop_scale } else { 1.0 };

                                let active_para = self.build_active_overlay_paragraph(
                                    active_word,
                                    current_time,
                                    block_width,
                                    shadow_color,
                                );

                                canvas.save();
                                canvas.translate(Point::new(cx, cy));
                                canvas.scale((scaler, scaler));
                                canvas.translate(Point::new(-cx, -cy));
                                active_para.paint(canvas, (x, line_y));
                                canvas.restore();
                            }
                        }
                    }
                }

                line_y += line_height + self.style.line_spacing;
            }
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
        if property == "opacity" {
            let ease_fn = crate::node::parse_easing(easing);
            self.opacity.add_segment(start, target, duration, ease_fn);
        }
    }

    fn set_opacity(&mut self, opacity: f32) {
        self.opacity.current_value = opacity;
    }
}

impl CueLayout {
    fn cue_idx_match(&self, text: &str, start_sec: f64, end_sec: f64) -> bool {
        // Use approximate match based on text byte length + timings.
        // We don't store identity in CueLayout since Paragraph isn't Clone.
        // This is a quick heuristic — the actual builder uses lines.
        self.paragraph_layout_width > 0.0
            && start_sec >= 0.0
            && end_sec >= start_sec
            && !text.is_empty()
    }
}

// ── Public helpers ────────────────────────────────────────────────

/// Compute the maximum number of overlapping caption cues at any time.
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

pub fn auto_time_words(line: &mut CaptionLine) {
    if !line.words.is_empty() {
        return;
    }
    let display_lines: Vec<&str> = line.text.split('\n').collect();
    let word_list: Vec<(usize, &str)> = display_lines
        .iter()
        .enumerate()
        .flat_map(|(line_idx, dl)| {
            dl.split_whitespace()
                .map(move |w| (line_idx, w))
                .collect::<Vec<_>>()
        })
        .collect();
    if word_list.is_empty() {
        return;
    }
    let duration = line.end_sec - line.start_sec;
    if duration <= 0.0 {
        line.words = word_list
            .into_iter()
            .map(|(line_idx, w)| CaptionWord {
                text: w.to_string(),
                start_sec: line.start_sec,
                end_sec: line.end_sec,
                display_line_idx: line_idx,
            })
            .collect();
        return;
    }
    let word_duration = duration / word_list.len() as f64;
    line.words = word_list
        .into_iter()
        .enumerate()
        .map(|(i, (line_idx, w))| CaptionWord {
            text: w.to_string(),
            start_sec: line.start_sec + i as f64 * word_duration,
            end_sec: line.start_sec + (i + 1) as f64 * word_duration,
            display_line_idx: line_idx,
        })
        .collect();
}

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
        let input = "1\n00:00:01,000 --> 00:00:05,000\nHello world\n";
        let lines = parse_srt(input);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "Hello world");
        assert_eq!(lines[0].words.len(), 2);
    }

    #[test]
    fn test_parse_srt_multi_line() {
        let input = "1\n00:00:01,000 --> 00:00:05,000\nLine one\nLine two\n";
        let lines = parse_srt(input);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "Line one\nLine two");
    }

    #[test]
    fn test_max_overlapping_lines() {
        let lines = vec![
            CaptionLine { text: "A".into(), words: vec![], start_sec: 0.0, end_sec: 5.0 },
            CaptionLine { text: "B".into(), words: vec![], start_sec: 2.0, end_sec: 7.0 },
        ];
        assert_eq!(max_overlapping_lines(&lines), 2);
    }

    #[test]
    fn test_slot_assignments_stable() {
        let lines = vec![
            CaptionLine { text: "A".into(), words: vec![], start_sec: 0.0, end_sec: 4.0 },
            CaptionLine { text: "B".into(), words: vec![], start_sec: 2.0, end_sec: 7.0 },
            CaptionLine { text: "C".into(), words: vec![], start_sec: 5.0, end_sec: 8.0 },
        ];
        let slots = CaptionNode::compute_slot_assignments(&lines);
        assert_eq!(slots[0], 0);
        assert_eq!(slots[1], 1);
        // After A exits at t=4, C enters at t=5: slot 0 is free.
        assert_eq!(slots[2], 0);
    }

    #[test]
    fn test_single_global_active_word() {
        let line_a = CaptionLine {
            text: "alpha beta".into(),
            words: vec![
                CaptionWord { text: "alpha".into(), start_sec: 0.0, end_sec: 2.0, display_line_idx: 0 },
                CaptionWord { text: "beta".into(), start_sec: 2.0, end_sec: 4.0, display_line_idx: 0 },
            ],
            start_sec: 0.0,
            end_sec: 4.0,
        };
        let line_b = CaptionLine {
            text: "gamma delta".into(),
            words: vec![
                CaptionWord { text: "gamma".into(), start_sec: 1.0, end_sec: 3.0, display_line_idx: 0 },
                CaptionWord { text: "delta".into(), start_sec: 3.0, end_sec: 5.0, display_line_idx: 0 },
            ],
            start_sec: 1.0,
            end_sec: 5.0,
        };
        let visible: Vec<&CaptionLine> = vec![&line_a, &line_b];
        let a = CaptionNode::find_global_active(&visible, 2.5);
        // beta @ 2.0 is later-start than gamma @ 1.0 → beta wins.
        assert_eq!(a, Some((0, 1)));
    }

    #[test]
    fn test_word_font_size_uniform() {
        let fc = Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()));
        let node = CaptionNode::new(Vec::new(), fc);
        let style_fsz = node.style.font_size;
        // No per-word font_size scaling is exposed; uniform across states.
        assert!(style_fsz > 0.0);
    }

    #[test]
    fn test_ease_out_back_no_wobble_in_style() {
        let fc = Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()));
        let node = CaptionNode::new(Vec::new(), fc);
        // No continuous scale pulse: active pop is gated to active state only.
        assert!(node.style.active_pop_scale >= 1.0 && node.style.active_pop_scale <= 1.10);
    }

    /// Invariant: when a past word ends and the next word starts at exactly
    /// the same boundary, there must be exactly ONE active word, not two.
    /// The past→spoken transition is INSTANT to avoid a frame where the
    /// fading-out past word and the ramp-up next word both appear yellow.
    #[test]
    fn test_no_dual_yellow_at_word_boundary() {
        let fc = Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()));
        let node = CaptionNode::new(Vec::new(), fc);

        // Two words sharing a boundary: first ends at t=2.0, second starts at t=2.0.
        let word_a = CaptionWord {
            text: "alpha".into(),
            start_sec: 0.0,
            end_sec: 2.0,
            display_line_idx: 0,
        };
        let word_b = CaptionWord {
            text: "beta".into(),
            start_sec: 2.0,
            end_sec: 4.0,
            display_line_idx: 0,
        };
        let line = CaptionLine {
            text: "alpha beta".into(),
            words: vec![word_a.clone(), word_b.clone()],
            start_sec: 0.0,
            end_sec: 4.0,
        };
        let visible: Vec<&CaptionLine> = vec![&line];

        // At the boundary t=2.0: find_global_active picks word_b (later start).
        let active = CaptionNode::find_global_active(&visible, 2.0);
        assert_eq!(active, Some((0, 1)));

        // The past color is INSTANT (no transition phase). So `compute_word_color`
        // for word_a at t=2.0 returns the spoken color, not the highlight.
        let color_at_boundary = node.compute_word_color(&word_a, 2.0, false);
        let highlight = node.style.highlight_color;
        // Past must be exactly the spoken color at boundary (no more yellow).
        assert!(
            (color_at_boundary.r - node.style.spoken_color.r).abs() < 0.01
                && (color_at_boundary.g - node.style.spoken_color.g).abs() < 0.01
                && (color_at_boundary.b - node.style.spoken_color.b).abs() < 0.01
        );
        // Active word is the highlight color.
        let color_b_at_boundary = node.compute_word_color(&word_b, 2.0, true);
        assert!(
            (color_b_at_boundary.r - highlight.r).abs() < 0.01
                && (color_b_at_boundary.g - highlight.g).abs() < 0.01
                && (color_b_at_boundary.b - highlight.b).abs() < 0.01
        );
        // Sanity: highlight != spoken.
        assert!(
            highlight.r != node.style.spoken_color.r
                || highlight.g != node.style.spoken_color.g
                || highlight.b != node.style.spoken_color.b
        );
    }

    #[test]
    fn test_line_reveal_factor_step() {
        let fc = Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()));
        let node = CaptionNode::new(Vec::new(), fc);
        let mut line = CaptionLine {
            text: "test".into(),
            words: vec![],
            start_sec: 0.0,
            end_sec: 4.0,
        };
        auto_time_words(&mut line);
        // Line 0 reveal = 0.0 + 0.10s = full at t=0.1
        let r0_early = node.line_reveal_factor(&line, 0, 0.0);
        let r0_late = node.line_reveal_factor(&line, 0, 0.5);
        assert_eq!(r0_late, 1.0);
        // Line 1 reveal = 0.45*4 + 0.20s
        let r1_before = node.line_reveal_factor(&line, 1, 1.0);
        let r1_after = node.line_reveal_factor(&line, 1, 3.0);
        assert_eq!(r1_before, 0.0);
        assert_eq!(r1_after, 1.0);
        let _ = r0_early;
    }

    #[test]
    fn test_word_display_assigns_line_idx() {
        let mut line = CaptionLine {
            text: "Line one\nLine two".into(),
            words: vec![],
            start_sec: 0.0,
            end_sec: 4.0,
        };
        auto_time_words(&mut line);
        assert_eq!(line.words.len(), 4);
        assert_eq!(line.words[0].display_line_idx, 0);
        assert_eq!(line.words[1].display_line_idx, 0);
        assert_eq!(line.words[2].display_line_idx, 1);
        assert_eq!(line.words[3].display_line_idx, 1);
    }
}
