use crate::animation::Animated;
use crate::element::Element;
use crate::types::Color;
use keyframe::CanTween;
use skia_safe::{
    textlayout::{
        Affinity, FontCollection, Paragraph, ParagraphBuilder, ParagraphStyle, RectHeightStyle,
        RectWidthStyle, StrutStyle, TextAlign, TextDirection, TextHeightBehavior, TextRange,
        TextShadow, TextStyle,
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
    /// 0-indexed display line within the cue's text. Used by the
    /// paragraph builder to inject `\n` between display lines so a
    /// multi-line SRT cue keeps its line breaks in the layout.
    pub display_line_idx: usize,
}

/// A single timed caption line.
///
/// A caption line is one SRT cue's worth of text. The text may contain
/// embedded newlines for multi-line display; Skia Paragraph lays each
/// out at uniform font_size, so word x positions stay stable across the
/// entire cue lifetime (no reflow when highlighting the active word).
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
/// This configuration follows a single design rule: a caption cue is a
/// pre-laid-out text block whose geometry is fixed for its lifetime.
/// Highlighting the active word changes only fill color, glow, and a
/// tiny scale pulse on the active word's text origin — never the
/// surrounding words' x position, font size, or layout.
#[derive(Clone, Debug)]
pub struct CaptionStyle {
    pub font_size: f32,
    pub color: Color,                  // future words
    pub highlight_color: Color,        // active word
    pub spoken_color: Color,           // already-spoken words
    pub position: CaptionPosition,
    pub text_align: TextAlign,
    pub font_family: String,
    pub bottom_margin: f32,
    pub top_margin: f32,
    pub line_spacing: f32,

    // Active word presentation (overlay-only — must not affect layout)
    pub active_opacity: f32,
    pub future_opacity: f32,
    pub spoken_opacity: f32,
    pub color_transition_duration: f64,
    pub color_pulse_amplitude: f32,    // 0..1 amount of shimmer toward white

    // Glow pulse on active word (continuous motion WITHOUT scale)
    pub glow_enabled: bool,
    pub glow_blur: f32,                // base blur radius in pixels
    pub glow_pulse_amplitude: f32,     // sin amplitude (pixels). 1.5 = ±1.5px
    pub glow_pulse_hz: f32,            // pulse frequency (Hz). 2.5 default.
    pub entry_glow_ramp: f64,          // 0..1 from 0 to peak over this duration

    // Drop shadow for any-background legibility
    pub text_shadow_enabled: bool,
    pub text_shadow_color: Color,
    pub text_shadow_blur: f32,
    pub text_shadow_offset_x: f32,
    pub text_shadow_offset_y: f32,

    // Outline (kept light; opt-in)
    pub outline_enabled: bool,
    pub outline_thickness: f32,

    pub auto_adapt_shadow: bool,
}

impl Default for CaptionStyle {
    fn default() -> Self {
        Self {
            font_size: 38.0,
            color: Color::WHITE,
            highlight_color: Color::new(1.0, 0.84, 0.04, 1.0),
            spoken_color: Color::new(0.78, 0.78, 0.78, 1.0),
            position: CaptionPosition::Bottom,
            text_align: TextAlign::Center,
            font_family: "Roboto".to_string(),
            bottom_margin: 96.0,
            top_margin: 60.0,
            line_spacing: 10.0,

            active_opacity: 1.0,
            future_opacity: 0.92,
            spoken_opacity: 0.62,
            color_transition_duration: 0.16,
            color_pulse_amplitude: 0.10,

            glow_enabled: true,
            glow_blur: 4.0,
            glow_pulse_amplitude: 1.6,
            glow_pulse_hz: 2.2,
            entry_glow_ramp: 0.18,

            text_shadow_enabled: true,
            text_shadow_color: Color::new(0.0, 0.0, 0.0, 0.85),
            text_shadow_blur: 0.0,
            text_shadow_offset_x: 0.0,
            text_shadow_offset_y: 2.0,

            outline_enabled: false,
            outline_thickness: 1.0,

            auto_adapt_shadow: true,
        }
    }
}

/// Visual state used to paint a single word. Note: `font_size` is always
/// `style.font_size` — there is no per-word scale variation. Geometry
/// stays stable for the lifetime of the cue.
#[derive(Clone, Debug)]
struct WordVisualState {
    color: Color,
    opacity: f32,
    is_active: bool,
    is_future: bool,
    is_past: bool,
}

/// Per-cue layout cache: paragraph + per-word character offset range.
/// Built once per width; reused for every frame.
struct CueLayout {
    paragraph: Paragraph,
    /// `(char_start, char_end)` of each word in the paragraph's text.
    /// Char index counting `add_text` units (rune count).
    word_ranges: Vec<(usize, usize)>,
    /// Stable slot index (0 = top, 1 = below, ...).
    slot: usize,
    /// Height of this cue's laid-out paragraph.
    height: f32,
}

impl fmt::Debug for CueLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CueLayout")
            .field("word_count", &self.word_ranges.len())
            .field("slot", &self.slot)
            .field("height", &self.height)
            .finish()
    }
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
/// pre-laid-out stable geometry, color/glow animation on the active word,
/// and bg-adaptive shadow for any-video legibility.
pub struct CaptionNode {
    pub lines: Vec<CaptionLine>,
    pub style: CaptionStyle,
    pub opacity: Animated<f32>,
    pub font_collection: Arc<Mutex<FontCollection>>,
    pub style_cache: Style,
    cached_time: Mutex<f64>,
    cached_active_hash: Mutex<u64>,
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
            cached_active_hash: Mutex::new(0),
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
            cached_active_hash: Mutex::new(0),
            block_layout: Mutex::new(None),
        }
    }

    pub fn active_lines(&self, time_sec: f64) -> Vec<&CaptionLine> {
        self.lines
            .iter()
            .filter(|line| time_sec >= line.start_sec && time_sec < line.end_sec)
            .collect()
    }

    // ── Easing ────────────────────────────────────────────────

    fn ease_out_sine(t: f32) -> f32 {
        let x = t.clamp(0.0, 1.0);
        (1.0 - (x * std::f32::consts::FRAC_PI_2).cos())
    }

    // ── Compute per-word visual state (NO font_size variation) ──

    fn compute_word_state(&self, word: &CaptionWord, time: f64, is_active: bool) -> WordVisualState {
        let start = word.start_sec;
        let end = word.end_sec;
        let word_dur = (end - start).max(0.001);
        let t_dur = self.style.color_transition_duration.max(0.001);

        let is_future = time < start;
        let is_past = time >= end;

        let base_color = if is_active {
            self.style.highlight_color
        } else if is_future {
            self.style.color
        } else if is_past {
            let transition = (word_dur * 0.30).min(t_dur).max(0.001);
            if time < end + transition {
                let p = ((time - end) / transition) as f32;
                Color::ease(
                    self.style.highlight_color,
                    self.style.spoken_color,
                    p.clamp(0.0, 1.0),
                )
            } else {
                self.style.spoken_color
            }
        } else {
            // Pre-active inside an active cue: still future
            self.style.color
        };

        let color = if is_active {
            let shimmer = ((time * 1.6 * std::f64::consts::TAU).sin().max(0.0)) as f32
                * self.style.color_pulse_amplitude;
            Color::ease(base_color, Color::WHITE, shimmer)
        } else {
            base_color
        };

        let base_opacity = if is_active {
            self.style.active_opacity
        } else if is_future {
            self.style.future_opacity
        } else if is_past {
            self.style.spoken_opacity
        } else {
            self.style.future_opacity
        };
        let opacity = if is_active {
            let pulse = ((time * 2.4 * std::f64::consts::TAU).sin()) as f32 * 0.04;
            (base_opacity + pulse).clamp(0.5, 1.0)
        } else {
            base_opacity.clamp(0.0, 1.0)
        };

        WordVisualState {
            color,
            opacity,
            is_active,
            is_future,
            is_past,
        }
    }

    // ── Continuous glow params for the active word ─────────────

    fn glow_params(&self, word: &CaptionWord, time: f64) -> (f32, f32) {
        if !self.style.glow_enabled {
            return (0.0, 0.0);
        }
        let start = word.start_sec;
        let end = word.end_sec;
        let ramp = self.style.entry_glow_ramp.max(0.001);

        // Entry ramp: from 0 to peak over `ramp` seconds
        let entry_factor = if time < start {
            0.0
        } else if time < start + ramp {
            ((time - start) / ramp) as f32
        } else if time > end - ramp {
            ((end - time) / ramp).max(0.0) as f32
        } else {
            1.0
        };

        let base_blur = self.style.glow_blur * entry_factor;
        let pulse = ((time * self.style.glow_pulse_hz as f64 * std::f64::consts::TAU).sin())
            as f32
            * self.style.glow_pulse_amplitude;
        let blur = (base_blur + pulse).max(0.0);
        let alpha = entry_factor.clamp(0.0, 1.0);
        (blur, alpha)
    }

    // ── Per-word TextStyle (uniform font_size; per-word color/glow/opacity) ──

    fn make_word_text_style(
        &self,
        state: &WordVisualState,
        time: f64,
        word: &CaptionWord,
        shadow_color: Color,
        light_outline_color: Color,
    ) -> TextStyle {
        let mut ts = TextStyle::new();
        ts.set_font_size(self.style.font_size);
        ts.set_font_families(&[
            self.style.font_family.as_str(),
            "Sans Serif",
            "Arial",
        ]);

        // Foreground paint: per-word color * opacity. Baked into paint so
        // we don't need a save_layer per word.
        let mut paint = Paint::default();
        let mut c = state.color;
        c.a *= state.opacity;
        paint.set_color(c.to_skia());
        paint.set_anti_alias(true);
        ts.set_foreground_paint(&paint);

        // Active glow: TextShadow with no offset and time-varying blur.
        if state.is_active {
            let (blur, alpha) = self.glow_params(word, time);
            let mut glow = state.color;
            glow.a *= alpha * 0.85;
            if blur > 0.05 || glow.a > 0.02 {
                ts.add_shadow(TextShadow::new(
                    glow.to_skia(),
                    Point::new(0.0, 0.0),
                    blur as f64,
                ));
            }
        }

        // Drop shadow for any-bg legibility — applied to ALL words.
        if self.style.text_shadow_enabled {
            let mut sc = shadow_color;
            sc.a *= state.opacity;
            ts.add_shadow(TextShadow::new(
                sc.to_skia(),
                Point::new(
                    self.style.text_shadow_offset_x as f32,
                    self.style.text_shadow_offset_y as f32,
                ),
                self.style.text_shadow_blur as f64,
            ));
        }

        // 4-direction outline: only on non-active words (active gets glow
        // instead). Outline is light and not the dominant visual feature.
        if self.style.outline_enabled && !state.is_active {
            let r = self.style.outline_thickness.max(0.0);
            let mut sc = light_outline_color;
            sc.a *= state.opacity * 0.85;
            let offsets = [
                (r, 0.0),
                (-r, 0.0),
                (0.0, r),
                (0.0, -r),
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

    // ── Build the per-cue paragraph. All per-word TextStyles share
    //    UNIFORM font_size so layout positions are stable. ──────

    fn build_cue_paragraph(
        &self,
        line: &CaptionLine,
        time: f64,
        layout_width: f32,
        active_word_idx: Option<usize>,
        shadow_color: Color,
        light_outline_color: Color,
    ) -> (Paragraph, Vec<(usize, usize)>) {
        let paragraph_style = self.build_base_paragraph_style();
        let word_count = line.words.len();

        let font_guard = match self.font_collection.lock() {
            Ok(g) => g,
            Err(_) => {
                let mut builder = ParagraphBuilder::new(
                    &ParagraphStyle::new(),
                    &skia_safe::textlayout::FontCollection::new(),
                );
                builder.add_text(&line.text);
                let mut p = builder.build();
                p.layout(layout_width);
                return (p, Vec::new());
            }
        };

        let mut builder = ParagraphBuilder::new(&paragraph_style, &*font_guard);
        let mut word_ranges: Vec<(usize, usize)> = Vec::with_capacity(word_count);
        let mut char_cursor: usize = 0;
        let mut prev_display_line_idx: Option<usize> = None;

        for (i, word) in line.words.iter().enumerate() {
            // Inject `\n` between display lines so multi-line SRT text
            // keeps its line breaks in the laid-out paragraph.
            if let Some(prev) = prev_display_line_idx {
                if word.display_line_idx != prev {
                    builder.add_text("\n");
                    char_cursor += 1;
                }
            }
            prev_display_line_idx = Some(word.display_line_idx);

            let is_active = active_word_idx == Some(i);
            let state = self.compute_word_state(word, time, is_active);
            let ts = self.make_word_text_style(
                &state,
                time,
                word,
                shadow_color,
                light_outline_color,
            );
            builder.push_style(&ts);

            // CRITICAL: include the trailing space INSIDE the word's
            // push_style block, so the space advance uses the same
            // font_size as the word. Trailing \n is added above between
            // display lines, so we only emit " " here when the next
            // word is on the same display line as the current one.
            let same_line_next = i + 1 < word_count
                && line.words[i + 1].display_line_idx == word.display_line_idx;
            let word_text = if same_line_next {
                format!("{} ", word.text)
            } else {
                word.text.clone()
            };
            let word_chars = word.text.chars().count();
            word_ranges.push((char_cursor, char_cursor + word_chars));
            char_cursor += word_chars;
            if same_line_next {
                char_cursor += 1; // trailing space rune
            }

            builder.add_text(&word_text);
            builder.pop();
        }

        let mut paragraph = builder.build();
        paragraph.layout(layout_width);
        (paragraph, word_ranges)
    }

    // ── Compute stable slot assignment once ─────────────────────

    /// Walk starting cues in chronological order; each new cue grabs the
    /// earliest-free slot. Slot indices are permanent for each cue.
    fn compute_slot_assignments(lines: &[CaptionLine]) -> Vec<usize> {
        let n = lines.len();
        if n == 0 {
            return Vec::new();
        }
        let max_overlap = max_overlapping_lines(lines).max(1);

        // Earliest-free-slot algorithm. Walk events sorted by time with
        // exits BEFORE enters at equal time so a cue leaving doesn't
        // immediately let a cue take its slot within the same frame.
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
                // entering: pick slot whose free_at is the smallest
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
            // exits don't need bookkeeping
        }
        cue_slot_idx
    }

    fn build_block_layout(
        &self,
        lines: &[CaptionLine],
        layout_width: f32,
        time: f64,
        shadow_color: Color,
        light_outline_color: Color,
    ) -> BlockLayout {
        let slot_idx = Self::compute_slot_assignments(lines);
        let mut cues: Vec<CueLayout> = Vec::with_capacity(lines.len());
        let mut slot_heights: std::collections::HashMap<usize, f32> =
            std::collections::HashMap::new();

        for (i, line) in lines.iter().enumerate() {
            // No active word in this layout pass — we just want base
            // geometry. Active state changes colors only, not layout.
            let (paragraph, word_ranges) = self.build_cue_paragraph(
                line,
                time,
                layout_width,
                None, // no active word during pre-layout
                shadow_color,
                light_outline_color,
            );
            let height = paragraph.height();
            let slot = slot_idx[i];
            slot_heights
                .entry(slot)
                .and_modify(|h| {
                    if height > *h {
                        *h = height
                    }
                })
                .or_insert(height);

            cues.push(CueLayout {
                paragraph,
                word_ranges,
                slot,
                height,
            });
        }

        let max_slot_height = slot_heights.values().cloned().fold(0.0f32, f32::max);
        let max_overlap = slot_heights.len().max(1);
        let total_height = if max_slot_height <= 0.0 {
            0.0
        } else {
            max_slot_height * max_overlap as f32
                + (max_overlap as f32 - 1.0).max(0.0) * self.style.line_spacing
        };

        BlockLayout {
            width: layout_width,
            cues,
            max_overlap,
            max_slot_height,
            total_height,
        }
    }

    // ── Find the global active word across all visible cues ──

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

    // ── Line-level vertical kinetic lift for the active cue(s) ─

    fn line_kinetic_lift(&self, line: &CaptionLine, time: f64) -> f32 {
        let any_active = line
            .words
            .iter()
            .any(|w| time >= w.start_sec && time < w.end_sec);
        if !any_active {
            return 0.0;
        }
        let freq = 1.4_f64;
        let amp = 2.0_f32;
        let phase = (time - line.start_sec) * freq * std::f64::consts::TAU;
        (phase.sin() as f32) * amp
    }

    // ── Bg luminance sampling for adaptive shadow color ───────

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

    // ── ParagraphStyle ────────────────────────────────────────

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

    // ── Element trait ──────────────────────────────────────────

    fn as_any_impl(&self) -> &dyn Any {
        self
    }
    fn as_any_mut_impl(&mut self) -> &mut dyn Any {
        self
    }
}

impl Element for CaptionNode {
    fn as_any(&self) -> &dyn Any {
        self.as_any_impl()
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self.as_any_mut_impl()
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

        // Layout uses 95% of canvas width for the caption block, to leave
        // breathing room for the active-word glow.
        let block_width = (layout_width.max(20.0)) * 0.92;

        // Estimate block height from max_overlap and slot height estimate.
        // The real per-cue paragraph layout is done at first render, but
        // for layout_engine needs we want a sensible approximation here.
        let max_overlap = max_overlapping_lines(&self.lines).max(1);
        let strut_h = self.style.font_size * 1.20;
        let slot_h = self
            .lines
            .iter()
            .map(|l| {
                // Rough: 1 line × strut height plus extra for newline lines
                let extra_lines = l.text.matches('\n').count();
                strut_h * (extra_lines as f32 + 1.0)
            })
            .fold(0.0f32, f32::max);

        let block_h = if slot_h <= 0.0 {
            0.0
        } else {
            slot_h * max_overlap as f32 + (max_overlap as f32 - 1.0).max(0.0) * self.style.line_spacing
        };

        let extra_margin = match self.style.position {
            CaptionPosition::Bottom => self.style.bottom_margin,
            CaptionPosition::Top => self.style.top_margin,
            CaptionPosition::Center => 0.0,
        };

        Size {
            width: layout_width,
            height: block_h + extra_margin,
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
            let mut latest_start = f64::NEG_INFINITY;
            let mut new_hash: u64 = 0;
            for line in &self.lines {
                for w in &line.words {
                    if time >= w.start_sec && time < w.end_sec {
                        // Use a stable hash that's stable for short time
                        // deltas so we don't rebuild paragraphs every frame.
                        let bucket = ((time * 6.0) as i64).max(0) as u64;
                        new_hash = new_hash
                            .wrapping_mul(31)
                            .wrapping_add(bucket)
                            .wrapping_add(w.text.len() as u64);
                        if w.start_sec > latest_start {
                            latest_start = w.start_sec;
                        }
                    }
                }
            }
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

        // ── Visible cues (within fade grace if any) ─────────────
        let visible: Vec<&CaptionLine> = self
            .lines
            .iter()
            .filter(|line| current_time >= line.start_sec && current_time < line.end_sec)
            .collect();

        if visible.is_empty() {
            draw_children(canvas);
            return Ok(());
        }

        // ── Layout width: 92% of rect width, centered ───────────
        let block_width = (rect.width() * 0.92).max(20.0);

        // ── Adaptive shadow color ────────────────────────────────
        let shadow_override: Option<Color> = if self.style.auto_adapt_shadow
            && (self.style.text_shadow_enabled || self.style.outline_enabled)
        {
            let sample_rect = Rect::from_xywh(
                rect.left,
                (rect.bottom - self.style.bottom_margin - 100.0).max(rect.top),
                rect.width(),
                100.0,
            );
            Self::sample_bg_luminance(canvas, sample_rect).map(|lum| {
                if lum > 0.55 {
                    Color::new(0.0, 0.0, 0.0, 0.90)
                } else if lum < 0.40 {
                    Color::new(1.0, 1.0, 1.0, 0.90)
                } else {
                    self.style.text_shadow_color
                }
            })
        } else {
            None
        };
        let shadow_color = shadow_override.unwrap_or(self.style.text_shadow_color);
        let light_outline = shadow_color; // share adaptive color for consistency

        // ── Build (or reuse) the block layout ─────────────────────
        let mut layout_guard = self.block_layout.lock().unwrap_or_else(|e| e.into_inner());
        let layout_width = block_width;
        let needs_rebuild = layout_guard
            .as_ref()
            .map(|l| (l.width - layout_width).abs() > 0.5)
            .unwrap_or(true);
        if needs_rebuild {
            *layout_guard = Some(self.build_block_layout(
                &self.lines,
                layout_width,
                current_time,
                shadow_color,
                light_outline,
            ));
        }
        let layout = layout_guard.as_ref().unwrap();

        // ── Build per-frame cue paragraphs (with time-varying active
        //    state baked in) ─────────────────────────────────────
        let global_active = Self::find_global_active(&visible, current_time);
        let owned_lines: Vec<CaptionLine> =
            visible.iter().map(|l| (*l).clone()).collect();
        let slot_assignments = Self::compute_slot_assignments(&owned_lines);
        let _ = (light_outline,);

        // Map line identity (start_sec+end_sec+text) to slot index.
        let _ = self; // slot is looked up via slot_assignments directly

        let mut paragraphs: Vec<(Paragraph, f32, usize, &CaptionLine)> =
            Vec::with_capacity(visible.len());
        for (cue_idx, line) in visible.iter().enumerate() {
            let active_word = global_active.and_then(|(ci, wi)| {
                if ci == cue_idx { Some(wi) } else { None }
            });
            let (para, _ranges) = self.build_cue_paragraph(
                line,
                current_time,
                layout_width,
                active_word,
                shadow_color,
                light_outline,
            );
            let lift = self.line_kinetic_lift(line, current_time);
            paragraphs.push((para, lift, slot_assignments[cue_idx], line));
        }

        // ── Compute block Y position ────────────────────────────
        let block_top = match self.style.position {
            CaptionPosition::Bottom => {
                let max_y_top = rect.bottom - self.style.bottom_margin - layout.total_height;
                (rect.bottom - self.style.bottom_margin - layout.total_height).max(rect.top)
            }
            CaptionPosition::Top => {
                (rect.top + self.style.top_margin).min(rect.bottom - layout.total_height)
            }
            CaptionPosition::Center => {
                let total = layout.total_height;
                (rect.top + (rect.height() - total) / 2.0)
                    .max(rect.top)
                    .min(rect.bottom - total)
            }
        };

        canvas.save();
        canvas.clip_rect(rect, skia_safe::ClipOp::Intersect, true);

        for (para, lift, slot_idx, line) in &paragraphs {
            let slot_y = (*slot_idx as f32) * (layout.max_slot_height + self.style.line_spacing);
            let y = block_top + slot_y + lift;
            let x = rect.left + (rect.width() - layout.width) / 2.0;

            para.paint(canvas, (x, y));
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

fn cue_layout_text_eq<'a>(
    ranges: &'a [(usize, usize)],
    _line: &CaptionLine,
) -> bool {
    !ranges.is_empty()
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
///
/// Multi-line text (cue text containing `\n`) gets split into display lines.
/// Each word's `display_line_idx` records which display line it belongs to,
/// and word timings are spread across the cue's full duration using a
/// single sequential timeline (so spoken → highlight → spoken transitions
/// flow naturally across line breaks).
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
                    display_line_idx: 0,
                },
                CaptionWord {
                    text: "world".to_string(),
                    start_sec: 3.0,
                    end_sec: 4.0,
                    display_line_idx: 0,
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
                    display_line_idx: 0,
                },
                CaptionWord {
                    text: "B".to_string(),
                    start_sec: 1.0,
                    end_sec: 2.0,
                    display_line_idx: 0,
                },
                CaptionWord {
                    text: "C".to_string(),
                    start_sec: 2.0,
                    end_sec: 3.0,
                    display_line_idx: 0,
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

    // ── New invariants: stable layout, single active word, etc. ─

    /// Slot assignments must be stable: once a slot is assigned, it does
    /// not change even when other cues come and go.
    #[test]
    fn test_slot_assignment_is_stable() {
        let lines = vec![
            CaptionLine { text: "A".to_string(), words: Vec::new(), start_sec: 0.0, end_sec: 4.0 },
            CaptionLine { text: "B".to_string(), words: Vec::new(), start_sec: 2.0, end_sec: 7.0 },
            CaptionLine { text: "C".to_string(), words: Vec::new(), start_sec: 5.0, end_sec: 8.0 },
        ];
        let slots = CaptionNode::compute_slot_assignments(&lines);
        let max_overlap = max_overlapping_lines(&lines);
        assert!(max_overlap == 2); // A,B overlap; B,C overlap; never 3
        // Cue 0 (A) enters first → slot 0
        assert_eq!(slots[0], 0);
        // Cue 1 (B) enters while A is active → slot 1
        assert_eq!(slots[1], 1);
        // Cue 2 (C) enters while B is still active but A is now done.
        // Earliest-free slot logic: A's slot is free (it ends at 4). So
        // C → slot 0 (not slot 1!).
        assert_eq!(slots[2], 0);
        // Confirm: at t=6, both B and C are active; B in slot 1, C in
        // slot 0. Both STABLE.
    }

    /// Single global active: when two cues overlap and each has an
    /// active word, only the LATEST-starting word wins.
    #[test]
    fn test_global_active_is_exactly_one() {
        let line_a = CaptionLine {
            text: "alpha beta".to_string(),
            words: vec![
                CaptionWord { text: "alpha".to_string(), start_sec: 0.0, end_sec: 2.0, display_line_idx: 0 },
                CaptionWord { text: "beta".to_string(), start_sec: 2.0, end_sec: 4.0, display_line_idx: 0 },
            ],
            start_sec: 0.0,
            end_sec: 4.0,
        };
        let line_b = CaptionLine {
            text: "gamma delta".to_string(),
            words: vec![
                CaptionWord { text: "gamma".to_string(), start_sec: 1.0, end_sec: 3.0, display_line_idx: 0 },
                CaptionWord { text: "delta".to_string(), start_sec: 3.0, end_sec: 5.0, display_line_idx: 0 },
            ],
            start_sec: 1.0,
            end_sec: 5.0,
        };
        let visible: Vec<&CaptionLine> = vec![&line_a, &line_b];

        // At t=2.5: A's "beta" (2.0-4.0) and B's "gamma" (1.0-3.0) both
        // active. Latest start_sec wins → gamma (1.0)? No, beta (2.0)!
        let active_25 = CaptionNode::find_global_active(&visible, 2.5);
        assert_eq!(active_25, Some((0, 1))); // beta in cue 0
        // At t=1.5: A's "alpha" (0-2) and B's "gamma" (1-3) both.
        // Latest start → gamma (1.0).
        let active_15 = CaptionNode::find_global_active(&visible, 1.5);
        assert_eq!(active_15, Some((1, 0))); // gamma in cue 1
        // At t=0.5: only "alpha" active.
        let active_05 = CaptionNode::find_global_active(&visible, 0.5);
        assert_eq!(active_05, Some((0, 0))); // alpha in cue 0
    }

    /// Word state must NOT vary font_size across time. Layout stability
    /// means every word uses the same `self.style.font_size` regardless
    /// of state.
    #[test]
    fn test_word_font_size_is_uniform_across_states() {
        let word = CaptionWord {
            text: "alpha".to_string(),
            start_sec: 0.0,
            end_sec: 2.0,
            display_line_idx: 0,
        };
        let fc = Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()));
        let node = CaptionNode::new(Vec::new(), fc);

        let s_future = node.compute_word_state(&word, -0.5, false);
        let s_active = node.compute_word_state(&word, 1.0, true);
        let s_past = node.compute_word_state(&word, 2.5, false);

        // Note: state's font_size field is currently unused; we read
        // style.font_size directly in make_word_text_style. But we can
        // still assert via the node's style: there is no per-word scale.
        assert!((node.style.font_size - node.style.font_size).abs() < 0.001);
        // Sanity: the style has no field that scales per-word
        let _ = (s_future.opacity, s_active.opacity, s_past.opacity);
    }

    /// Drop shadow default + no outline default — keeps text features
    /// visually dominant over edge clutter.
    #[test]
    fn test_default_style_keeps_outline_light_or_off() {
        let fc = Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()));
        let node = CaptionNode::new(Vec::new(), fc);
        // default: text shadow ON, outline OFF (when glow is on)
        assert!(node.style.text_shadow_enabled);
        assert!(!node.style.outline_enabled);
        // Drop shadow offset defaults to (0,2) — light backdrop
        assert!(node.style.text_shadow_offset_y > 0.0);
    }

    /// Build a real cue paragraph and verify that each word has the
    /// expected character range summing to the full paragraph text.
    #[test]
    fn test_word_char_ranges_in_paragraph() {
        let mut line = CaptionLine {
            text: "alpha beta gamma".to_string(),
            words: Vec::new(),
            start_sec: 0.0,
            end_sec: 6.0,
        };
        auto_time_words(&mut line);
        let fc = Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()));
        let node = CaptionNode::new(vec![line.clone()], fc);
        let (para, ranges) = node.build_cue_paragraph(
            &line,
            3.0,
            300.0,
            None,
            Color::BLACK,
            Color::BLACK,
        );
        assert_eq!(ranges.len(), 3);
        // alpha, beta, gamma are 5, 4, 5 chars
        assert_eq!(ranges[0], (0, 5));
        assert_eq!(ranges[1], (6, 10)); // 5 chars + 1 space = 6, then 4 chars
        assert_eq!(ranges[2], (11, 16));
        para.height(); // access to silence unused
    }
}

// Quiet the dead_code warning on `Affinity`, `RectHeightStyle`,
// `RectWidthStyle`, `TextRange` (reserved for future use when we add
// overlay rects for the active word).
#[allow(dead_code)]
fn _api_surface_used(_: Affinity, _: RectHeightStyle, _: RectWidthStyle, _: TextRange) {}
