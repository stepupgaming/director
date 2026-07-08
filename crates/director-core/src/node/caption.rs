use crate::animation::Animated;
use crate::tokens::DesignSystem;
use crate::element::Element;
use crate::text_direction::{resolve_text_direction, TextDirectionMode};
use crate::types::Color;
use skia_safe::{
    textlayout::{
        FontCollection, Paragraph, ParagraphBuilder, ParagraphStyle, RectHeightStyle,
        RectWidthStyle, StrutStyle, TextAlign, TextHeightBehavior,
        TextShadow, TextStyle,
    },
    font_style::{Slant, Weight, Width},
    Canvas, FontStyle, Paint, Point, Rect, RRect,
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
    /// 0-indexed display line within the cue's text. The renderer groups
    /// words by `display_line_idx` and renders one Paragraph per display
    /// line, stacked vertically at the cue's pre-laid slot.
    pub display_line_idx: usize,
}

/// A single timed caption cue. All tokens belonging to a single cue share a
/// block-level slot which is computed once and is stable for the cue's
/// lifetime. Adding/removing other cues never moves this one's lines.
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
/// white text, warm yellow active word, restrained shadow only.
/// Uniform font_size across all token states → glyph metrics never
/// vary → no neighbor shifting when state changes.
#[derive(Clone, Debug)]
pub struct CaptionStyle {
    pub font_size: f32,
    pub color: Color,            // future words (default white)
    pub highlight_color: Color,  // active word (default warm gold)
    pub spoken_color: Color,     // past words (default dim white)

    pub position: CaptionPosition,
    pub text_align: TextAlign,
    /// Paragraph direction: auto-detect from cue text, or force LTR/RTL.
    pub text_direction: TextDirectionMode,
    pub font_family: String,
    /// Font weight (400 = regular, 700 = bold). Uses bundled family weights.
    pub font_weight: u16,
    pub bottom_margin: f32,
    pub top_margin: f32,
    pub line_spacing: f32,
    pub layout_width_fraction: f32, // fraction of canvas width

    // Per-word opacity for each state.
    pub active_opacity: f32,
    pub future_opacity: f32,
    pub spoken_color_opacity: f32,  // alpha multiplier for spoken color

    // Future→active entry ramp (smooth, no overshoot).
    pub color_transition_duration: f64, // seconds

    // Active word emphasis: small scale pop at activation moment only.
    // The pop is implemented as a per-token canvas-scale transform applied
    // around the active token's PRE-COMPUTED center. Implemented as a
    // single-token glyph paragraph that replaces the active-token run in
    // the base paragraph for the duration of the pop, so exactly ONE
    // active token is drawn per frame and never at an offset location.
    pub active_pop_scale: f32,
    pub active_pop_duration: f64,

    // Drop shadow for any-bg legibility.
    pub text_shadow_enabled: bool,
    pub text_shadow_color: Color,
    pub text_shadow_offset_x: f32,
    pub text_shadow_offset_y: f32,
    pub text_shadow_blur: f32,

    pub auto_adapt_shadow: bool,

    // Outline stroke for any-background legibility (8-direction shadow trick).
    pub stroke_enabled: bool,
    pub stroke_width: f32,
    pub stroke_color: Color,

    // Active-word glow: a soft colored halo around each word (most visible on
    // the active word). Documented in AGENTS.md; previously unimplemented.
    pub glow_enabled: bool,
    pub glow_blur: f32,

    // Per-display-line reveal.
    pub line_reveal_offsets: Vec<f32>,    // fraction of CUE duration
    pub line_reveal_durations: Vec<f32>,  // seconds; fade-in duration

    // Rounded pill behind each display line (OpusClip / CapCut tier legibility).
    pub pill_enabled: bool,
    pub pill_color: Color,
    pub pill_adaptive: bool,
    pub pill_padding_h: f32,
    pub pill_padding_v: f32,
    pub pill_radius: f32,

    // Auto-split single-line cues into phrase chunks (e.g. 4 words per line).
    pub auto_phrase_break: bool,
    pub max_words_per_line: usize,

    // Platform safe-area preset (`tiktok`, `youtube_shorts`, `instagram_reel`, …).
    pub safe_area_preset: Option<String>,

    // Scale font/margins/pill from `reference_canvas_height` (default 1920).
    pub scale_with_canvas: bool,
    pub reference_canvas_height: f32,

    /// Render all words uppercase (layout-stable; applied at shape time).
    pub uppercase: bool,
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
            text_direction: TextDirectionMode::Auto,
            font_family: "Poppins".to_string(),
            font_weight: 700,
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

            text_shadow_enabled: true,
            text_shadow_color: Color::new(0.0, 0.0, 0.0, 0.85),
            text_shadow_offset_x: 0.0,
            text_shadow_offset_y: 2.0,
            text_shadow_blur: 4.0,

            auto_adapt_shadow: true,

            stroke_enabled: true,
            stroke_width: 2.0,
            stroke_color: Color::new(0.0, 0.0, 0.0, 0.92),

            glow_enabled: true,
            glow_blur: 4.0,

            line_reveal_offsets: vec![0.0, 0.45],
            line_reveal_durations: vec![0.10, 0.20],

            pill_enabled: true,
            pill_color: Color::new(0.0, 0.0, 0.0, 0.62),
            pill_adaptive: true,
            pill_padding_h: 18.0,
            pill_padding_v: 10.0,
            pill_radius: 14.0,

            auto_phrase_break: true,
            max_words_per_line: 4,

            safe_area_preset: None,

            scale_with_canvas: true,
            reference_canvas_height: REFERENCE_SHORT_FORM_HEIGHT,

            uppercase: false,
        }
    }
}

/// Per-line, per-token layout. Computed ONCE at measure time and is
/// immutable for the cue's lifetime. The x/baselineY/center values are
/// the canonical positions used both for the base render pass and any
/// active-word emphasis overlay scale transform.
#[derive(Clone, Debug)]
pub struct TokenLayout {
    pub word_idx: usize,
    pub text: String,
    pub x: f32,         // left pixel of glyph run within line
    pub baseline_y: f32, // y of line for this token
    pub width: f32,
    pub height: f32,
    pub center_x: f32,  // x + width/2 (post-shaping, used for pop transform)
    pub center_y: f32,  // line_baseline_y for transform
    pub line_index: usize, // 0-indexed display line within cue
}

/// Per-display-line layout. Final canonical positions for every token
/// on this line. Built ONCE per cue lifetime and reused per frame.
#[derive(Clone, Debug)]
pub struct DisplayLineLayout {
    pub paragraph_layout_width: f32,
    /// Pre-computed width of the line (longest_line()) — used to center.
    pub line_width: f32,
    /// Pre-computed height of the line — used to stack lines vertically.
    pub line_height: f32,
    /// One immutable layout entry per token on this display line, in
    /// order. `word_idx` is into the cue's `CaptionLine.words`.
    pub tokens: Vec<TokenLayout>,
    pub reveal_offset: f32,
    pub reveal_duration: f32,
}

#[derive(Clone, Debug)]
pub struct CueLayout {
    pub slot: usize,
    pub display_lines: Vec<DisplayLineLayout>,
}

#[derive(Clone, Debug)]
pub struct BlockLayout {
    pub width: f32,
    pub canvas_height: f32,
    pub cues: Vec<CueLayout>,
    pub max_overlap: usize,
    pub max_slot_height: f32,
    pub total_height: f32,
}

/// A node for rendering timed caption text (SRT/WebVTT subtitles).
/// Architecture: pre-laid-out structurally stable text blocks. Each
/// token has ONE canonical `(x, baselineY, width, height, centerX,
/// centerY, lineIndex)`. Active emphasis never re-measures positions;
/// it scale-transforms around the cached token center and only paints
/// the active token via its own per-token glyph paragraph during the
/// optional pop window. Single token = single draw.
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
    pub fn new(mut lines: Vec<CaptionLine>, font_collection: Arc<Mutex<FontCollection>>) -> Self {
        let style = CaptionStyle::default();
        refresh_caption_lines(&mut lines, &style);
        Self {
            lines,
            style,
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

    /// Re-apply phrase breaks and drop cached layout after style / line edits.
    pub(crate) fn after_style_changed(&mut self) {
        refresh_caption_lines(&mut self.lines, &self.style);
        if let Ok(mut guard) = self.block_layout.lock() {
            *guard = None;
        }
    }

    // ── Easings (smooth, no overshoot) ────────────────────────────

    fn ease_out_sine(t: f32) -> f32 {
        let x = t.clamp(0.0, 1.0);
        1.0 - (x * std::f32::consts::FRAC_PI_2).cos()
    }

    // ── Per-word color at time t. Uniform font_size ⇒ no scale or
    //    shaping variation; only color varies. Past is INSTANT to
    //    keep exactly one active token per frame. ────────────────

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
            // Smooth future→active entry over `color_transition_duration`
            // before `start`. At `time = start - t_dur` we are 100% base
            // color; at `time = start` we are 100% highlight (entry frame).
            if time > start - t_dur {
                let p = ((start - time) / t_dur) as f32;
                let p = p.clamp(0.0, 1.0);
                return lerp_color(self.style.highlight_color, self.style.color, p);
            }
            return self.style.color;
        }
        if is_past {
            // Instant past→spoken. Smooth transitions on the past side
            // would create a frame where a fading-out past word and a
            // ramping-in next word both register as yellow, breaking
            // the exactly-one-active-word invariant.
            return self.style.spoken_color;
        }
        // During cue but outside word window: future color.
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

    /// Active pop scale: returns scale > 1.0 only during the first
    /// `active_pop_duration` seconds after the active token's
    /// `start_sec`. Returns 1.0 outside that window (no overlay).
    fn active_pop_scale_at(&self, word_start_sec: f64, time: f64) -> f32 {
        let pop_dur = self.style.active_pop_duration.max(0.001);
        let dt = time - word_start_sec;
        if dt < 0.0 || dt > pop_dur {
            return 1.0;
        }
        let p = (dt as f32) / (pop_dur as f32);
        // Peak (1.0 + peak_delta) at p=0, settle to 1.0 at p=1.
        let eased = Self::ease_out_sine(p);
        let peak = (self.style.active_pop_scale - 1.0).max(0.0);
        1.0 + peak * (1.0 - eased)
    }

    // ── Build base ParagraphStyle ─────────────────────────────────

    fn build_base_paragraph_style(style: &CaptionStyle, line_text: &str) -> ParagraphStyle {
        let mut ps = ParagraphStyle::new();
        // CRITICAL: text-align must be LEFT (Start). Caption block
        // centering is done by the renderer via `line_left_x`, NOT by
        // Skia's internal alignment. Skia's Center alignment shifts
        // glyph runs inside the layout width, breaking per-token
        // canonical x positions.
        ps.set_text_align(TextAlign::Left);
        ps.set_text_direction(resolve_text_direction(
            style.text_direction,
            line_text,
        ));
        ps.set_text_height_behavior(TextHeightBehavior::DisableAll);
        ps.set_replace_tab_characters(true);

        let mut ss = StrutStyle::new();
        ss.set_font_families(&[style.font_family.as_str()]);
        ss.set_font_size(style.font_size);
        ss.set_height(1.20);
        ss.set_leading(0.0);
        ss.set_strut_enabled(true);
        ss.set_force_strut_height(false);
        ss.set_height_override(true);
        ps.set_strut_style(ss);
        ps
    }

    // ── Per-word TextStyle ─────────────────────────────────────────

    fn make_word_text_style(
        style: &CaptionStyle,
        color: Color,
        opacity: f32,
        _word: &CaptionWord,
        shadow_color: Color,
        is_active: bool,
    ) -> TextStyle {
        let mut ts = TextStyle::new();
        ts.set_font_size(style.font_size);
        ts.set_font_families(&[
            style.font_family.as_str(),
            "Roboto",
            "Sans Serif",
            "Arial",
        ]);
        ts.set_font_style(FontStyle::new(
            Weight::from(style.font_weight as i32),
            Width::NORMAL,
            Slant::Upright,
        ));

        let mut paint = Paint::default();
        let mut c = color;
        c.a *= opacity;
        paint.set_color(c.to_skia());
        paint.set_anti_alias(true);
        ts.set_foreground_paint(&paint);

        if style.stroke_enabled && style.stroke_width > 0.0 {
            let mut sc = style.stroke_color;
            sc.a *= opacity;
            let w = style.stroke_width;
            for (dx, dy) in [
                (-1.0, 0.0),
                (1.0, 0.0),
                (0.0, -1.0),
                (0.0, 1.0),
                (-0.7, -0.7),
                (0.7, -0.7),
                (-0.7, 0.7),
                (0.7, 0.7),
            ] {
                ts.add_shadow(TextShadow::new(
                    sc.to_skia(),
                    Point::new(dx * w, dy * w),
                    0.0,
                ));
            }
        }

        if style.text_shadow_enabled {
            let mut sc = shadow_color;
            sc.a *= opacity;
            ts.add_shadow(TextShadow::new(
                sc.to_skia(),
                Point::new(
                    style.text_shadow_offset_x,
                    style.text_shadow_offset_y,
                ),
                style.text_shadow_blur as f64,
            ));
        }

        if style.glow_enabled && is_active {
            let mut gc = style.highlight_color;
            gc.a *= opacity * 0.65;
            ts.add_shadow(TextShadow::new(
                gc.to_skia(),
                Point::new(0.0, 0.0),
                style.glow_blur as f64,
            ));
        }

        ts
    }

    fn paint_line_pill(
        canvas: &Canvas,
        line_left_x: f32,
        line_y: f32,
        display_line: &DisplayLineLayout,
        style: &CaptionStyle,
        pill_color: Color,
        alpha: f32,
    ) {
        if !style.pill_enabled {
            return;
        }
        let pad_h = style.pill_padding_h;
        let pad_v = style.pill_padding_v;
        let pill_rect = Rect::from_xywh(
            line_left_x - pad_h,
            line_y - pad_v,
            display_line.line_width + pad_h * 2.0,
            display_line.line_height + pad_v * 2.0,
        );
        let rrect = RRect::new_rect_xy(&pill_rect, style.pill_radius, style.pill_radius);
        let mut paint = Paint::default();
        let mut c = pill_color;
        c.a *= alpha;
        paint.set_color(c.to_skia());
        paint.set_anti_alias(true);
        canvas.draw_rrect(rrect, &paint);
    }

    // ── Slot allocation (no reflow when adding/removing cues) ─────

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

    // ── Build the per-cue pre-laid layout. Each token gets its
    //    immutable x / baselineY / centerX / centerY / width /
    //    height / lineIndex ONCE here. These values do not change
    //    frame-to-frame because every TextStyle shares `font_size`
    //    and per-word foreground paint only varies color. ─────────

    fn build_block_layout(
        &self,
        layout_width: f32,
        canvas_h: f32,
        shadow_color: Color,
    ) -> BlockLayout {
        let style = self.style.resolved_for_canvas(canvas_h);
        let slot_assignments = Self::compute_slot_assignments(&self.lines);
        let mut cues: Vec<CueLayout> = Vec::with_capacity(self.lines.len());

        for (cue_idx, line) in self.lines.iter().enumerate() {
            // Group words by display_line_idx (preserve order in group).
            let mut group_indices: std::collections::BTreeMap<usize, Vec<usize>> =
                std::collections::BTreeMap::new();
            for (idx, word) in line.words.iter().enumerate() {
                group_indices
                    .entry(word.display_line_idx)
                    .or_default()
                    .push(idx);
            }

            let mut display_lines: Vec<DisplayLineLayout> =
                Vec::with_capacity(group_indices.len());

            for (line_idx_idx, (_display_line_idx, word_indices)) in
                group_indices.iter().enumerate()
            {
                // Build "measure-only" paragraph with ALL tokens (uniform
                // shaping — every word's TextStyle has the same
                // `font_size`, just neutral color). We then use Skia's
                // get_rects_for_range() to obtain each token's actual
                // pixel bbox within the laid-out paragraph. Those x/y
                // values are the canonical token positions.
                let words_for_line: Vec<&CaptionWord> =
                    word_indices.iter().map(|&i| &line.words[i]).collect();

                // Layout the measure paragraph with the SAME layout
                // width as the per-frame paragraph so token x positions
                // are identical (no wrapping shift). Use a very large
                // fallback if layout_width is 0 to keep words on a
                // single line.
                let measure_layout_width = if layout_width > 1.0 {
                    layout_width
                } else {
                    4096.0
                };
                let (paragraph, token_char_ranges) = self.build_measure_paragraph(
                    &style,
                    &words_for_line,
                    measure_layout_width,
                    shadow_color,
                );
                let line_width = paragraph.longest_line();
                let line_height = paragraph.height();

                // Cache token layouts via get_rects_for_range (Tight).
                let mut tokens: Vec<TokenLayout> = Vec::with_capacity(words_for_line.len());
                for (i, word) in words_for_line.iter().enumerate() {
                    let (cs, ce) = token_char_ranges[i];
                    let rects = paragraph.get_rects_for_range(
                        cs..ce,
                        RectHeightStyle::Tight,
                        RectWidthStyle::Tight,
                    );
                    let rect = if let Some(first) = rects.first() {
                        first.rect
                    } else {
                        Rect::from_xywh(0.0, 0.0, 0.0, 0.0)
                    };
                    tokens.push(TokenLayout {
                        word_idx: word_indices[i],
                        text: word.text.clone(),
                        x: rect.left() as f32,
                        baseline_y: rect.top() as f32, // paragraph-local
                        width: rect.width() as f32,
                        height: rect.height() as f32,
                        center_x: rect.left() as f32 + rect.width() as f32 * 0.5,
                        center_y: rect.top() as f32 + rect.height() as f32 * 0.5,
                        line_index: line_idx_idx,
                    });
                }

                let reveal_offset = style
                    .line_reveal_offsets
                    .get(line_idx_idx)
                    .copied()
                    .unwrap_or(0.0);
                let reveal_duration = style
                    .line_reveal_durations
                    .get(line_idx_idx)
                    .copied()
                    .unwrap_or(0.10);

                display_lines.push(DisplayLineLayout {
                    paragraph_layout_width: layout_width,
                    line_width,
                    line_height,
                    tokens,
                    reveal_offset,
                    reveal_duration,
                });
            }

            cues.push(CueLayout {
                slot: slot_assignments[cue_idx],
                display_lines,
            });
            let _ = cue_idx; // silence unused-context warning
        }

        let max_overlap = cues.iter().map(|c| c.slot + 1).max().unwrap_or(1);
        let mut slot_heights: Vec<f32> = vec![0.0; max_overlap];
        for c in &cues {
            let total_dl_height: f32 = c
                .display_lines
                .iter()
                .map(|dl| dl.line_height)
                .sum::<f32>()
                + (c.display_lines.len().saturating_sub(1)) as f32
                    * style.line_spacing;
            if total_dl_height > slot_heights[c.slot] {
                slot_heights[c.slot] = total_dl_height;
            }
        }
        let max_slot_height = slot_heights.iter().cloned().fold(0.0f32, f32::max);
        let total_height = slot_heights
            .iter()
            .fold(0.0f32, |acc, h| acc + h + style.line_spacing)
            - if max_slot_height > 0.0 {
                style.line_spacing
            } else {
                0.0
            };

        BlockLayout {
            width: layout_width,
            canvas_height: canvas_h,
            cues,
            max_overlap,
            max_slot_height,
            total_height,
        }
    }

    #[doc(hidden)]
    pub fn debug_dump_block_layout(&self, layout_width: f32) -> String {
        let shadow = Color::new(0.0, 0.0, 0.0, 0.85);
        let layout = self.build_block_layout(layout_width, 1080.0, shadow);
        let mut out = String::new();
        for (ci, cue) in layout.cues.iter().enumerate() {
            out.push_str(&format!("cue[{}] slot={} display_lines={}\n", ci, cue.slot, cue.display_lines.len()));
            for (li, dl) in cue.display_lines.iter().enumerate() {
                out.push_str(&format!("  line[{}] tokens={} line_w={:.1} line_h={:.1}\n",
                    li, dl.tokens.len(), dl.line_width, dl.line_height));
                for t in &dl.tokens {
                    out.push_str(&format!("    word_idx={} '{}' x={:.1} cy={:.1}\n",
                        t.word_idx, t.text, t.x, t.center_y));
                }
            }
        }
        out
    }

    /// Build a measure-only paragraph: every token in this display line
    /// gets identical TextStyle except color. No continuous motion
    /// affecting layout. Used to compute CANONICAL token bboxes via
    /// `get_rects_for_range`. The layout_width MUST match the
    /// per-frame paragraph's layout_width so token x positions are
    /// identical (no wrapping).
    fn build_measure_paragraph(
        &self,
        style: &CaptionStyle,
        words: &[&CaptionWord],
        layout_width: f32,
        shadow_color: Color,
    ) -> (Paragraph, Vec<(usize, usize)>) {
        let line_text: String = words
            .iter()
            .map(|w| format_caption_word(&w.text, style.uppercase))
            .collect::<Vec<_>>()
            .join(" ");
        let paragraph_style = Self::build_base_paragraph_style(style, &line_text);
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
        let mut char_cursor: usize = 0;
        let mut token_char_ranges: Vec<(usize, usize)> = Vec::with_capacity(words.len());

        for (i, word) in words.iter().enumerate() {
            // For measure / canonical layout: highlight color is fine,
            // it does not affect shaping. We just need font_size uniform.
            let ts = Self::make_word_text_style(
                style,
                style.color,
                style.future_opacity,
                word,
                shadow_color,
                false,
            );
            builder.push_style(&ts);

            let trailing_space = i + 1 < words.len();
            let display_text = format_caption_word(&word.text, style.uppercase);
            let word_chars = display_text.chars().count();
            token_char_ranges.push((char_cursor, char_cursor + word_chars));
            char_cursor += word_chars;
            if trailing_space {
                char_cursor += 1;
            }

            let word_text = if trailing_space {
                format!("{} ", display_text)
            } else {
                display_text
            };
            builder.add_text(&word_text);
            builder.pop();
        }

        let mut paragraph = builder.build();
        paragraph.layout(layout_width);
        (paragraph, token_char_ranges)
    }

    // ── Single global active word across all visible cues ─────────

    /// Returns `(line_index, word_index)` using indices into `CaptionNode::lines`,
    /// not indices into the visible-cue subset.
    fn find_global_active(visible: &[(usize, &CaptionLine)], time: f64) -> Option<(usize, usize)> {
        let mut latest_start = f64::NEG_INFINITY;
        let mut result: Option<(usize, usize)> = None;
        for &(line_idx, line) in visible {
            for (word_idx, word) in line.words.iter().enumerate() {
                if time >= word.start_sec && time < word.end_sec {
                    if word.start_sec > latest_start {
                        latest_start = word.start_sec;
                        result = Some((line_idx, word_idx));
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

// ── Style helpers ─────────────────────────────────────────────────

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

// ── Element trait ─────────────────────────────────────────────────

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
        let canvas_h = match available_space.height {
            AvailableSpace::Definite(h) => h,
            _ => known_dimensions.height.unwrap_or(1080.0),
        };
        let layout_style = self.style.resolved_for_canvas(canvas_h);
        let block_width = (layout_width.max(20.0)) * layout_style.layout_width_fraction;

        // Derive height from real SkParagraph metrics (same path as render) so
        // wrap and multi-slot stacking match the painted layout.
        let stack_h = {
            let mut guard = self.block_layout.lock().unwrap_or_else(|e| e.into_inner());
            let needs_rebuild = guard
                .as_ref()
                .map(|l| {
                    (l.width - block_width).abs() > 0.5
                        || (l.canvas_height - canvas_h).abs() > 1.0
                })
                .unwrap_or(true);
            if needs_rebuild {
                *guard = Some(self.build_block_layout(
                    block_width,
                    canvas_h,
                    self.style.text_shadow_color,
                ));
            }
            guard.as_ref().map(|l| l.total_height).unwrap_or(0.0)
        };

        let extra_margin = match layout_style.position {
            CaptionPosition::Bottom => layout_style.bottom_margin,
            CaptionPosition::Top => layout_style.top_margin,
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

        // Visible cues — retain original `lines` indices for layout lookup.
        let visible: Vec<(usize, &CaptionLine)> = self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| current_time >= line.start_sec && current_time < line.end_sec)
            .collect();
        if visible.is_empty() {
            draw_children(canvas);
            return Ok(());
        }

        let canvas_h = rect.height();
        let layout_style = self.style.resolved_for_canvas(canvas_h);
        let block_width = (rect.width() * layout_style.layout_width_fraction).max(20.0);

        // Adaptive shadow + pill colors from background luminance.
        let caption_sample_rect = Rect::from_xywh(
            rect.left,
            (rect.bottom - layout_style.bottom_margin - 120.0).max(rect.top),
            rect.width(),
            120.0,
        );
        let bg_luma = Self::sample_bg_luminance(canvas, caption_sample_rect);

        let shadow_override: Option<Color> = if self.style.auto_adapt_shadow
            && self.style.text_shadow_enabled
        {
            bg_luma.map(|lum| {
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
        let pill_color = layout_style.resolve_pill_color(bg_luma);

        // Build / reuse block layout.
        let mut layout_guard = self.block_layout.lock().unwrap_or_else(|e| e.into_inner());
        let needs_rebuild = layout_guard
            .as_ref()
            .map(|l| {
                (l.width - block_width).abs() > 0.5
                    || (l.canvas_height - canvas_h).abs() > 1.0
            })
            .unwrap_or(true);
        if needs_rebuild {
            *layout_guard = Some(self.build_block_layout(block_width, canvas_h, shadow_color));
        }

        let layout = match layout_guard.as_ref() {
            Some(l) => l,
            None => {
                draw_children(canvas);
                return Ok(());
            }
        };

        // ── Single canonical global active token ────────────────
        let global_active = Self::find_global_active(&visible, current_time);

        canvas.save();
        canvas.clip_rect(rect, skia_safe::ClipOp::Intersect, true);

        let slot_h = layout.max_slot_height + layout_style.line_spacing;

        // The active token's pop scale: > 1.0 only during the ~80ms
        // entry window. Outside this window, scale == 1.0 and the
        // active token's paint is identical to its base paint — no
        // duplicate draw.
        let pop_scale = match global_active {
            Some((line_idx, word_idx)) => {
                let active_word = &self.lines[line_idx].words[word_idx];
                self.active_pop_scale_at(active_word.start_sec, current_time)
            }
            None => 1.0,
        };
        let active_pop_active = pop_scale > 1.001;

        // ── Render each visible cue, each display line. ─────────
        for &(line_idx, line) in &visible {
            let cue_layout = match layout.cues.get(line_idx) {
                Some(c) => c,
                None => continue,
            };

            // Block top Y, depending on caption position.
            let cue_block_top_y = match layout_style.position {
                CaptionPosition::Bottom => {
                    (rect.bottom - layout_style.bottom_margin - layout.total_height).max(rect.top)
                }
                CaptionPosition::Top => {
                    (rect.top + layout_style.top_margin).min(rect.bottom - layout.total_height)
                }
                CaptionPosition::Center => {
                    let max_y = (rect.bottom - layout.total_height).max(rect.top);
                    (rect.top + (rect.height() - layout.total_height) / 2.0)
                        .clamp(rect.top, max_y)
                }
            };

            let slot_y = cue_layout.slot as f32 * slot_h;
            let mut line_y_cursor: f32 = cue_block_top_y + slot_y;

            // Active token info, if any, for THIS cue.
            let active_in_this_cue: Option<&TokenLayout> = match global_active {
                Some((ci, wi)) if ci == line_idx => cue_layout
                    .display_lines
                    .iter()
                    .flat_map(|dl| dl.tokens.iter())
                    .find(|t| t.word_idx == wi),
                _ => None,
            };

            for (line_idx_idx, display_line) in cue_layout.display_lines.iter().enumerate() {
                let reveal = line_reveal_factor(
                    line,
                    line_idx_idx,
                    current_time,
                    &layout_style,
                );
                if reveal <= 0.0 {
                    line_y_cursor += display_line.line_height + self.style.line_spacing;
                    continue;
                }

                // ── Center each DISPLAY LINE individually using
                //    precomputed `line_width` — never use a token
                //    origin as the line's left edge. ─────────────
                let rect_w = rect.width();
                let line_left_x = rect.left + (rect_w - display_line.line_width) / 2.0;

                let line_render_alpha = effective_opacity * reveal;
                let line_needs_layer = reveal < 0.99;

                // Build ONE per-frame line paragraph containing ALL
                // tokens on this line, with per-token foreground
                // colors reflecting the current time state. The line
                // paragraph's glyph layout is identical to the cached
                // measure paragraph because every TextStyle shares
                // `font_size` and `font_family`; only the foreground
                // paint's color varies per token. Therefore this
                // paragraph's glyph run x/y values match the cached
                // TokenLayout x/y within sub-pixel precision.
                let active_local_in_line: Option<usize> =
                    if active_in_this_cue.is_some() {
                        let target_word_idx =
                            active_in_this_cue.as_ref().unwrap().word_idx;
                        display_line.tokens.iter().position(|t| t.word_idx == target_word_idx)
                    } else {
                        None
                    };

                // ── Apply reveal as a save-layer alpha when reveal
                //    < 1.0. ─────────────────────────────────────
                if line_needs_layer {
                    let mut alpha_paint = Paint::default();
                    alpha_paint.set_alpha_f(line_render_alpha);
                    let bbox = Rect::from_xywh(
                        line_left_x - layout_style.pill_padding_h - 4.0,
                        line_y_cursor - layout_style.pill_padding_v - 4.0,
                        display_line.line_width + (layout_style.pill_padding_h + 4.0) * 2.0,
                        display_line.line_height + (layout_style.pill_padding_v + 4.0) * 2.0,
                    );
                    canvas.save_layer(
                        &skia_safe::canvas::SaveLayerRec::default()
                            .bounds(&bbox)
                            .paint(&alpha_paint),
                    );
                }

                Self::paint_line_pill(
                    canvas,
                    line_left_x,
                    line_y_cursor,
                    display_line,
                    &layout_style,
                    pill_color,
                    if line_needs_layer { 1.0 } else { line_render_alpha },
                );

                // ── If pop-active and THIS line contains the active
                //    token, paint the entire line paragraph INSIDE a
                //    transformed, clipped canvas such that only the
                //    active word's bbox is visible, scaled around the
                //    cached token center. This guarantees ONE draw of
                //    the line per frame (no second paragraph; no
                //    separately-measured coordinates for the active
                //    word). ──────────────────────────────────
                if active_pop_active && active_local_in_line.is_some() {
                    let local_idx = active_local_in_line.unwrap();
                    let active_token = &display_line.tokens[local_idx];
                    let cx = line_left_x + active_token.center_x;
                    let cy = line_y_cursor + active_token.center_y;

                    // Clipped canvas region = active word's bbox.
                    let clip_bbox = Rect::from_xywh(
                        line_left_x + active_token.x - 2.0,
                        line_y_cursor + active_token.baseline_y - 2.0,
                        active_token.width + 4.0,
                        active_token.height + 4.0,
                    );

                    canvas.save();
                    canvas.clip_rect(clip_bbox, skia_safe::ClipOp::Intersect, true);
                    canvas.translate(Point::new(cx, cy));
                    canvas.scale((pop_scale, pop_scale));
                    canvas.translate(Point::new(-cx, -cy));

                    // Paint the SAME line paragraph at the line's
                    // canonical line_left / line_y origin. The clip
                    // and scale transform restrict visible output to
                    // the active word's bbox scaled around its center.
                    self.paint_line_paragraph_at(
                        &layout_style,
                        display_line,
                        line,
                        current_time,
                        block_width,
                        active_local_in_line,
                        shadow_color,
                        canvas,
                        line_left_x,
                        line_y_cursor,
                    );
                    canvas.restore();
                } else {
                    // No pop — paint the line paragraph once at its
                    // canonical origin. The active word (if any) is
                    // rendered at highlight color via the per-token
                    // TextStyle foreground paint.
                    self.paint_line_paragraph_at(
                        &layout_style,
                        display_line,
                        line,
                        current_time,
                        block_width,
                        active_local_in_line,
                        shadow_color,
                        canvas,
                        line_left_x,
                        line_y_cursor,
                    );
                }

                if line_needs_layer {
                    canvas.restore();
                }

                line_y_cursor += display_line.line_height + layout_style.line_spacing;
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

impl CaptionNode {
    // ── Build a per-frame Paragraph for a single display line.
    // Includes ALL tokens on the line. Per-token foreground color is
    // selected based on word state at `current_time`. Active token's
    // foreground is the highlight color. This paragraph's glyph
    // x/baselineY positions are IDENTICAL to the cached measure-time
    // paragraph because every TextStyle in both shares `font_size`,
    // `font_family`, and identical FontCollection state. Therefore,
    // painting this paragraph at the line's cached line_left_x /
    // line_y_cursor places every glyph inside its cached TokenLayout
    // bbox.
    fn build_line_paragraph_for_frame(
        &self,
        style: &CaptionStyle,
        display_line: &DisplayLineLayout,
        line: &CaptionLine,
        time: f64,
        layout_width: f32,
        active_local_in_line: Option<usize>,
        shadow_color: Color,
    ) -> Paragraph {
        let line_text: String = display_line
            .tokens
            .iter()
            .map(|t| format_caption_word(&t.text, style.uppercase))
            .collect::<Vec<_>>()
            .join(" ");
        let paragraph_style = Self::build_base_paragraph_style(style, &line_text);
        let font_guard = match self.font_collection.lock() {
            Ok(g) => g,
            Err(_) => {
                let mut builder = ParagraphBuilder::new(
                    &ParagraphStyle::new(),
                    &skia_safe::textlayout::FontCollection::new(),
                );
                builder.add_text("");
                return builder.build();
            }
        };
        let mut builder = ParagraphBuilder::new(&paragraph_style, &*font_guard);
        for (local_idx, token) in display_line.tokens.iter().enumerate() {
            let word = &line.words[token.word_idx];
            let is_active = Some(local_idx) == active_local_in_line;

            let color = if is_active {
                self.style.highlight_color
            } else if time >= word.end_sec {
                self.style.spoken_color
            } else if time < word.start_sec {
                self.compute_word_color(word, time, false)
            } else {
                self.style.color
            };
            let opacity = if is_active {
                self.style.active_opacity
            } else if time >= word.end_sec {
                self.style.spoken_color_opacity
            } else {
                self.style.future_opacity
            };
            let ts = Self::make_word_text_style(
                style,
                color,
                opacity,
                word,
                shadow_color,
                is_active,
            );
            builder.push_style(&ts);

            let trailing_space = local_idx + 1 < display_line.tokens.len();
            let display_text = format_caption_word(&word.text, style.uppercase);
            let text = if trailing_space {
                format!("{} ", display_text)
            } else {
                display_text
            };
            builder.add_text(&text);
            builder.pop();
        }
        let mut p = builder.build();
        p.layout(layout_width);
        p
    }

    // ── Build + paint the line paragraph in one step. ───────────
    fn paint_line_paragraph_at(
        &self,
        style: &CaptionStyle,
        display_line: &DisplayLineLayout,
        line: &CaptionLine,
        time: f64,
        layout_width: f32,
        active_local_in_line: Option<usize>,
        shadow_color: Color,
        canvas: &Canvas,
        x: f32,
        y: f32,
    ) {
        let p = self.build_line_paragraph_for_frame(
            style,
            display_line,
            line,
            time,
            layout_width,
            active_local_in_line,
            shadow_color,
        );
        p.paint(canvas, (x, y));
    }
}

// ── Public helpers ────────────────────────────────────────────────

/// Maximum line overlap across the entire caption timeline.
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

/// Line-reveal factor: returns 0.0 → 1.0 over `reveal_duration`
/// seconds starting at `cue.start_sec + offset_frac * cue_duration`.
pub fn line_reveal_factor(
    line: &CaptionLine,
    display_line_idx: usize,
    time: f64,
    style: &CaptionStyle,
) -> f32 {
    let total_dur = (line.end_sec - line.start_sec).max(0.001);
    let offset_frac = style
        .line_reveal_offsets
        .get(display_line_idx)
        .copied()
        .unwrap_or(0.0);
    let duration = style
        .line_reveal_durations
        .get(display_line_idx)
        .copied()
        .unwrap_or(0.10);
    let reveal_start = line.start_sec + total_dur * (offset_frac as f64);
    let dt = (time - reveal_start).max(0.0);
    let raw = if duration <= 0.0 { 1.0 } else { dt / (duration as f64) };
    CaptionNode::ease_out_sine_inline(raw as f32).clamp(0.0, 1.0)
}

impl CaptionNode {
    pub(crate) fn ease_out_sine_inline(t: f32) -> f32 {
        let x = t.clamp(0.0, 1.0);
        1.0 - (x * std::f32::consts::FRAC_PI_2).cos()
    }
}

// ── Platform presets, phrase break, resolution scaling ────────────

/// Reference canvas height for 9:16 short-form safe-area scaling (1080×1920).
pub const REFERENCE_SHORT_FORM_HEIGHT: f32 = 1920.0;

/// Scale a named platform safe zone to the output canvas height.
pub fn scaled_safe_zone(preset: &str, canvas_h: f32) -> Option<crate::types::SafeZone> {
    let ds = DesignSystem::new();
    let zone = ds.safe_areas.get(preset)?;
    let scale = canvas_h / REFERENCE_SHORT_FORM_HEIGHT;
    Some(crate::types::SafeZone {
        top: zone.top * scale,
        bottom: zone.bottom * scale,
        left: zone.left * scale,
        right: zone.right * scale,
        aspect_ratio: zone.aspect_ratio.clone(),
    })
}

/// Split a single-line cue into stacked display lines (max N words per line).
pub fn apply_phrase_break(line: &mut CaptionLine, max_words_per_line: usize) {
    if max_words_per_line == 0 || line.words.is_empty() || line.text.contains('\n') {
        return;
    }
    let mut line_idx = 0usize;
    let mut count = 0usize;
    for w in line.words.iter_mut() {
        w.display_line_idx = line_idx;
        count += 1;
        if count >= max_words_per_line {
            line_idx += 1;
            count = 0;
        }
    }
}

/// After phrase-break, re-time each display-line group into its own cue sub-window
/// so word highlights align with stacked lines (fixes global linear auto-time drift).
pub fn retime_words_by_display_line(line: &mut CaptionLine, style: &CaptionStyle) {
    if line.words.is_empty() {
        return;
    }
    let duration = (line.end_sec - line.start_sec).max(0.001);
    let max_dl = line
        .words
        .iter()
        .map(|w| w.display_line_idx)
        .max()
        .unwrap_or(0);

    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (i, w) in line.words.iter().enumerate() {
        groups.entry(w.display_line_idx).or_default().push(i);
    }

    for dl_idx in 0..=max_dl {
        let Some(indices) = groups.get(&dl_idx) else {
            continue;
        };
        let offset_start = style
            .line_reveal_offsets
            .get(dl_idx)
            .copied()
            .unwrap_or(0.0);
        let offset_end = style
            .line_reveal_offsets
            .get(dl_idx + 1)
            .copied()
            .unwrap_or(1.0);
        let window_start = line.start_sec + duration * offset_start as f64;
        let window_end = line.start_sec + duration * offset_end as f64;
        let window = (window_end - window_start).max(0.001);
        let word_dur = window / indices.len() as f64;
        for (i, &word_idx) in indices.iter().enumerate() {
            line.words[word_idx].start_sec = window_start + i as f64 * word_dur;
            line.words[word_idx].end_sec = window_start + (i + 1) as f64 * word_dur;
        }
    }
}

/// Re-run phrase break for all cues (after style props change).
pub fn refresh_caption_lines(lines: &mut [CaptionLine], style: &CaptionStyle) {
    for line in lines.iter_mut() {
        if style.auto_phrase_break && !line.text.contains('\n') {
            apply_phrase_break(line, style.max_words_per_line);
            retime_words_by_display_line(line, style);
        }
    }
}

/// Format token text (optional uppercase).
pub fn format_caption_word(text: &str, uppercase: bool) -> String {
    if uppercase {
        text.to_uppercase()
    } else {
        text.to_string()
    }
}

/// Contrast-based legibility score in `[0, 1]` (higher = more readable).
pub fn estimate_legibility(text_luma: f32, bg_luma: f32) -> f32 {
    let contrast = (text_luma - bg_luma).abs();
    (contrast * 2.2).clamp(0.0, 1.0)
}

impl CaptionStyle {
    /// Resolve dimensional style for a concrete canvas height (safe area + scale).
    pub fn resolved_for_canvas(&self, canvas_h: f32) -> CaptionStyle {
        let mut s = self.clone();
        let dim_scale = if s.scale_with_canvas {
            (canvas_h / s.reference_canvas_height).clamp(0.45, 2.5)
        } else {
            1.0
        };

        s.font_size *= dim_scale;
        s.line_spacing *= dim_scale;
        s.stroke_width *= dim_scale;
        s.text_shadow_offset_x *= dim_scale;
        s.text_shadow_offset_y *= dim_scale;
        s.text_shadow_blur *= dim_scale;
        s.glow_blur *= dim_scale;
        s.pill_padding_h *= dim_scale;
        s.pill_padding_v *= dim_scale;
        s.pill_radius *= dim_scale;

        if let Some(ref preset) = s.safe_area_preset {
            if let Some(zone) = scaled_safe_zone(preset, canvas_h) {
                s.bottom_margin = zone.bottom;
                s.top_margin = zone.top;
            }
        } else {
            s.bottom_margin *= dim_scale;
            s.top_margin *= dim_scale;
        }
        s
    }

    /// Pill fill color, optionally inverted from sampled background luminance.
    pub fn resolve_pill_color(&self, bg_luma: Option<f32>) -> Color {
        if self.pill_adaptive {
            if let Some(lum) = bg_luma {
                if lum > 0.55 {
                    return Color::new(0.0, 0.0, 0.0, 0.68);
                }
                if lum < 0.40 {
                    return Color::new(1.0, 1.0, 1.0, 0.82);
                }
            }
        }
        self.pill_color
    }
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
    use crate::{AssetLoader, DefaultAssetLoader};
    use skia_safe::textlayout::TypefaceFontProvider;
    use skia_safe::{Data, FontMgr};

    fn test_font_collection() -> Arc<Mutex<FontCollection>> {
        let loader = DefaultAssetLoader;
        let mut font_collection = FontCollection::new();
        let mut font_provider = TypefaceFontProvider::new();
        for (file, aliases) in [
            ("fonts/Roboto-Regular.ttf", &["Roboto", "Sans Serif"][..]),
            ("fonts/Roboto-Bold.ttf", &["Roboto"][..]),
            ("fonts/Poppins-Bold.ttf", &["Poppins"][..]),
        ] {
            if let Ok(bytes) = loader.load_bytes(file) {
                let data = Data::new_copy(&bytes);
                if let Some(tf) = FontMgr::new().new_from_data(&data, 0) {
                    for alias in aliases {
                        font_provider.register_typeface(tf.clone(), Some(*alias));
                    }
                }
            }
        }
        font_collection.set_asset_font_manager(Some(font_provider.into()));
        font_collection.set_default_font_manager(FontMgr::default(), None);
        Arc::new(Mutex::new(font_collection))
    }

    #[test]
    fn test_visible_cue_uses_original_line_index() {
        let fc = test_font_collection();
        let lines = vec![
            CaptionLine {
                text: "first".into(),
                words: vec![CaptionWord {
                    text: "first".into(),
                    start_sec: 0.0,
                    end_sec: 2.0,
                    display_line_idx: 0,
                }],
                start_sec: 0.0,
                end_sec: 2.0,
            },
            CaptionLine {
                text: "second".into(),
                words: vec![CaptionWord {
                    text: "second".into(),
                    start_sec: 2.5,
                    end_sec: 5.0,
                    display_line_idx: 0,
                }],
                start_sec: 2.5,
                end_sec: 5.0,
            },
        ];
        let node = CaptionNode::new(lines, fc);
        let layout = node.build_block_layout(400.0, 600.0, Color::BLACK);
        assert_eq!(layout.cues.len(), 2);

        let visible: Vec<(usize, &CaptionLine)> = node
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| 3.0 >= l.start_sec && 3.0 < l.end_sec)
            .collect();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].0, 1);
        let cue_layout = &layout.cues[visible[0].0];
        assert_eq!(cue_layout.display_lines[0].tokens[0].text, "second");
    }

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
        let visible: Vec<(usize, &CaptionLine)> = vec![(0, &line_a), (1, &line_b)];
        let a = CaptionNode::find_global_active(&visible, 2.5);
        assert_eq!(a, Some((0, 1)));
    }

    #[test]
    fn test_word_font_size_uniform() {
        let fc = Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()));
        let node = CaptionNode::new(Vec::new(), fc);
        assert!(node.style.font_size > 0.0);
    }

    #[test]
    fn test_ease_out_back_no_wobble_in_style() {
        let fc = Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()));
        let node = CaptionNode::new(Vec::new(), fc);
        assert!(node.style.active_pop_scale >= 1.0 && node.style.active_pop_scale <= 1.10);
    }

    #[test]
    fn test_no_dual_yellow_at_word_boundary() {
        let fc = Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()));
        let node = CaptionNode::new(Vec::new(), fc);

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
        let visible: Vec<(usize, &CaptionLine)> = vec![(0, &line)];

        let active = CaptionNode::find_global_active(&visible, 2.0);
        assert_eq!(active, Some((0, 1)));

        let color_at_boundary = node.compute_word_color(&word_a, 2.0, false);
        assert!(
            (color_at_boundary.r - node.style.spoken_color.r).abs() < 0.01
                && (color_at_boundary.g - node.style.spoken_color.g).abs() < 0.01
                && (color_at_boundary.b - node.style.spoken_color.b).abs() < 0.01
        );
        let color_b_at_boundary = node.compute_word_color(&word_b, 2.0, true);
        let highlight = node.style.highlight_color;
        assert!(
            (color_b_at_boundary.r - highlight.r).abs() < 0.01
                && (color_b_at_boundary.g - highlight.g).abs() < 0.01
                && (color_b_at_boundary.b - highlight.b).abs() < 0.01
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
        let r0_late = CaptionNode::ease_out_sine_inline(1.5);
        assert!(r0_late > 0.99);
        let r1_before = line_reveal_factor(&line, 1, 1.0, &node.style);
        let r1_after = line_reveal_factor(&line, 1, 3.0, &node.style);
        assert_eq!(r1_before, 0.0);
        assert_eq!(r1_after, 1.0);
    }

    #[test]
    fn test_pop_overlay_only_during_window() {
        let fc = Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()));
        let node = CaptionNode::new(Vec::new(), fc);
        let scale_at_start = node.active_pop_scale_at(2.0, 2.0);
        let scale_at_mid = node.active_pop_scale_at(2.0, 2.04);
        let scale_after = node.active_pop_scale_at(2.0, 2.5);
        assert!(scale_at_start >= 1.04 - 0.01);
        assert!(scale_at_mid > 1.0 && scale_at_mid < scale_at_start + 0.05);
        assert_eq!(scale_after, 1.0);
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

    #[test]
    fn test_two_line_cue_has_two_display_lines() {
        let fc = Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()));
        let content = "1\n00:00:00,000 --> 00:00:03,000\nhigh quality\ncaptions\n";
        let mut lines = parse_srt(content);
        // Override font_size to match the proof4 example.
        let node = CaptionNode::new(lines.clone(), Arc::clone(&fc));
        // Build block layout directly:
        let shadow = Color::new(0.0, 0.0, 0.0, 0.85);
        let layout = node.build_block_layout(688.0, 600.0, shadow);
        assert_eq!(layout.cues.len(), 1);
        let cue = &layout.cues[0];
        assert_eq!(
            cue.display_lines.len(),
            2,
            "two-line cue should produce 2 display_lines, got {}",
            cue.display_lines.len()
        );
        assert_eq!(cue.display_lines[0].tokens.len(), 2); // high, quality
        assert_eq!(cue.display_lines[1].tokens.len(), 1); // captions
        let _ = &mut lines;
    }

    #[test]
    fn test_caption_rtl_word_order() {
        let fc = test_font_collection();
        let mut line = CaptionLine {
            text: "مرحبا بالعالم".into(),
            words: vec![],
            start_sec: 0.0,
            end_sec: 4.0,
        };
        auto_time_words(&mut line);
        let node = CaptionNode::new(vec![line], fc);
        let shadow = Color::new(0.0, 0.0, 0.0, 0.85);
        let layout = node.build_block_layout(400.0, 600.0, shadow);
        let tokens = &layout.cues[0].display_lines[0].tokens;
        assert!(tokens.len() >= 2);
        assert!(
            tokens[0].x > tokens[tokens.len() - 1].x,
            "RTL caption: first word x ({:.1}) should be right of last ({:.1})",
            tokens[0].x,
            tokens[tokens.len() - 1].x
        );
    }

    #[test]
    fn test_phrase_break_retimes_per_display_line() {
        let mut line = CaptionLine {
            text: "one two three four five six seven eight".into(),
            words: vec![],
            start_sec: 0.0,
            end_sec: 4.0,
        };
        auto_time_words(&mut line);
        let style = CaptionStyle::default();
        apply_phrase_break(&mut line, 4);
        retime_words_by_display_line(&mut line, &style);
        // Line 1 words (indices 4-7) must start no earlier than line 1 reveal (45% = 1.8s).
        assert!(
            line.words[4].start_sec >= 1.8 - 0.01,
            "line 1 first word should start at reveal offset, got {:.2}",
            line.words[4].start_sec
        );
        assert!(
            line.words[3].end_sec <= 1.8 + 0.01,
            "line 0 last word should end before line 1 window"
        );
    }

    #[test]
    fn test_phrase_auto_break_splits_long_cue() {
        let mut line = CaptionLine {
            text: "one two three four five six seven".into(),
            words: vec![],
            start_sec: 0.0,
            end_sec: 4.0,
        };
        auto_time_words(&mut line);
        apply_phrase_break(&mut line, 4);
        let max_line = line.words.iter().map(|w| w.display_line_idx).max().unwrap();
        assert_eq!(max_line, 1, "7 words @ 4/line → 2 display lines");
        assert_eq!(line.words[3].display_line_idx, 0);
        assert_eq!(line.words[4].display_line_idx, 1);
    }

    #[test]
    fn test_safe_area_preset_scales_bottom_margin() {
        let mut style = CaptionStyle::default();
        style.safe_area_preset = Some("tiktok".to_string());
        style.scale_with_canvas = false;
        let at_1920 = style.resolved_for_canvas(1920.0);
        let at_960 = style.resolved_for_canvas(960.0);
        assert!(at_1920.bottom_margin > 100.0);
        assert!(
            (at_960.bottom_margin - at_1920.bottom_margin * 0.5).abs() < 2.0,
            "tiktok bottom margin should scale with canvas height"
        );
    }

    #[test]
    fn test_pill_enabled_by_default() {
        let style = CaptionStyle::default();
        assert!(style.pill_enabled);
        assert!(style.auto_phrase_break);
        assert_eq!(style.max_words_per_line, 4);
    }

    #[test]
    fn test_legibility_score_high_contrast() {
        assert!(estimate_legibility(0.95, 0.1) > 0.8);
        assert!(estimate_legibility(0.2, 0.8) > 0.8);
    }

    #[test]
    fn test_measure_height_matches_block_layout() {
        let fc = test_font_collection();
        let long_text = "The quick brown fox jumps over the lazy dog repeatedly";
        let mut line = CaptionLine {
            text: long_text.into(),
            words: vec![],
            start_sec: 0.0,
            end_sec: 5.0,
        };
        auto_time_words(&mut line);
        let node = CaptionNode::new(vec![line], fc);
        let canvas_width = 400.0f32;
        let canvas_h = 600.0f32;
        let layout_style = node.style.resolved_for_canvas(canvas_h);
        let block_width = canvas_width * layout_style.layout_width_fraction;

        let measured = node.measure(
            Size {
                width: Some(canvas_width),
                height: None,
            },
            Size {
                width: AvailableSpace::Definite(canvas_width),
                height: AvailableSpace::Definite(canvas_h),
            },
        );

        let layout = node.build_block_layout(block_width, canvas_h, node.style.text_shadow_color);
        let expected_h = layout.total_height + layout_style.bottom_margin;
        assert!(
            (measured.height - expected_h).abs() < 0.5,
            "measure height {:.1} should match block layout {:.1}",
            measured.height,
            expected_h
        );
        assert!(
            layout.total_height > layout_style.font_size * 1.5,
            "long text at {:.0}px block width should wrap to multiple lines",
            block_width
        );
    }
}
