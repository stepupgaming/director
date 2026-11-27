use crate::animation::Animated;
use crate::tokens::DesignSystem;
use crate::element::Element;
use serde::{Deserialize, Serialize};
use crate::text_direction::{resolve_text_direction, TextDirectionMode};
use crate::types::Color;
use skia_safe::{
    textlayout::{
        FontCollection, Paragraph, ParagraphBuilder, ParagraphStyle, RectHeightStyle,
        RectWidthStyle, StrutStyle, TextAlign, TextHeightBehavior,
        TextStyle,
    },
    font_style::{Slant, Weight, Width},
    BlurStyle, Canvas, FontStyle, MaskFilter, Paint, PaintJoin, PaintStyle, Point, Rect, RRect,
};
use std::any::Any;
use std::fmt;
use std::sync::{Arc, Mutex};
use taffy::geometry::Size;
use taffy::style::{AvailableSpace, Style};

/// A single timed word within a caption cue.
#[derive(Clone, Debug, Serialize, Deserialize)]
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CaptionLine {
    pub text: String,
    pub words: Vec<CaptionWord>,
    pub start_sec: f64,
    pub end_sec: f64,
    /// The word times are measured: speech recognition or authored per word.
    /// Phrase break keeps them, and each display line reveals by its first
    /// word. Without it the words split the cue evenly, an estimate.
    #[serde(default)]
    pub measured: bool,
}

/// One spoken word with measured times in seconds, as `director-engine asr
/// transcribe --format json` writes it. `word` is accepted for `text`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimedWord {
    #[serde(alias = "word")]
    pub text: String,
    pub start: f64,
    pub end: f64,
}

/// One page of a caption: the words on screen together and when.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CaptionPage {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// When [`cues_from_words`] starts and ends a cue. The grouping defaults
/// match `director-engine asr transcribe --format srt`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CueRules {
    /// Most characters on a cue, spaces included. A longer word keeps its own cue.
    pub max_chars: usize,
    /// Longest cue in seconds.
    pub max_duration: f64,
    /// A pause of at least this many seconds between words ends the cue.
    pub max_gap: f64,
    /// A word ending in `.`, `?` or `!` ends the cue.
    pub break_on_sentence: bool,
    /// Seconds a cue stays up after its last word, cut short by the next
    /// cue, so a pause shorter than this never blanks the screen. 0 ends
    /// each cue on its last word.
    pub hold: f64,
}

impl Default for CueRules {
    fn default() -> Self {
        Self { max_chars: 42, max_duration: 5.0, max_gap: 0.8, break_on_sentence: true, hold: 0.5 }
    }
}

/// Where to position the caption within the layout rect.
#[derive(Clone, Debug, PartialEq)]
#[derive(Default)]
pub enum CaptionPosition {
    #[default]
    Bottom,
    Top,
    Center,
    /// Place the caption block along the left edge of the frame.
    Left,
    /// Place the caption block along the right edge of the frame.
    Right,
    /// Place the caption block in the top-left corner of the frame.
    TopLeft,
    /// Place the caption block in the top-right corner of the frame.
    TopRight,
    /// Place the caption block in the bottom-left corner of the frame.
    BottomLeft,
    /// Place the caption block in the bottom-right corner of the frame.
    BottomRight,
    /// Normalized vertical offset `[0, 1]` for the center of the caption block
    /// relative to the layout rect. `0.0` places the caption at the top of
    /// the rect, `0.5` at the vertical center, and `1.0` at the bottom.
    ///
    /// This is independent of the output canvas size, so it works correctly
    /// when the caption node is placed inside a smaller container.
    Free(f32),
    /// Absolute canvas (x, y) coordinate for the center of the caption block.
    /// This is used by smart-caption nodes that solve a continuous 2D trajectory.
    ///
    /// # Note
    /// The coordinates are interpreted as absolute coordinates on the output
    /// canvas. This works correctly when the caption node fills the full
    /// canvas; for smaller layout rects the coordinate may need adjustment.
    FreeXY { x: f32, y: f32 },
}

impl CaptionPosition {
    /// Return a stable snake_case string for this position.
    pub fn as_position_str(&self) -> &'static str {
        match self {
            Self::Bottom => "bottom",
            Self::Top => "top",
            Self::Center => "center",
            Self::Left => "left",
            Self::Right => "right",
            Self::TopLeft => "top_left",
            Self::TopRight => "top_right",
            Self::BottomLeft => "bottom_left",
            Self::BottomRight => "bottom_right",
            Self::Free(_) => "free",
            Self::FreeXY { .. } => "free_xy",
        }
    }

    /// Serialize the position to a string that round-trips through
    /// `CaptionPosition::from_str`.
    pub fn to_position_string(&self) -> String {
        match self {
            Self::Free(y) => format!("free({})", y),
            Self::FreeXY { x, y } => format!("free_xy({}, {})", x, y),
            _ => self.as_position_str().to_string(),
        }
    }

    /// Parse a `CaptionPosition` from its string representation.
    ///
    /// Unknown or malformed strings fall back to `CaptionPosition::Bottom`
    /// after logging a warning. Use [`CaptionPosition::try_from_str`] if you
    /// need to surface parse errors.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        Self::try_from_str(s).unwrap_or_else(|e| {
            tracing::warn!("{}", e);
            Self::Bottom
        })
    }

    /// Parse a `CaptionPosition` from its string representation, returning
    /// an error for unknown or malformed inputs.
    pub fn try_from_str(s: &str) -> Result<Self, String> {
        match s {
            "top" => Ok(Self::Top),
            "center" => Ok(Self::Center),
            "bottom" => Ok(Self::Bottom),
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "top_left" => Ok(Self::TopLeft),
            "top_right" => Ok(Self::TopRight),
            "bottom_left" => Ok(Self::BottomLeft),
            "bottom_right" => Ok(Self::BottomRight),
            _ => {
                if let Some(rest) = s.strip_prefix("free(") {
                    if let Some(num) = rest.strip_suffix(")") {
                        if let Ok(y) = num.parse::<f32>() {
                            return Ok(Self::Free(y));
                        }
                    }
                    return Err(format!("malformed CaptionPosition::Free string: {}", s));
                }
                if let Some(rest) = s.strip_prefix("free_xy(") {
                    if let Some(inner) = rest.strip_suffix(")") {
                        let parts: Vec<&str> = inner.split(',').collect();
                        if parts.len() == 2 {
                            if let (Ok(x), Ok(y)) = (parts[0].trim().parse::<f32>(), parts[1].trim().parse::<f32>()) {
                                return Ok(Self::FreeXY { x, y });
                            }
                        }
                    }
                    return Err(format!("malformed CaptionPosition::FreeXY string: {}", s));
                }
                if s.is_empty() {
                    return Err("empty CaptionPosition string".to_string());
                }
                Err(format!("unknown CaptionPosition string: {}", s))
            }
        }
    }
}


impl serde::Serialize for CaptionPosition {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_position_string())
    }
}

impl<'de> serde::Deserialize<'de> for CaptionPosition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        CaptionPosition::try_from_str(&s).map_err(serde::de::Error::custom)
    }
}

/// How phrase-broken display lines appear within one cue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhraseLineMode {
    /// One display line at a time in the same position (Opus/TikTok default).
    Replace,
    /// All revealed display lines stack vertically (legacy).
    Stack,
}

/// How a cue splits into display lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineBreak {
    /// Lines hold as many words as fit the measured column, at most
    /// `max_words_per_line`, balanced, with no article, preposition or
    /// conjunction left at a line end.
    #[default]
    Width,
    /// Fixed chunks of `max_words_per_line` words.
    Words,
}

/// Where the caption block sits from a time on: an anchor with its margins.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptionPlacement {
    /// Seconds on the caption's timeline from which this placement holds.
    pub at: f64,
    pub position: CaptionPosition,
    pub top_margin: f32,
    pub bottom_margin: f32,
    pub left_margin: f32,
    pub right_margin: f32,
}

/// Caption styling. Defaults target a clean, TikTok/Reels look:
/// white text, warm yellow active word, restrained shadow only.
/// Uniform font_size across all token states → glyph metrics never
/// vary → no neighbor shifting when state changes.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptionStyle {
    pub font_size: f32,
    pub color: Color,            // future words (default white)
    pub highlight_color: Color,  // active word (default warm gold)
    pub spoken_color: Color,     // past words (default dim white)

    /// Where the block sits. Lines centre on a centred anchor and align to
    /// the side of a side or corner anchor.
    pub position: CaptionPosition,
    /// Horizontal margins used by `CaptionPosition::Left` and `Right`.
    pub left_margin: f32,
    pub right_margin: f32,
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

    // Optional future→active color lead-in before each word starts, in
    // seconds. 0 (the default) lands the highlight on the word's start.
    pub color_transition_duration: f64,

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

    // Outline: a round-joined stroke `stroke_width` px wide outside the glyph
    // edge, painted under the fill.
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
    pub pill_padding_h: f32,
    pub pill_padding_v: f32,
    pub pill_radius: f32,

    // Auto-split single-line cues into phrase chunks (e.g. 4 words per line).
    pub auto_phrase_break: bool,
    pub max_words_per_line: usize,
    /// Phrase-broken lines: `Replace` shows one line at a time; `Stack` keeps all.
    pub phrase_line_mode: PhraseLineMode,

    // Platform safe-area preset (`tiktok`, `youtube_shorts`, `instagram_reel`, …).
    pub safe_area_preset: Option<String>,

    /// Render all words uppercase (layout-stable; applied at shape time).
    pub uppercase: bool,

    /// How cues split into display lines.
    pub line_break: LineBreak,
    /// Display lines shown together in `Replace` mode: a page of up to this
    /// many lines replaces the previous page.
    pub max_lines: usize,
    /// Colour of the emphasis words while they are not active.
    pub emphasis_color: Option<Color>,
    /// Words painted in `emphasis_color`, matched without case or edge
    /// punctuation.
    pub emphasis_words: Vec<String>,
}

impl Default for CaptionStyle {
    fn default() -> Self {
        Self {
            font_size: 42.0,
            color: Color::new(1.0, 1.0, 1.0, 1.0),
            highlight_color: Color::new(1.0, 0.84, 0.04, 1.0),  // #FFD60A
            spoken_color: Color::new(0.78, 0.78, 0.78, 1.0),

            position: CaptionPosition::Bottom,
            left_margin: 60.0,
            right_margin: 60.0,
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

            color_transition_duration: 0.0,

            active_pop_scale: 1.04,
            active_pop_duration: 0.08,

            text_shadow_enabled: true,
            text_shadow_color: Color::new(0.0, 0.0, 0.0, 0.85),
            text_shadow_offset_x: 0.0,
            text_shadow_offset_y: 2.0,
            text_shadow_blur: 4.0,

            stroke_enabled: true,
            stroke_width: 2.0,
            stroke_color: Color::new(0.0, 0.0, 0.0, 0.92),

            glow_enabled: true,
            glow_blur: 4.0,

            line_reveal_offsets: vec![0.0, 0.45],
            line_reveal_durations: vec![0.10, 0.20],

            pill_enabled: true,
            pill_color: Color::new(0.0, 0.0, 0.0, 0.62),
            pill_padding_h: 18.0,
            pill_padding_v: 10.0,
            pill_radius: 14.0,

            auto_phrase_break: true,
            max_words_per_line: 4,
            phrase_line_mode: PhraseLineMode::Replace,

            safe_area_preset: None,

            uppercase: false,

            line_break: LineBreak::Width,
            max_lines: 1,
            emphasis_color: None,
            emphasis_words: Vec::new(),
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
    /// Width the line's paragraphs lay out at: the column, or no limit for a
    /// one-word line so a word with spaces inside never wraps.
    pub paragraph_layout_width: f32,
    /// Scale that fits a one-word line wider than the column inside it, 1
    /// when the line fits.
    pub fit: f32,
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
    pub cues: Vec<CueLayout>,
    pub max_overlap: usize,
    pub max_slot_height: f32,
    pub total_height: f32,
}

/// A node for rendering timed caption text (SRT/WebVTT subtitles).
/// Architecture: pre-laid-out structurally stable text blocks. Each
/// token has ONE canonical `(x, baselineY, width, height, centerX,
/// centerY, lineIndex)`. Active emphasis never re-measures positions;
/// it scale-transforms around the cached token center. During the
/// optional pop window the full line is painted first, then the active
/// word is painted again inside an unclipped scale overlay.
pub struct CaptionNode {
    pub lines: Vec<CaptionLine>,
    pub style: CaptionStyle,
    pub opacity: Animated<f32>,
    pub font_collection: Arc<Mutex<FontCollection>>,
    pub style_cache: Style,
    /// Placements in time order; the style's own placement holds before the
    /// first one.
    positions: Vec<CaptionPlacement>,
    /// The style's placement while `positions` moves it.
    base_placement: Option<CaptionPlacement>,
    /// Width of the caption's layout box that width line breaks measure
    /// against. Unknown until set or laid out.
    box_width: Option<f32>,
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
            positions: self.positions.clone(),
            base_placement: self.base_placement.clone(),
            box_width: self.box_width,
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
        let mut node = Self {
            lines,
            style: CaptionStyle::default(),
            opacity: Animated::new(1.0),
            font_collection,
            style_cache: Style::default(),
            positions: Vec::new(),
            base_placement: None,
            box_width: None,
            cached_time: Mutex::new(0.0),
            block_layout: Mutex::new(None),
        };
        node.refresh_lines();
        node.rebuild_layout_style();
        node
    }

    /// The placements `positions` switches between, in time order.
    pub fn positions(&self) -> &[CaptionPlacement] {
        &self.positions
    }

    /// Every page of display lines the caption shows, in time order. A page
    /// is `max_lines` display lines in replace mode and a whole cue in stack
    /// mode; it runs from its first word to the next page of its cue, the
    /// last page to the cue end. Lines break by measured width once layout
    /// has given the caption its box width.
    pub fn pages(&self) -> Vec<CaptionPage> {
        let style = self.style.resolved();
        let together = match style.phrase_line_mode {
            PhraseLineMode::Stack => usize::MAX,
            PhraseLineMode::Replace => style.max_lines.max(1),
        };
        let mut out = Vec::new();
        for line in &self.lines {
            let mut groups: Vec<(usize, Vec<&CaptionWord>)> = Vec::new();
            for word in &line.words {
                let page = word.display_line_idx / together;
                match groups.last_mut() {
                    Some((p, words)) if *p == page => words.push(word),
                    _ => groups.push((page, vec![word])),
                }
            }
            if groups.is_empty() {
                out.push(CaptionPage { start: line.start_sec, end: line.end_sec, text: line.text.clone() });
                continue;
            }
            let starts: Vec<f64> = groups.iter().map(|(_, w)| w[0].start_sec.max(line.start_sec)).collect();
            for (i, (_, words)) in groups.iter().enumerate() {
                out.push(CaptionPage {
                    start: starts[i],
                    end: starts.get(i + 1).copied().unwrap_or(line.end_sec),
                    text: words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "),
                });
            }
        }
        out.sort_by(|a, b| a.start.total_cmp(&b.start));
        out
    }

    /// Move the caption over time: each placement holds from its `at` until
    /// the next, and the style's current placement holds before the first.
    pub fn set_positions(&mut self, mut positions: Vec<CaptionPlacement>) {
        positions.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
        let base = self.base_placement.take().unwrap_or_else(|| self.placement(f64::NEG_INFINITY));
        self.apply_placement(&base);
        self.base_placement = (!positions.is_empty()).then_some(base);
        self.positions = positions;
    }

    /// The style's own placement, before any `positions` key.
    pub fn base_style(&self) -> CaptionStyle {
        let mut style = self.style.clone();
        if let Some(base) = &self.base_placement {
            style.position = base.position.clone();
            style.top_margin = base.top_margin;
            style.bottom_margin = base.bottom_margin;
            style.left_margin = base.left_margin;
            style.right_margin = base.right_margin;
        }
        style
    }

    /// The current placement of the style, holding from `at`.
    pub fn placement(&self, at: f64) -> CaptionPlacement {
        CaptionPlacement {
            at,
            position: self.style.position.clone(),
            top_margin: self.style.top_margin,
            bottom_margin: self.style.bottom_margin,
            left_margin: self.style.left_margin,
            right_margin: self.style.right_margin,
        }
    }

    fn apply_placement(&mut self, p: &CaptionPlacement) {
        let s = &self.style;
        if s.position == p.position
            && s.top_margin == p.top_margin
            && s.bottom_margin == p.bottom_margin
            && s.left_margin == p.left_margin
            && s.right_margin == p.right_margin
        {
            return;
        }
        // The parent stays, so the box keeps its parent width less the new
        // side insets.
        let (old_left, old_right) = side_insets(&self.style);
        self.style.position = p.position.clone();
        self.style.top_margin = p.top_margin;
        self.style.bottom_margin = p.bottom_margin;
        self.style.left_margin = p.left_margin;
        self.style.right_margin = p.right_margin;
        self.rebuild_layout_style();
        let (left, right) = side_insets(&self.style);
        self.set_box_width(self.box_width.map(|w| w + old_left + old_right - left - right));
        if let Ok(mut guard) = self.block_layout.lock() {
            *guard = None;
        }
    }

    /// Width of the caption's layout box, once known.
    pub fn box_width(&self) -> Option<f32> {
        self.box_width
    }

    /// The box width under the base placement: what [`Self::base_style`] lays
    /// out to in the same parent.
    pub fn base_box_width(&self) -> Option<f32> {
        let (left, right) = side_insets(&self.style);
        let (base_left, base_right) = side_insets(&self.base_style());
        self.box_width.map(|w| w + left + right - base_left - base_right)
    }

    /// Set the layout box width that width line breaks measure against. The
    /// caption lays out to its box on every frame and breaks its lines again
    /// when the width changes.
    pub fn set_box_width(&mut self, width: Option<f32>) {
        let same = match (self.box_width, width) {
            (Some(a), Some(b)) => (a - b).abs() < 0.5,
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        self.box_width = width;
        if self.style.line_break == LineBreak::Width && self.style.auto_phrase_break {
            self.refresh_lines();
            if let Ok(mut guard) = self.block_layout.lock() {
                *guard = None;
            }
        }
        self.sync_centred_height();
    }

    /// The box width a caption gets in a parent `parent_width` wide: the
    /// parent less the side margin of its anchor.
    pub fn box_width_in(&self, parent_width: f32) -> f32 {
        let (left, right) = side_insets(&self.style);
        (parent_width - left - right).max(1.0)
    }

    pub fn active_lines(&self, time_sec: f64) -> Vec<&CaptionLine> {
        self.lines
            .iter()
            .filter(|line| time_sec >= line.start_sec && time_sec < line.end_sec)
            .collect()
    }

    /// Switch the caption's vertical anchor. This is used by smart caption
    /// nodes to follow a solved trajectory without re-laying out text.
    pub fn set_position(&mut self, position: CaptionPosition) {
        if self.style.position != position {
            self.style.position = position;
            self.rebuild_layout_style();
            // Position only affects render placement and measure margin;
            // invalidate the cached block layout so measure picks up the new margin.
            if let Ok(mut guard) = self.block_layout.lock() {
                *guard = None;
            }
        }
    }

    /// Rebuild the Taffy `Style` cache from the current `CaptionPosition` so
    /// that the layout engine positions the node's rect at the actual render
    /// location (top/bottom/center/etc.) instead of always defaulting to y=0.
    pub fn rebuild_layout_style(&mut self) {
        use taffy::style::{LengthPercentageAuto, Position};

        let style = &self.style;
        let s = &mut self.style_cache;

        s.position = Position::Absolute;
        s.inset = taffy::geometry::Rect {
            left: LengthPercentageAuto::auto(),
            right: LengthPercentageAuto::auto(),
            top: LengthPercentageAuto::auto(),
            bottom: LengthPercentageAuto::auto(),
        };
        s.align_self = None;
        s.margin.top = LengthPercentageAuto::length(0.0);
        s.margin.bottom = LengthPercentageAuto::length(0.0);
        s.size.height = taffy::style::Dimension::auto();

        // The block spans its parent's width less the side margin of its
        // anchor, so lines align inside the frame without a width key.
        let len = LengthPercentageAuto::length;
        let (left, right) = side_insets(style);
        s.inset.left = len(left);
        s.inset.right = len(right);
        s.size.width = taffy::style::Dimension::auto();

        match &style.position {
            CaptionPosition::Top | CaptionPosition::TopLeft | CaptionPosition::TopRight => {
                s.inset.top = len(style.top_margin);
            }
            CaptionPosition::Bottom | CaptionPosition::BottomLeft | CaptionPosition::BottomRight => {
                s.inset.bottom = len(style.bottom_margin);
            }
            CaptionPosition::Left | CaptionPosition::Right | CaptionPosition::Center => {
                // Auto margins centre a box of the block's height between
                // zero insets, in block and flex parents alike.
                s.inset.top = len(0.0);
                s.inset.bottom = len(0.0);
                s.margin.top = LengthPercentageAuto::auto();
                s.margin.bottom = LengthPercentageAuto::auto();
            }
            CaptionPosition::Free(_) | CaptionPosition::FreeXY { .. } => {
                // Free/FreeXY positions are resolved at render time from the
                // supplied rect, so make the layout rect fill the parent. The
                // render code then computes the caption's actual offset
                // within this full-parent rect.
                s.inset = taffy::geometry::Rect {
                    left: LengthPercentageAuto::length(0.0),
                    right: LengthPercentageAuto::length(0.0),
                    top: LengthPercentageAuto::length(0.0),
                    bottom: LengthPercentageAuto::length(0.0),
                };
            }
        }
        self.sync_centred_height();
    }

    /// Centred anchors (left, right, center) hold the block's height in the
    /// layout style, so their auto margins centre the box on its parent. Without
    /// a box width yet the box fills the parent and render centres the block.
    fn sync_centred_height(&mut self) {
        let centred = matches!(self.style.position, CaptionPosition::Left | CaptionPosition::Right | CaptionPosition::Center);
        let height = self.box_width.filter(|_| centred).map(|w| {
            let layout = self.build_block_layout((w * self.style.layout_width_fraction).max(20.0));
            let height = layout.total_height;
            if let Ok(mut guard) = self.block_layout.lock() {
                *guard = Some(layout);
            }
            height
        });
        self.style_cache.size.height =
            height.map(taffy::style::Dimension::length).unwrap_or_else(taffy::style::Dimension::auto);
    }

    /// Re-apply phrase breaks and drop cached layout after style / line edits.
    pub fn after_style_changed(&mut self) {
        self.refresh_lines();
        if let Ok(mut guard) = self.block_layout.lock() {
            *guard = None;
        }
        self.sync_centred_height();
    }

    /// Break every cue into display lines by the style: measured and
    /// balanced for `LineBreak::Width`, fixed word chunks for `Words`.
    fn refresh_lines(&mut self) {
        if self.style.line_break == LineBreak::Words || !self.style.auto_phrase_break {
            refresh_caption_lines(&mut self.lines, &self.style);
            return;
        }
        let style = self.style.resolved();
        let max_width = match self.box_width {
            Some(w) => (w * style.layout_width_fraction).max(20.0) - 2.0 * style.outline(),
            None => f32::INFINITY,
        };
        let shown_together = match style.phrase_line_mode {
            PhraseLineMode::Stack => usize::MAX,
            PhraseLineMode::Replace => style.max_lines.max(1),
        };
        let mut lines = std::mem::take(&mut self.lines);
        refresh_caption_lines_by(&mut lines, &style, &mut |line: &CaptionLine| {
            let (widths, space) = self.word_widths(&style, line);
            let words: Vec<&str> = line.words.iter().map(|w| w.text.as_str()).collect();
            balanced_breaks(&widths, space, max_width, style.max_words_per_line, shown_together, &words)
        });
        self.lines = lines;
    }

    /// Each word's shaped width and the width of a space between words.
    fn word_widths(&self, style: &CaptionStyle, line: &CaptionLine) -> (Vec<f32>, f32) {
        let words: Vec<&CaptionWord> = line.words.iter().collect();
        let (paragraph, ranges) = self.build_measure_paragraph(style, &words, 1.0e6);
        let widths: Vec<f32> = ranges.iter().map(|&(a, b)| range_extent(&paragraph, a..b).width()).collect();
        let space = if widths.len() > 1 {
            ((paragraph.longest_line() - widths.iter().sum::<f32>()) / (widths.len() - 1) as f32).max(0.0)
        } else {
            0.0
        };
        (widths, space)
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
        let t_dur = self.style.color_transition_duration;

        if is_active {
            return self.style.highlight_color;
        }

        let is_future = time < start;
        let is_past = time >= end;

        if is_future {
            // Smooth future→active entry over `color_transition_duration`
            // before `start`. At `time = start - t_dur` we are 100% base
            // color; at `time = start` we are 100% highlight (entry frame).
            if t_dur > 0.0 && time > start - t_dur {
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

    #[allow(dead_code)]
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

    /// A word's text style for one coat. `visible` false paints the word
    /// transparent so the paragraph shapes exactly like every other coat.
    fn make_word_text_style(style: &CaptionStyle, color: Color, visible: bool, coat: Coat) -> TextStyle {
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

        // The outline is a real stroke centred on the glyph edge at twice the
        // visible width; the fill covers its inner half. Shadow and glow take
        // the outlined shape, so they sit under the outline.
        let outline = style.outline();
        let mut c = match coat {
            Coat::Fill => color,
            Coat::Stroke => style.stroke_color,
            Coat::Shadow => style.text_shadow_color,
            Coat::Glow => {
                let mut g = style.highlight_color;
                g.a *= 0.65;
                g
            }
        };
        if !visible {
            c.a = 0.0;
        }
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_color(c.to_skia());
        if coat != Coat::Fill && outline > 0.0 {
            paint.set_style(if coat == Coat::Stroke { PaintStyle::Stroke } else { PaintStyle::StrokeAndFill });
            paint.set_stroke_width(2.0 * outline);
            paint.set_stroke_join(PaintJoin::Round);
        }
        let blur = match coat {
            Coat::Shadow => style.text_shadow_blur,
            Coat::Glow => style.glow_blur,
            _ => 0.0,
        };
        if blur > 0.0 {
            paint.set_mask_filter(MaskFilter::blur(BlurStyle::Normal, blur, None));
        }
        ts.set_foreground_paint(&paint);
        ts
    }

    fn paint_line_pill(
        canvas: &Canvas,
        line_left_x: f32,
        line_y: f32,
        display_line: &DisplayLineLayout,
        style: &CaptionStyle,
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
        let rrect = RRect::new_rect_xy(pill_rect, style.pill_radius, style.pill_radius);
        let mut paint = Paint::default();
        let mut c = style.pill_color;
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
        for (_t, i, delta) in events {
            if delta > 0 {
                let mut best = 0usize;
                let mut best_t = slot_free_at[0];
                for (k, &free_at) in slot_free_at.iter().enumerate().skip(1) {
                    if free_at < best_t {
                        best_t = free_at;
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

    fn build_block_layout(&self, layout_width: f32) -> BlockLayout {
        let style = self.style.resolved();
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

                // The measure paragraph lays out at the same width as
                // the per-frame paragraph so token x positions match. A
                // one-word line never wraps: when it is wider than the
                // column it shrinks to fit instead.
                let one_word = words_for_line.len() == 1;
                let measure_layout_width = if layout_width > 1.0 && !one_word { layout_width } else { NO_WRAP };
                let (paragraph, token_char_ranges) =
                    self.build_measure_paragraph(&style, &words_for_line, measure_layout_width);
                let line_width = paragraph.longest_line();
                let line_height = paragraph.height();
                let room = layout_width - 2.0 * style.outline();
                let fit = if one_word && layout_width > 1.0 && line_width > room && line_width > 0.0 {
                    (room / line_width).clamp(0.05, 1.0)
                } else {
                    1.0
                };

                // Cache token layouts via get_rects_for_range (Tight).
                let mut tokens: Vec<TokenLayout> = Vec::with_capacity(words_for_line.len());
                for (i, word) in words_for_line.iter().enumerate() {
                    let (cs, ce) = token_char_ranges[i];
                    let rect = range_extent(&paragraph, cs..ce);
                    tokens.push(TokenLayout {
                        word_idx: word_indices[i],
                        text: word.text.clone(),
                        x: rect.left(),
                        baseline_y: rect.top(), // paragraph-local
                        width: rect.width(),
                        height: rect.height(),
                        center_x: rect.left() + rect.width() * 0.5,
                        center_y: rect.top() + rect.height() * 0.5,
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
                    paragraph_layout_width: measure_layout_width,
                    fit,
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
            let total_dl_height: f32 = if style.phrase_line_mode == PhraseLineMode::Replace {
                c.display_lines
                    .chunks(style.max_lines.max(1))
                    .map(|page| {
                        page.iter().map(|dl| dl.line_height).sum::<f32>()
                            + page.len().saturating_sub(1) as f32 * style.line_spacing
                    })
                    .fold(0.0f32, f32::max)
            } else {
                c.display_lines
                    .iter()
                    .map(|dl| dl.line_height)
                    .sum::<f32>()
                    + (c.display_lines.len().saturating_sub(1)) as f32 * style.line_spacing
            };
            if total_dl_height > slot_heights[c.slot] {
                slot_heights[c.slot] = total_dl_height;
            }
        }
        let max_slot_height = slot_heights.iter().cloned().fold(0.0f32, f32::max);
        // Single anchor position: reserve height for the tallest cue block, not
        // a vertical stack of every overlap slot.
        let total_height = max_slot_height;

        BlockLayout {
            width: layout_width,
            cues,
            max_overlap,
            max_slot_height,
            total_height,
        }
    }

    #[doc(hidden)]
    pub fn debug_dump_block_layout(&self, layout_width: f32) -> String {
        let layout = self.build_block_layout(layout_width);
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
                    skia_safe::textlayout::FontCollection::new(),
                );
                for w in words {
                    builder.add_text(&w.text);
                    builder.add_text(" ");
                }
                crate::counters::count(crate::counters::Counter::paragraphs_shaped);
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
            let ts = Self::make_word_text_style(style, style.color, true, Coat::Fill);
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

        crate::counters::count(crate::counters::Counter::paragraphs_shaped);
        let mut paragraph = builder.build();
        paragraph.layout(layout_width);
        (paragraph, token_char_ranges)
    }

    // ── Single global active word across all visible cues ─────────

    /// Returns `(line_index, word_index)` using indices into `CaptionNode::lines`,
    /// not indices into the visible-cue subset.
    /// Compute the left x-coordinate of a display line for the given caption
    /// position and layout rect.
    /// Left edge of a display line. Taffy insets already hold the side
    /// margins, so the side anchors align to the layout rect itself.
    fn compute_line_left_x(position: &CaptionPosition, rect: Rect, line_width: f32) -> f32 {
        match position {
            CaptionPosition::Left
            | CaptionPosition::TopLeft
            | CaptionPosition::BottomLeft => rect.left,
            CaptionPosition::Right
            | CaptionPosition::TopRight
            | CaptionPosition::BottomRight => (rect.right - line_width).max(rect.left),
            CaptionPosition::FreeXY { x, .. } => (x - line_width / 2.0).max(rect.left),
            // Top, Bottom, Center, and legacy Free(y) all center horizontally.
            _ => rect.left + (rect.width() - line_width) / 2.0,
        }
    }

    /// Compute the top y-coordinate of the caption block for the given
    /// position, layout rect, and block layout.
    ///
    /// For Top, Bottom and the corners, Taffy has already positioned the
    /// layout rect at the render location, so we just use `rect.top`. Left,
    /// Right and Center fill the parent's height and centre the block in it.
    /// For Free/FreeXY the position is resolved at render time within the
    /// supplied rect.
    fn compute_cue_block_top_y(
        layout_style: &CaptionStyle,
        rect: Rect,
        layout: &BlockLayout,
    ) -> f32 {
        match layout_style.position {
            // Anchors whose positioning is delegated to Taffy via
            // `rebuild_layout_style`.
            CaptionPosition::Top
            | CaptionPosition::Bottom
            | CaptionPosition::TopLeft
            | CaptionPosition::TopRight
            | CaptionPosition::BottomLeft
            | CaptionPosition::BottomRight => rect.top,
            CaptionPosition::Left | CaptionPosition::Right | CaptionPosition::Center => {
                rect.top + ((rect.height() - layout.total_height) / 2.0).max(0.0)
            }
            CaptionPosition::Free(y) => {
                // `y` is a normalized [0, 1] offset relative to the layout rect.
                let t = y.clamp(0.0, 1.0);
                let center_y = rect.top + t * rect.height();
                (center_y - layout.total_height / 2.0)
                    .clamp(rect.top, rect.bottom - layout.total_height)
            }
            CaptionPosition::FreeXY { y, .. } => {
                // Center the caption block at the absolute canvas y.
                (y - layout.total_height / 2.0).clamp(rect.top, rect.bottom - layout.total_height)
            }
        }
    }

    fn find_global_active(visible: &[(usize, &CaptionLine)], time: f64) -> Option<(usize, usize)> {
        let mut latest_start = f64::NEG_INFINITY;
        let mut result: Option<(usize, usize)> = None;
        for &(line_idx, line) in visible {
            for (word_idx, word) in line.words.iter().enumerate() {
                if time >= word.start_sec && time < word.end_sec
                    && word.start_sec > latest_start {
                        latest_start = word.start_sec;
                        result = Some((line_idx, word_idx));
                    }
            }
        }
        result
    }
}

// ── Style helpers ─────────────────────────────────────────────────

/// Layout width of a paragraph that must not wrap.
const NO_WRAP: f32 = 1.0e6;

/// Bounds of every run of the text in `range` on a laid-out paragraph, so a
/// word with a space inside measures whole.
fn range_extent(paragraph: &Paragraph, range: std::ops::Range<usize>) -> Rect {
    let rects = paragraph.get_rects_for_range(range, RectHeightStyle::Tight, RectWidthStyle::Tight);
    let mut out = match rects.first() {
        Some(first) => first.rect,
        None => return Rect::from_xywh(0.0, 0.0, 0.0, 0.0),
    };
    for r in &rects[1..] {
        out.join(r.rect);
    }
    out
}

/// Left and right Taffy insets of a caption box: the side margin of its
/// anchor's side.
fn side_insets(style: &CaptionStyle) -> (f32, f32) {
    match &style.position {
        CaptionPosition::TopLeft | CaptionPosition::BottomLeft | CaptionPosition::Left => (style.left_margin, 0.0),
        CaptionPosition::TopRight | CaptionPosition::BottomRight | CaptionPosition::Right => {
            (0.0, style.right_margin)
        }
        _ => (0.0, 0.0),
    }
}

/// A word as emphasis matching sees it: lowercase, edge punctuation removed,
/// so `"$600"`, `600` and `600,` match.
pub fn emphasis_key(word: &str) -> String {
    word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

/// Whether `word` is one of `style.emphasis_words`.
pub fn is_emphasis_word(style: &CaptionStyle, word: &str) -> bool {
    if style.emphasis_words.is_empty() {
        return false;
    }
    let key = emphasis_key(word);
    !key.is_empty() && style.emphasis_words.iter().any(|w| emphasis_key(w) == key)
}

/// English articles, prepositions, conjunctions and possessives that read
/// badly at the end of a caption line.
const WEAK_WORDS: &[&str] = &[
    "a", "an", "the", "of", "to", "in", "on", "at", "for", "with", "by", "from", "into", "onto", "over", "under",
    "about", "as", "than", "and", "or", "but", "nor", "so", "if", "my", "your", "our", "their", "his", "her", "its",
    "this", "these", "those", "is", "are", "was", "were",
];

/// Whether `word` ends a clause: a trailing `,` `.` `;` `:` `!` or `?`.
pub fn ends_clause(word: &str) -> bool {
    word.trim_end().ends_with([',', '.', ';', ':', '!', '?'])
}

/// Whether a line should not end on `word`.
pub fn is_weak_word(word: &str) -> bool {
    // A word that ends a clause carries its own pause.
    !ends_clause(word) && WEAK_WORDS.contains(&emphasis_key(word).as_str())
}

/// Whether `word` reads badly on a line of its own: a weak word or one of
/// three letters or fewer that does not end a clause.
pub fn is_weak_alone(word: &str) -> bool {
    !ends_clause(word) && (is_weak_word(word) || emphasis_key(word).chars().count() <= 3)
}

/// How a line split of some words reads, word by word.
struct BreakHints {
    /// A line should not end on this word.
    weak: Vec<bool>,
    /// The word ends a clause, so a line should end on it.
    clause: Vec<bool>,
    /// The word should not stand on a line alone.
    weak_alone: Vec<bool>,
}

impl BreakHints {
    fn new(words: &[&str], n: usize) -> Self {
        let flag = |f: fn(&str) -> bool| (0..n).map(|i| words.get(i).is_some_and(|w| f(w))).collect();
        Self { weak: flag(is_weak_word), clause: flag(ends_clause), weak_alone: flag(is_weak_alone) }
    }

    /// Flaws of the line holding words `i..j` of `n`: a weak last word, each
    /// clause end inside it, and a weak word alone.
    fn flaws(&self, i: usize, j: usize, n: usize) -> usize {
        let weak_end = j < n && self.weak[j - 1];
        let inside = self.clause[i..j - 1].iter().filter(|&&c| c).count();
        let alone = n > 1 && j - i == 1 && self.weak_alone[i];
        usize::from(weak_end) + inside + usize::from(alone)
    }

    /// Flaws of a whole split.
    fn count(&self, lines: &[usize]) -> usize {
        let n = lines.len();
        let mut total = 0;
        let mut start = 0;
        for j in 1..=n {
            if j == n || lines[j] != lines[j - 1] {
                total += self.flaws(start, j, n);
                start = j;
            }
        }
        total
    }
}

/// Display line of each word: the fewest lines that fit `max_width` with at
/// most `max_words` words each, then the split among those with the most even
/// widths and the fewest flaws. A flaw is a line ending on a weak word
/// ([`is_weak_word`]), a clause end (`,` `.` `;` `:` `!` `?`) inside a line,
/// or a weak word alone ([`is_weak_alone`]). One more line wins when it has
/// fewer flaws. A lower line stays at least as wide as the one above it when
/// both show together (`shown_together` lines per page). A word wider than
/// `max_width` takes a line of its own.
pub fn balanced_breaks(
    widths: &[f32],
    space: f32,
    max_width: f32,
    max_words: usize,
    shown_together: usize,
    words: &[&str],
) -> Vec<usize> {
    let n = widths.len();
    if n == 0 {
        return Vec::new();
    }
    let hints = BreakHints::new(words, n);
    let max_words = max_words.max(1);
    let mut prefix = vec![0.0f32; n + 1];
    for i in 0..n {
        prefix[i + 1] = prefix[i] + widths[i];
    }
    let line_w = |i: usize, j: usize| prefix[j] - prefix[i] + space * (j - i - 1) as f32;
    let fits = |i: usize, j: usize| j - i <= max_words && (j - i == 1 || line_w(i, j) <= max_width + 0.5);

    // Fewest lines.
    let mut fewest = vec![usize::MAX; n + 1];
    fewest[0] = 0;
    for j in 1..=n {
        for i in j.saturating_sub(max_words)..j {
            if fewest[i] != usize::MAX && fits(i, j) {
                fewest[j] = fewest[j].min(fewest[i] + 1);
            }
        }
    }
    let k = fewest[n];
    let greedy = || {
        let mut out = Vec::with_capacity(n);
        let (mut line, mut start) = (0usize, 0usize);
        for j in 0..n {
            if j > start && !fits(start, j + 1) {
                line += 1;
                start = j;
            }
            out.push(line);
        }
        out
    };
    if k <= 1 || n > 64 {
        return if k <= 1 { vec![0; n] } else { greedy() };
    }

    let fewest_lines = solve_breaks(k, &line_w, &fits, n, prefix[n], space, shown_together, &hints);
    match fewest_lines {
        // One more line is worth it when it has fewer flaws.
        Some(lines) if hints.count(&lines) > 0 && k < n => {
            match solve_breaks(k + 1, &line_w, &fits, n, prefix[n], space, shown_together, &hints) {
                Some(more) if hints.count(&more) < hints.count(&lines) => more,
                _ => lines,
            }
        }
        Some(lines) => lines,
        None => greedy(),
    }
}

/// The lowest-cost split of `n` words into exactly `k` lines, if one fits.
#[allow(clippy::too_many_arguments)]
fn solve_breaks(
    k: usize,
    line_w: &dyn Fn(usize, usize) -> f32,
    fits: &dyn Fn(usize, usize) -> bool,
    n: usize,
    total: f32,
    space: f32,
    shown_together: usize,
    hints: &BreakHints,
) -> Option<Vec<usize>> {
    let mean = ((total + space * (n - k) as f32) / k as f32).max(1.0);
    let line_cost = |i: usize, j: usize| {
        let d = (line_w(i, j) - mean) / mean;
        d * d + 0.5 * hints.flaws(i, j, n) as f32
    };
    let pair_cost = |top: (usize, usize), bottom: (usize, usize), top_line: usize| {
        if shown_together > 1 && top_line / shown_together == (top_line + 1) / shown_together {
            let over = line_w(top.0, top.1) - line_w(bottom.0, bottom.1);
            if over > 0.0 {
                return 0.15 * over / mean;
            }
        }
        0.0
    };

    // best[l][j][s]: lowest cost of words 0..j in l + 1 lines, the last
    // starting at s.
    let inf = f32::INFINITY;
    let mut best = vec![vec![vec![inf; n]; n + 1]; k];
    let mut from = vec![vec![vec![usize::MAX; n]; n + 1]; k];
    for (j, first) in best[0].iter_mut().enumerate().skip(1) {
        if fits(0, j) {
            first[0] = line_cost(0, j);
        }
    }
    for l in 1..k {
        for s in 1..n {
            for j in (s + 1)..=n {
                if !fits(s, j) {
                    continue;
                }
                let here = line_cost(s, j);
                for ps in 0..s {
                    let prev = best[l - 1][s][ps];
                    if prev == inf {
                        continue;
                    }
                    let c = prev + here + pair_cost((ps, s), (s, j), l - 1);
                    if c < best[l][j][s] {
                        best[l][j][s] = c;
                        from[l][j][s] = ps;
                    }
                }
            }
        }
    }
    let mut s = (0..n)
        .filter(|&s| best[k - 1][n][s] < inf)
        .min_by(|&a, &b| best[k - 1][n][a].total_cmp(&best[k - 1][n][b]))?;
    let mut out = vec![0; n];
    let mut j = n;
    for l in (0..k).rev() {
        for slot in out.iter_mut().take(j).skip(s) {
            *slot = l;
        }
        if l > 0 {
            let ps = from[l][j][s];
            j = s;
            s = ps;
        }
    }
    Some(out)
}

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
    fn export_body(
        &self,
        cx: &mut crate::scene_export::ExportContext,
    ) -> Result<crate::scene_export::ElementExport, crate::scene_export::ExportError> {
        crate::scene_export::caption::caption_body(self, cx)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn kind(&self) -> &'static str {
        "caption"
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
        let block_width = (layout_width * self.style.layout_width_fraction).max(20.0);

        // Derive content height from the same paragraph width and metrics as render.
        let stack_h = {
            let mut guard = self.block_layout.lock().unwrap_or_else(|e| e.into_inner());
            let needs_rebuild = guard.as_ref().map(|l| (l.width - block_width).abs() > 0.5).unwrap_or(true);
            if needs_rebuild {
                *guard = Some(self.build_block_layout(block_width));
            }
            guard.as_ref().map(|l| l.total_height).unwrap_or(0.0)
        };

        // Margins are encoded in the Taffy style insets (via
        // `rebuild_layout_style`), so the measured size is the caption
        // block height alone.
        Size {
            width: layout_width,
            height: stack_h,
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
        if let Some(base) = &self.base_placement {
            let target = self.positions.iter().rev().find(|p| p.at <= time).unwrap_or(base).clone();
            self.apply_placement(&target);
        }
        true
    }

    fn post_layout(&mut self, rect: Rect) {
        self.set_box_width(Some(rect.width()));
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

        let layout_style = self.style.resolved();
        let block_width = (rect.width() * layout_style.layout_width_fraction).max(20.0);

        // Build / reuse block layout.
        let mut layout_guard = self.block_layout.lock().unwrap_or_else(|e| e.into_inner());
        let needs_rebuild = layout_guard.as_ref().map(|l| (l.width - block_width).abs() > 0.5).unwrap_or(true);
        if needs_rebuild {
            *layout_guard = Some(self.build_block_layout(block_width));
        }

        let layout = match layout_guard.as_ref() {
            Some(l) => l,
            None => {
                draw_children(canvas);
                return Ok(());
            }
        };

        // Standard captions: one block at a time in the same anchor position.
        // When SRT cues overlap, only the newest (latest start_sec) is shown.
        let render_cues: Vec<(usize, &CaptionLine)> = visible
            .iter()
            .max_by(|a, b| {
                a.1.start_sec
                    .partial_cmp(&b.1.start_sec)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|entry| vec![*entry])
            .unwrap_or_default();

        // ── Single canonical global active token ────────────────
        let global_active = Self::find_global_active(&render_cues, current_time);

        // No clip: the outline, shadow, pill and pop reach past the measured
        // text box, as CSS overflow does.
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

        // ── Render the active cue, each display line. ─────────────
        for &(line_idx, line) in &render_cues {
            let cue_layout = match layout.cues.get(line_idx) {
                Some(c) => c,
                None => continue,
            };

            // Block top Y, depending on caption position.
            let cue_block_top_y = Self::compute_cue_block_top_y(&layout_style, rect, layout);

            let phrase_replace =
                layout_style.phrase_line_mode == PhraseLineMode::Replace;
            let current_display_line = if phrase_replace {
                Some(current_display_line_index(
                    line,
                    line_idx,
                    current_time,
                    &layout_style,
                    global_active,
                ))
            } else {
                None
            };

            // Replace mode shows the page of the current line: up to
            // `max_lines` lines, bottom-aligned in the block.
            let page_len = layout_style.max_lines.max(1);
            let current_page = current_display_line.map(|dl| dl / page_len);
            let mut line_y_cursor: f32 = cue_block_top_y;
            if let Some(page) = current_page {
                let page_lines = cue_layout.display_lines.iter().skip(page * page_len).take(page_len);
                let page_count = page_lines.clone().count();
                let page_h: f32 = page_lines.map(|dl| dl.line_height).sum::<f32>()
                    + page_count.saturating_sub(1) as f32 * layout_style.line_spacing;
                line_y_cursor = cue_block_top_y + layout.total_height - page_h;
            }

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
                if phrase_replace && Some(line_idx_idx / page_len) != current_page {
                    continue;
                }

                let reveal = line_reveal_factor(
                    line,
                    line_idx_idx,
                    current_time,
                    &layout_style,
                );
                if reveal <= 0.0 {
                    line_y_cursor += display_line.line_height + layout_style.line_spacing;
                    continue;
                }

                // ── Position each DISPLAY LINE horizontally based on the
                //    caption's anchor. Precomputed `line_width` is used to
                //    avoid relying on a token origin as the line's left edge.
                let line_left_x =
                    Self::compute_line_left_x(&layout_style.position, rect, display_line.line_width);

                let line_render_alpha = effective_opacity * reveal;
                let line_needs_layer = line_render_alpha < 0.999;

                // A one-word line wider than the column paints scaled to
                // fit, centred on the box it would take at full size.
                let fitted = display_line.fit < 1.0;
                if fitted {
                    let fit = display_line.fit;
                    let left =
                        Self::compute_line_left_x(&layout_style.position, rect, display_line.line_width * fit);
                    canvas.save();
                    canvas.translate(Point::new(left, line_y_cursor + display_line.line_height * (1.0 - fit) * 0.5));
                    canvas.scale((fit, fit));
                    canvas.translate(Point::new(-line_left_x, -line_y_cursor));
                }

                let active_local_in_line: Option<usize> = active_in_this_cue
                    .and_then(|active| display_line.tokens.iter().position(|t| t.word_idx == active.word_idx));

                // The node opacity and the line reveal fade the pill and
                // every coat together.
                if line_needs_layer {
                    let bbox = Self::line_paint_bounds(&layout_style, display_line, line_left_x, line_y_cursor);
                    canvas.save_layer_alpha_f(bbox, line_render_alpha);
                }

                Self::paint_line_pill(canvas, line_left_x, line_y_cursor, display_line, &layout_style, 1.0);

                // Base pass: always paint the full line at scale 1.0 so
                // non-active words stay visible every frame. While the
                // active word pops, it is transparent here and the
                // overlay below paints it, so it never shows twice.
                let popping = if active_pop_active { active_local_in_line } else { None };
                let frame = LineFrame {
                    style: &layout_style,
                    display_line,
                    line,
                    time: current_time,
                    layout_width: display_line.paragraph_layout_width,
                    active: active_local_in_line,
                    x: line_left_x,
                    y: line_y_cursor,
                };
                self.paint_line(&frame, if popping.is_some() { PopPass::ExceptActive } else { PopPass::All }, canvas);

                // Pop overlay: during the activation window, paint the
                // line again scaled around the active word, with every
                // other word transparent. No clip, so the scaled word's
                // outline and shadow stay whole.
                if let Some(local_idx) = popping {
                    let tokens = &display_line.tokens;
                    let active_token = &tokens[local_idx];
                    let cx = line_left_x + active_token.center_x;
                    let cy = line_y_cursor + active_token.center_y;
                    // The popped word's outline never reaches a neighbor's
                    // glyphs: each side grows at most half the gap left
                    // after its own outline, so a long word pops less.
                    let outline = layout_style.outline();
                    let gap = |a: &TokenLayout, b: &TokenLayout| b.x - (a.x + a.width) - outline;
                    let mut room = f32::INFINITY;
                    if local_idx > 0 {
                        room = room.min(gap(&tokens[local_idx - 1], active_token));
                    }
                    if local_idx + 1 < tokens.len() {
                        room = room.min(gap(active_token, &tokens[local_idx + 1]));
                    }
                    let scale = pop_scale.min(1.0 + room.max(0.0) / active_token.width.max(1.0));
                    canvas.save();
                    canvas.translate(Point::new(cx, cy));
                    canvas.scale((scale, scale));
                    canvas.translate(Point::new(-cx, -cy));
                    self.paint_line(&frame, PopPass::ActiveOnly, canvas);
                    canvas.restore();
                }

                if line_needs_layer {
                    canvas.restore();
                }
                if fitted {
                    canvas.restore();
                }

                line_y_cursor += display_line.line_height + layout_style.line_spacing;
            }
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
    ) -> Result<(), String> {
        match property {
            "opacity" => {
                let ease_fn = crate::scripting::utils::parse_easing_result(easing)?;
                self.opacity.add_segment(start, target, duration, ease_fn);
                Ok(())
            }
            other => Err(format!(
                "unknown property '{other}' on caption; valid: opacity"
            )),
        }
    }

    fn set_opacity(&mut self, opacity: f32) {
        self.opacity.current_value = opacity;
    }
}

impl CaptionNode {
    /// Paint one display line coat by coat: shadow, glow, outline, fill.
    /// Words that share an opacity paint inside one layer at that opacity,
    /// so a faded word's outline never shows through its own fill.
    fn paint_line(&self, frame: &LineFrame, pass: PopPass, canvas: &Canvas) {
        let style = frame.style;
        let words: Vec<(Color, f32)> = frame
            .display_line
            .tokens
            .iter()
            .enumerate()
            .map(|(local_idx, token)| {
                let word = &frame.line.words[token.word_idx];
                let is_active = Some(local_idx) == frame.active;
                let hidden = match pass {
                    PopPass::All => false,
                    PopPass::ExceptActive => is_active,
                    PopPass::ActiveOnly => !is_active,
                };
                let emphasis = self.style.emphasis_color.filter(|_| is_emphasis_word(&self.style, &word.text));
                let (color, opacity) = if is_active {
                    (self.style.highlight_color, self.style.active_opacity)
                } else if frame.time >= word.end_sec {
                    (emphasis.unwrap_or(self.style.spoken_color), self.style.spoken_color_opacity)
                } else if let Some(c) = emphasis {
                    (c, self.style.future_opacity)
                } else if frame.time < word.start_sec {
                    (self.compute_word_color(word, frame.time, false), self.style.future_opacity)
                } else {
                    (self.style.color, self.style.future_opacity)
                };
                (color, if hidden { 0.0 } else { opacity.clamp(0.0, 1.0) })
            })
            .collect();

        let mut groups: Vec<f32> = Vec::new();
        for &(_, opacity) in &words {
            if opacity > 0.0 && !groups.iter().any(|g| (g - opacity).abs() < 1e-4) {
                groups.push(opacity);
            }
        }
        let mut coats = Vec::with_capacity(4);
        if style.text_shadow_enabled {
            coats.push(Coat::Shadow);
        }
        if style.glow_enabled && style.glow_blur > 0.0 {
            coats.push(Coat::Glow);
        }
        if style.outline() > 0.0 {
            coats.push(Coat::Stroke);
        }
        coats.push(Coat::Fill);

        let bounds = Self::line_paint_bounds(style, frame.display_line, frame.x, frame.y);
        for group in groups {
            let in_group = |opacity: f32| (opacity - group).abs() < 1e-4;
            let layered = group < 0.999;
            if layered {
                canvas.save_layer_alpha_f(bounds, group);
            }
            for &coat in &coats {
                let visible: Vec<bool> = words
                    .iter()
                    .enumerate()
                    .map(|(i, &(_, opacity))| {
                        in_group(opacity) && (coat != Coat::Glow || Some(i) == frame.active)
                    })
                    .collect();
                if !visible.contains(&true) {
                    continue;
                }
                let paragraph = self.coat_paragraph(frame, &words, &visible, coat);
                let (dx, dy) = if coat == Coat::Shadow {
                    (style.text_shadow_offset_x, style.text_shadow_offset_y)
                } else {
                    (0.0, 0.0)
                };
                paragraph.paint(canvas, (frame.x + dx, frame.y + dy));
            }
            if layered {
                canvas.restore();
            }
        }
    }

    /// One coat of a display line as a paragraph. Every coat shapes the same
    /// text with the same font, so its glyphs land on the cached token
    /// positions.
    fn coat_paragraph(&self, frame: &LineFrame, words: &[(Color, f32)], visible: &[bool], coat: Coat) -> Paragraph {
        let style = frame.style;
        let tokens = &frame.display_line.tokens;
        let line_text: String = tokens
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
                    skia_safe::textlayout::FontCollection::new(),
                );
                builder.add_text("");
                crate::counters::count(crate::counters::Counter::paragraphs_shaped);
                return builder.build();
            }
        };
        let mut builder = ParagraphBuilder::new(&paragraph_style, &*font_guard);
        for (local_idx, token) in tokens.iter().enumerate() {
            let word = &frame.line.words[token.word_idx];
            let ts = Self::make_word_text_style(style, words[local_idx].0, visible[local_idx], coat);
            builder.push_style(&ts);
            let display_text = format_caption_word(&word.text, style.uppercase);
            if local_idx + 1 < tokens.len() {
                builder.add_text(format!("{display_text} "));
            } else {
                builder.add_text(&display_text);
            }
            builder.pop();
        }
        crate::counters::count(crate::counters::Counter::paragraphs_shaped);
        let mut p = builder.build();
        p.layout(frame.layout_width);
        p
    }

    /// Everything a display line paints: pill, outline, shadow and glow, with
    /// room for the pop.
    fn line_paint_bounds(style: &CaptionStyle, display_line: &DisplayLineLayout, x: f32, y: f32) -> Rect {
        let mut pad = style.outline() + style.font_size * (style.active_pop_scale - 1.0).max(0.0) + 2.0;
        if style.pill_enabled {
            pad = pad.max(style.pill_padding_h.max(style.pill_padding_v) + 2.0);
        }
        if style.text_shadow_enabled {
            pad += style.text_shadow_offset_x.abs().max(style.text_shadow_offset_y.abs()) + 3.0 * style.text_shadow_blur;
        }
        if style.glow_enabled {
            pad += 3.0 * style.glow_blur;
        }
        Rect::from_xywh(x, y, display_line.line_width, display_line.line_height).with_outset((pad, pad))
    }
}

/// One display line placed for one frame.
struct LineFrame<'a> {
    style: &'a CaptionStyle,
    display_line: &'a DisplayLineLayout,
    line: &'a CaptionLine,
    time: f64,
    layout_width: f32,
    /// The active word's index among the line's tokens.
    active: Option<usize>,
    x: f32,
    y: f32,
}

/// One paint pass of a caption line, painted in declaration order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Coat {
    Shadow,
    Glow,
    Stroke,
    Fill,
}

/// Which words a line paint draws. While the active word pops, the base
/// pass leaves it transparent and the scaled overlay draws only it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PopPass {
    All,
    ExceptActive,
    ActiveOnly,
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

/// Display line to show for phrase-break `Replace` mode at `time`.
pub fn current_display_line_index(
    line: &CaptionLine,
    cue_line_idx: usize,
    time: f64,
    style: &CaptionStyle,
    global_active: Option<(usize, usize)>,
) -> usize {
    if let Some((ci, wi)) = global_active {
        if ci == cue_line_idx {
            return line.words[wi].display_line_idx;
        }
    }
    for w in &line.words {
        if time >= w.start_sec && time < w.end_sec {
            return w.display_line_idx;
        }
    }
    let n = display_line_count(line);
    for dl in (0..n).rev() {
        if line_reveal_factor(line, dl, time, style) > 0.0 {
            return dl;
        }
    }
    0
}

/// Number of stacked display lines in a cue (phrase-break or `\n`).
pub fn display_line_count(line: &CaptionLine) -> usize {
    line.words
        .iter()
        .map(|w| w.display_line_idx)
        .max()
        .map(|m| m + 1)
        .unwrap_or(1)
}

/// Fractional `[start, end)` window of cue duration for a display line's
/// word timing and reveal. Two-line cues use configured offsets; 3+ lines
/// split the cue evenly so phrase-break karaoke stays aligned.
pub fn display_line_window_fracs(
    style: &CaptionStyle,
    display_line_idx: usize,
    num_display_lines: usize,
) -> (f32, f32) {
    let n = num_display_lines.max(1);
    if n <= 2 && style.line_reveal_offsets.len() >= 2 {
        let start = style
            .line_reveal_offsets
            .get(display_line_idx)
            .copied()
            .unwrap_or(0.0);
        let end = style
            .line_reveal_offsets
            .get(display_line_idx + 1)
            .copied()
            .unwrap_or(1.0);
        (start, end.max(start))
    } else {
        let start = display_line_idx as f32 / n as f32;
        let end = (display_line_idx + 1) as f32 / n as f32;
        (start, end)
    }
}

/// Line-reveal factor: returns 0.0 → 1.0 over `reveal_duration`
/// seconds starting at `cue.start_sec + offset_frac * cue_duration`.
///
/// A measured cue shows line 0 from `cue.start_sec` and finishes each later
/// line's ramp at its first word's start, so a spoken word is never faded.
pub fn line_reveal_factor(
    line: &CaptionLine,
    display_line_idx: usize,
    time: f64,
    style: &CaptionStyle,
) -> f32 {
    // A page of lines in `Replace` mode appears together with its first line.
    let display_line_idx = if style.phrase_line_mode == PhraseLineMode::Replace {
        let page = style.max_lines.max(1);
        display_line_idx / page * page
    } else {
        display_line_idx
    };
    let duration = style
        .line_reveal_durations
        .get(display_line_idx)
        .copied()
        .unwrap_or(0.10);
    if line.measured {
        if display_line_idx == 0 {
            return if time >= line.start_sec { 1.0 } else { 0.0 };
        }
        let first = line
            .words
            .iter()
            .filter(|w| w.display_line_idx == display_line_idx)
            .map(|w| w.start_sec)
            .fold(f64::INFINITY, f64::min);
        if first.is_finite() {
            if time >= first {
                return 1.0;
            }
            let reveal_start = (first - duration as f64).max(line.start_sec);
            let span = first - reveal_start;
            if span <= 0.0 || time < reveal_start {
                return 0.0;
            }
            let raw = ((time - reveal_start) / span) as f32;
            return CaptionNode::ease_out_sine_inline(raw).clamp(0.0, 1.0);
        }
    }
    let total_dur = (line.end_sec - line.start_sec).max(0.001);
    let num_lines = display_line_count(line);
    let (offset_frac, _) =
        display_line_window_fracs(style, display_line_idx, num_lines);
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

// ── Platform presets and phrase break ─────────────────────────────

/// Reference canvas height for 9:16 short-form safe-area scaling (1080×1920).
pub const REFERENCE_SHORT_FORM_HEIGHT: f32 = 1920.0;

/// A platform safe area at its native 1080x1920 size.
pub fn safe_zone(preset: &str) -> Option<crate::types::SafeZone> {
    scaled_safe_zone(preset, REFERENCE_SHORT_FORM_HEIGHT)
}

/// Names `safe_area_preset` accepts, sorted.
pub fn safe_area_presets() -> Vec<String> {
    let mut names: Vec<String> = DesignSystem::new().safe_areas.keys().cloned().collect();
    names.sort();
    names
}

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

    let num_lines = max_dl + 1;
    for dl_idx in 0..=max_dl {
        let Some(indices) = groups.get(&dl_idx) else {
            continue;
        };
        let (offset_start, offset_end) =
            display_line_window_fracs(style, dl_idx, num_lines);
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

/// Re-run phrase break for all cues (after style props change). Measured
/// cues keep their word times; estimated cues re-split each display line's
/// window evenly.
pub fn refresh_caption_lines(lines: &mut [CaptionLine], style: &CaptionStyle) {
    let max_words = style.max_words_per_line;
    refresh_caption_lines_by(lines, style, &mut |line: &CaptionLine| {
        (0..line.words.len()).map(|i| i / max_words.max(1)).collect()
    });
}

/// `refresh_caption_lines` with `breaks` giving each word's display line.
pub fn refresh_caption_lines_by(
    lines: &mut [CaptionLine],
    style: &CaptionStyle,
    breaks: &mut dyn FnMut(&CaptionLine) -> Vec<usize>,
) {
    for line in lines.iter_mut() {
        if line.text.contains('\n') {
            continue;
        }
        if style.auto_phrase_break {
            if line.words.is_empty() {
                continue;
            }
            let assigned = breaks(line);
            for (w, dl) in line.words.iter_mut().zip(assigned) {
                w.display_line_idx = dl;
            }
            if !line.measured {
                retime_words_by_display_line(line, style);
            }
        } else if line.words.iter().any(|w| w.display_line_idx != 0) {
            // A cue without breaks is one display line. Estimated words split
            // the cue evenly again so no time stays tied to a removed line.
            if line.measured {
                for w in line.words.iter_mut() {
                    w.display_line_idx = 0;
                }
            } else {
                line.words.clear();
                auto_time_words(line);
            }
        }
    }
}

/// The word highlighted at `time`: `(cue index, word index)` into `lines`,
/// as the renderer picks it. The newest cue showing at `time` wins, and in it
/// the word whose `[start, end)` holds `time` with the latest start.
pub fn active_word_at(lines: &[CaptionLine], time: f64) -> Option<(usize, usize)> {
    let cue = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| time >= line.start_sec && time < line.end_sec)
        .max_by(|a, b| a.1.start_sec.partial_cmp(&b.1.start_sec).unwrap_or(std::cmp::Ordering::Equal))?;
    CaptionNode::find_global_active(&[cue], time)
}

/// Groups measured words into cues by `rules`, in start order. Words with no
/// text are dropped. A cue cut for length (not at a pause) hands the words
/// after its last clause end to the next cue when it keeps at least a third,
/// else its trailing weak words ([`is_weak_word`]). Every cue is measured and
/// spans its first word's start to its last word's end.
pub fn cues_from_words(words: &[TimedWord], rules: &CueRules) -> Vec<CaptionLine> {
    let mut words: Vec<&TimedWord> = words.iter().filter(|w| !w.text.trim().is_empty()).collect();
    words.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<CaptionLine> = Vec::new();
    let mut open = false;
    let mut previous_end = f64::NEG_INFINITY;
    for word in words {
        let text = word.text.trim();
        let mut carried = Vec::new();
        if open {
            let cue = out.last_mut().expect("an open cue exists");
            let paused = word.start - previous_end >= rules.max_gap;
            let fits = cue.text.chars().count() + 1 + text.chars().count() <= rules.max_chars
                && word.end - cue.start_sec <= rules.max_duration
                && !paused;
            open = fits;
            // A cue cut for length ends at its last clause end when that
            // keeps at least a third of it, else before its trailing articles
            // and prepositions, so no cue ends on "to the".
            if !fits && !paused {
                let len = cue.words.len();
                let after_clause = cue.words.iter().rposition(|w| ends_clause(&w.text)).map(|i| len - 1 - i);
                let tail = after_clause.filter(|&t| t > 0 && t * 3 <= len * 2).unwrap_or_else(|| {
                    cue.words.iter().rev().take_while(|w| is_weak_word(&w.text)).count()
                });
                if tail < len {
                    carried = cue.words.split_off(len - tail);
                    cue.text = cue.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
                    cue.end_sec = cue.words.iter().map(|w| w.end_sec).fold(cue.start_sec, f64::max);
                }
            }
        }
        if !open {
            let start = carried.first().map_or(word.start, |w| w.start_sec);
            out.push(CaptionLine {
                text: carried.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "),
                end_sec: carried.iter().map(|w| w.end_sec).fold(word.end, f64::max),
                words: carried,
                start_sec: start,
                measured: true,
            });
            open = true;
        }
        let cue = out.last_mut().expect("a cue was just pushed");
        if !cue.text.is_empty() {
            cue.text.push(' ');
        }
        cue.text.push_str(text);
        cue.end_sec = cue.end_sec.max(word.end);
        cue.words.push(CaptionWord {
            text: text.to_string(),
            start_sec: word.start,
            end_sec: word.end.max(word.start),
            display_line_idx: 0,
        });
        previous_end = word.end;
        if rules.break_on_sentence && text.ends_with(['.', '?', '!']) {
            open = false;
        }
    }
    let hold = rules.hold.max(0.0);
    let starts: Vec<f64> = out.iter().skip(1).map(|c| c.start_sec).collect();
    for (cue, next) in out.iter_mut().zip(starts.into_iter().map(Some).chain(std::iter::once(None))) {
        let held = cue.end_sec + hold;
        cue.end_sec = next.map_or(held, |next| held.min(next).max(cue.end_sec));
    }
    out
}

/// The cues of a caption file named `name`: a word-timed JSON transcript
/// grouped by `rules`, WebVTT or SRT, chosen by extension and then by
/// content. A file with no cues fails.
pub fn caption_lines_from_file(name: &str, content: &str, rules: &CueRules) -> Result<Vec<CaptionLine>, String> {
    let body = content.trim_start_matches('\u{feff}').trim_start();
    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let json = |content: &str| -> Result<Vec<CaptionLine>, String> {
        Ok(cues_from_words(&parse_transcript_words(content).map_err(|e| format!("{name:?}: {e}"))?, rules))
    };
    let lines = match ext.as_deref() {
        Some("json") => json(content)?,
        Some("vtt") => parse_vtt(content),
        Some("srt") => parse_srt(content),
        _ if body.starts_with('[') || body.starts_with('{') => json(content)?,
        _ if body.starts_with("WEBVTT") => parse_vtt(content),
        _ => parse_srt(content),
    };
    if lines.is_empty() {
        return Err(format!("no caption cues in {name:?}"));
    }
    Ok(lines)
}

/// Reads a word-timed transcript: a JSON array of `{text, start, end}` words
/// (`word` works for `text`), or an object holding such a `words` array, such
/// as the output of `director-engine asr transcribe --format json`. Times are
/// seconds. A word whose times are not finite or whose end precedes its start
/// fails.
pub fn parse_transcript_words(content: &str) -> Result<Vec<TimedWord>, String> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Transcript {
        Words(Vec<TimedWord>),
        Object { words: Vec<TimedWord> },
    }
    let words = match serde_json::from_str::<Transcript>(content.trim_start_matches('\u{feff}')) {
        Ok(Transcript::Words(words)) | Ok(Transcript::Object { words }) => words,
        Err(e) => {
            return Err(format!(
                "not a word-timed transcript ({e}); expected [{{\"text\", \"start\", \"end\"}}, ...] or {{\"words\": [...]}}"
            ))
        }
    };
    if let Some(bad) = words
        .iter()
        .find(|w| !(w.start.is_finite() && w.end.is_finite() && w.start >= 0.0 && w.end >= w.start))
    {
        return Err(format!(
            "word {:?} has start {} and end {}; times must be finite seconds with end >= start >= 0",
            bad.text, bad.start, bad.end
        ));
    }
    Ok(words)
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
    /// The style with its platform safe area applied to the top and bottom
    /// margins.
    pub fn resolved(&self) -> CaptionStyle {
        let mut s = self.clone();
        if let Some(zone) = s.safe_area_preset.as_deref().and_then(safe_zone) {
            s.bottom_margin = zone.bottom;
            s.top_margin = zone.top;
        }
        s
    }

    /// The visible outline width in px, 0 when the outline is off.
    pub fn outline(&self) -> f32 {
        if self.stroke_enabled {
            self.stroke_width.max(0.0)
        } else {
            0.0
        }
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

/// Reads SubRip cues. SRT holds cue times only, so each cue's words split it
/// evenly: an estimate. The index line is optional and CRLF line ends work.
pub fn parse_srt(content: &str) -> Vec<CaptionLine> {
    cue_blocks(content)
        .into_iter()
        .map(|(start_sec, end_sec, text)| {
            let mut line = CaptionLine {
                text: text.join("\n"),
                words: Vec::new(),
                start_sec,
                end_sec,
                measured: false,
            };
            auto_time_words(&mut line);
            line
        })
        .collect()
}

/// Reads WebVTT cues. A cue with inline timestamps (`<00:00:01.250>word`, the
/// karaoke form `director-engine asr transcribe --format vtt` writes) is
/// measured: each word starts at the timestamp before it, the first word at
/// the cue start, and ends where the next word starts or the cue ends. A cue
/// without them splits its words evenly, an estimate. Cue settings, `NOTE`,
/// `STYLE` and `REGION` blocks and tags such as `<c>`, `<v>` and `<i>` are
/// skipped; `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&#39;` and `&nbsp;` decode.
pub fn parse_vtt(content: &str) -> Vec<CaptionLine> {
    cue_blocks(content)
        .into_iter()
        .map(|(start_sec, end_sec, text)| vtt_cue(start_sec, end_sec, &text))
        .collect()
}

/// `(start, end, text lines)` of each cue in SRT or WebVTT text.
fn cue_blocks(content: &str) -> Vec<(f64, f64, Vec<String>)> {
    let content = content.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let mut out = Vec::new();
    let mut block: Vec<&str> = Vec::new();
    let mut flush = |block: &mut Vec<&str>| {
        if let Some(at) = block.iter().position(|l| l.contains("-->")) {
            if let Some((start, end)) = parse_cue_timing(block[at]) {
                let text: Vec<String> = block[at + 1..].iter().map(|l| l.trim().to_string()).collect();
                if text.iter().any(|l| !l.is_empty()) {
                    out.push((start, end, text));
                }
            }
        }
        block.clear();
    };
    for line in content.lines().chain(std::iter::once("")) {
        if line.trim().is_empty() {
            flush(&mut block);
        } else {
            block.push(line);
        }
    }
    out
}

/// `HH:MM:SS,mmm --> HH:MM:SS,mmm`, `.` for `,`, hours optional, cue settings ignored.
fn parse_cue_timing(line: &str) -> Option<(f64, f64)> {
    let (start, rest) = line.split_once("-->")?;
    let end = rest.split_whitespace().next()?;
    Some((parse_timestamp(start.trim())?, parse_timestamp(end)?))
}

/// Seconds of `HH:MM:SS.mmm`, `MM:SS.mmm` or `HH:MM:SS,mmm`.
fn parse_timestamp(s: &str) -> Option<f64> {
    let s = s.replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    let (h, m, sec) = match parts.as_slice() {
        [h, m, sec] => (h.parse::<f64>().ok()?, m.parse::<f64>().ok()?, sec.parse::<f64>().ok()?),
        [m, sec] => (0.0, m.parse::<f64>().ok()?, sec.parse::<f64>().ok()?),
        _ => return None,
    };
    let total = h * 3600.0 + m * 60.0 + sec;
    total.is_finite().then_some(total)
}

fn decode_entities(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&lrm;", "")
        .replace("&rlm;", "")
        .replace("&amp;", "&")
}

/// One WebVTT cue: tags stripped, words timed by inline timestamps when present.
fn vtt_cue(start_sec: f64, end_sec: f64, lines: &[String]) -> CaptionLine {
    // (display line, word text, start of the word when a timestamp precedes it)
    let mut words: Vec<(usize, String, f64)> = Vec::new();
    let mut timed = false;
    let mut clean_lines = Vec::new();
    for (line_idx, raw) in lines.iter().enumerate() {
        let mut clean = String::new();
        let mut current = String::new();
        let mut current_start = None;
        let mut mark = start_sec;
        let mut rest = raw.as_str();
        let push_word = |current: &mut String, current_start: &mut Option<f64>, words: &mut Vec<(usize, String, f64)>| {
            if !current.is_empty() {
                let text = decode_entities(current);
                words.push((line_idx, text, current_start.unwrap_or(start_sec)));
                current.clear();
            }
            *current_start = None;
        };
        while !rest.is_empty() {
            if let Some(tag) = rest.strip_prefix('<') {
                let Some(close) = tag.find('>') else {
                    rest = "";
                    continue;
                };
                if let Some(at) = parse_timestamp(&tag[..close]) {
                    timed = true;
                    mark = at;
                }
                rest = &tag[close + 1..];
                continue;
            }
            let ch = rest.chars().next().expect("rest is not empty");
            rest = &rest[ch.len_utf8()..];
            if ch.is_whitespace() {
                push_word(&mut current, &mut current_start, &mut words);
                if !clean.ends_with(' ') && !clean.is_empty() {
                    clean.push(' ');
                }
            } else {
                if current.is_empty() {
                    current_start = Some(mark);
                }
                current.push(ch);
                clean.push(ch);
            }
        }
        push_word(&mut current, &mut current_start, &mut words);
        clean_lines.push(decode_entities(clean.trim()));
    }
    let text = clean_lines.join("\n");
    if !timed {
        let mut line = CaptionLine { text, words: Vec::new(), start_sec, end_sec, measured: false };
        auto_time_words(&mut line);
        return line;
    }
    let starts: Vec<f64> = words.iter().map(|w| w.2.clamp(start_sec, end_sec.max(start_sec))).collect();
    let words = words
        .into_iter()
        .enumerate()
        .map(|(i, (display_line_idx, text, _))| CaptionWord {
            text,
            start_sec: starts[i],
            end_sec: starts.get(i + 1).copied().unwrap_or(end_sec).max(starts[i]),
            display_line_idx,
        })
        .collect();
    CaptionLine { text, words, start_sec, end_sec, measured: true }
}

#[cfg(test)]
#[allow(clippy::arc_with_non_send_sync)]
mod tests {
    use super::*;
    use crate::{AssetLoader, DefaultAssetLoader};
    use skia_safe::textlayout::TypefaceFontProvider;
    use skia_safe::FontMgr;

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
                if let Some(tf) = FontMgr::new().new_from_bytes(&bytes, 0) {
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
                measured: false,
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
                measured: false,
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
        let layout = node.build_block_layout(400.0);
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
            CaptionLine { measured: false, text: "A".into(), words: vec![], start_sec: 0.0, end_sec: 5.0 },
            CaptionLine { measured: false, text: "B".into(), words: vec![], start_sec: 2.0, end_sec: 7.0 },
        ];
        assert_eq!(max_overlapping_lines(&lines), 2);
    }

    #[test]
    fn test_slot_assignments_stable() {
        let lines = vec![
            CaptionLine { measured: false, text: "A".into(), words: vec![], start_sec: 0.0, end_sec: 4.0 },
            CaptionLine { measured: false, text: "B".into(), words: vec![], start_sec: 2.0, end_sec: 7.0 },
            CaptionLine { measured: false, text: "C".into(), words: vec![], start_sec: 5.0, end_sec: 8.0 },
        ];
        let slots = CaptionNode::compute_slot_assignments(&lines);
        assert_eq!(slots[0], 0);
        assert_eq!(slots[1], 1);
        assert_eq!(slots[2], 0);
    }

    #[test]
    fn test_single_global_active_word() {
        let line_a = CaptionLine {
            measured: false,
            text: "alpha beta".into(),
            words: vec![
                CaptionWord { text: "alpha".into(), start_sec: 0.0, end_sec: 2.0, display_line_idx: 0 },
                CaptionWord { text: "beta".into(), start_sec: 2.0, end_sec: 4.0, display_line_idx: 0 },
            ],
            start_sec: 0.0,
            end_sec: 4.0,
        };
        let line_b = CaptionLine {
            measured: false,
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
            measured: false,
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
            measured: false,
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
            measured: false,
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
        let layout = node.build_block_layout(688.0);
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
            measured: false,
            text: "مرحبا بالعالم".into(),
            words: vec![],
            start_sec: 0.0,
            end_sec: 4.0,
        };
        auto_time_words(&mut line);
        let node = CaptionNode::new(vec![line], fc);
        let layout = node.build_block_layout(400.0);
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
    fn test_phrase_break_retimes_three_display_lines() {
        let mut line = CaptionLine {
            measured: false,
            text: "one two three four five six seven eight nine ten".into(),
            words: vec![],
            start_sec: 0.0,
            end_sec: 9.0,
        };
        auto_time_words(&mut line);
        let style = CaptionStyle::default();
        apply_phrase_break(&mut line, 4);
        retime_words_by_display_line(&mut line, &style);

        assert_eq!(display_line_count(&line), 3);
        // Line 0: [0, 3s), line 1: [3s, 6s), line 2: [6s, 9s)
        assert!(
            line.words[3].end_sec <= 3.0 + 0.01,
            "line 0 last word should end by 1/3 cue, got {:.2}",
            line.words[3].end_sec
        );
        assert!(
            line.words[4].start_sec >= 3.0 - 0.01,
            "line 1 first word should start at 1/3 cue, got {:.2}",
            line.words[4].start_sec
        );
        assert!(
            line.words[8].start_sec >= 6.0 - 0.01,
            "line 2 first word should start at 2/3 cue, got {:.2}",
            line.words[8].start_sec
        );
        assert!(
            line.words[8].end_sec <= 9.0 + 0.01,
            "line 2 last word should finish with cue, got {:.2}",
            line.words[8].end_sec
        );
    }

    #[test]
    fn test_phrase_break_retimes_per_display_line() {
        let mut line = CaptionLine {
            measured: false,
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
            measured: false,
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
    fn test_safe_area_preset_sets_frame_margins() {
        let style = CaptionStyle {
            safe_area_preset: Some("tiktok".to_string()),
            ..CaptionStyle::default()
        };
        let zone = safe_zone("tiktok").expect("tiktok preset");
        let resolved = style.resolved();
        assert!(resolved.bottom_margin > 100.0);
        assert_eq!(resolved.bottom_margin, zone.bottom);
        assert_eq!(resolved.top_margin, zone.top);
        assert_eq!(resolved.font_size, style.font_size, "presets move margins only");
    }

    #[test]
    fn test_pill_enabled_by_default() {
        let style = CaptionStyle::default();
        assert!(style.pill_enabled);
        assert!(style.auto_phrase_break);
        assert_eq!(style.max_words_per_line, 4);
        assert_eq!(style.phrase_line_mode, PhraseLineMode::Replace);
    }

    #[test]
    fn test_phrase_replace_reserves_single_line_height() {
        let fc = test_font_collection();
        let mut line = CaptionLine {
            measured: false,
            text: "one two three four five six seven eight nine ten".into(),
            words: vec![],
            start_sec: 0.0,
            end_sec: 9.0,
        };
        auto_time_words(&mut line);
        let mut node = CaptionNode::new(vec![line], fc);
        node.style.phrase_line_mode = PhraseLineMode::Replace;
        let layout = node.build_block_layout(688.0);
        let cue = &layout.cues[0];
        let max_line_h = cue
            .display_lines
            .iter()
            .map(|dl| dl.line_height)
            .fold(0.0f32, f32::max);
        let stacked_h: f32 = cue
            .display_lines
            .iter()
            .map(|dl| dl.line_height)
            .sum::<f32>()
            + (cue.display_lines.len().saturating_sub(1)) as f32 * node.style.line_spacing;
        assert!(stacked_h > max_line_h * 1.5);
        assert!((layout.total_height - max_line_h).abs() < 2.0);
    }

    #[test]
    fn test_legibility_score_high_contrast() {
        assert!(estimate_legibility(0.95, 0.1) > 0.8);
        assert!(estimate_legibility(0.2, 0.8) > 0.8);
    }

    #[test]
    fn test_free_position_block_top_is_centered() {
        let fc = test_font_collection();
        let mut line = CaptionLine {
            measured: false,
            text: "hello".into(),
            words: vec![],
            start_sec: 0.0,
            end_sec: 2.0,
        };
        auto_time_words(&mut line);
        let mut node = CaptionNode::new(vec![line], fc);
        node.style.position = CaptionPosition::Free(0.5);
        let layout = node.build_block_layout(688.0);
        assert!(layout.total_height > 0.0);

        let rect = Rect::from_xywh(0.0, 0.0, 1080.0, 1920.0);
        let top = CaptionNode::compute_cue_block_top_y(&node.style, rect, &layout);
        let expected_top = rect.top + rect.height() * 0.5 - layout.total_height / 2.0;
        assert!((top - expected_top).abs() < 0.01);
    }

    #[test]
    fn test_free_position_normalized_in_non_full_canvas_container() {
        let fc = test_font_collection();
        let mut line = CaptionLine {
            measured: false,
            text: "hello".into(),
            words: vec![],
            start_sec: 0.0,
            end_sec: 2.0,
        };
        auto_time_words(&mut line);
        let mut node = CaptionNode::new(vec![line], fc);
        node.style.position = CaptionPosition::Free(0.25);
        let layout = node.build_block_layout(688.0);
        assert!(layout.total_height > 0.0);

        // Container starts at y=200 and is 400 pixels tall.
        let rect = Rect::from_xywh(0.0, 200.0, 1080.0, 400.0);
        let top = CaptionNode::compute_cue_block_top_y(&node.style, rect, &layout);
        let expected_center_y = rect.top + rect.height() * 0.25;
        let expected_top = expected_center_y - layout.total_height / 2.0;
        assert!((top - expected_top).abs() < 0.01);
    }

    #[test]
    fn test_caption_position_corner_variants_round_trip() {
        for position in [
            CaptionPosition::TopLeft,
            CaptionPosition::TopRight,
            CaptionPosition::BottomLeft,
            CaptionPosition::BottomRight,
        ] {
            let serialized = serde_json::to_string(&position).expect("serialize");
            let deserialized: CaptionPosition =
                serde_json::from_str(&serialized).expect("deserialize");
            assert_eq!(deserialized, position, "{} round-trips", serialized);
        }
    }

    #[test]
    fn test_caption_position_free_round_trips() {
        let position = CaptionPosition::Free(0.5);
        let serialized = serde_json::to_string(&position).expect("serialize");
        assert_eq!(serialized, "\"free(0.5)\"");
        let deserialized: CaptionPosition = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(deserialized, position);
    }

    #[test]
    fn test_caption_position_free_xy_round_trips() {
        let position = CaptionPosition::FreeXY { x: 120.0, y: 340.0 };
        let serialized = serde_json::to_string(&position).expect("serialize");
        assert_eq!(serialized, "\"free_xy(120, 340)\"");
        let deserialized: CaptionPosition = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(deserialized, position);
    }

    #[test]
    fn test_free_position_measure_does_not_panic() {
        let fc = test_font_collection();
        let mut node = CaptionNode::new(Vec::new(), fc);
        node.style.position = CaptionPosition::Free(0.5);
        let measured = node.measure(
            Size {
                width: Some(1080.0),
                height: None,
            },
            Size {
                width: AvailableSpace::Definite(1080.0),
                height: AvailableSpace::Definite(1920.0),
            },
        );
        assert!(measured.height >= 0.0);
        assert!(matches!(node.style.position, CaptionPosition::Free(0.5)));
    }

    #[test]
    fn test_measure_keeps_anchor_margins_out_of_content_height() {
        let mut line = CaptionLine {
            measured: false,
            text: "caption content".into(),
            words: vec![],
            start_sec: 0.0,
            end_sec: 5.0,
        };
        auto_time_words(&mut line);
        let mut node = CaptionNode::new(vec![line], test_font_collection());
        let known = Size {
            width: Some(400.0),
            height: None,
        };
        let available = Size {
            width: AvailableSpace::Definite(400.0),
            height: AvailableSpace::Definite(1920.0),
        };
        let original = node.measure(known, available);
        assert!(original.height > 0.0);
        for position in [CaptionPosition::Top, CaptionPosition::Bottom] {
            for margin in [0.0, 110.0, 300.0] {
                node.style.top_margin = margin;
                node.style.bottom_margin = margin;
                node.set_position(position.clone());
                node.rebuild_layout_style();
                node.after_style_changed();
                assert_eq!(node.measure(known, available), original);
                let insets = node.layout_style().inset;
                let actual = if position == CaptionPosition::Top {
                    insets.top
                } else {
                    insets.bottom
                };
                assert_eq!(actual, taffy::style::LengthPercentageAuto::length(margin));
            }
        }
    }

    #[test]
    fn test_narrow_caption_measure_and_render_share_block_width() {
        let mut line = CaptionLine {
            measured: false,
            text: "narrow caption".into(),
            words: vec![],
            start_sec: 0.0,
            end_sec: 5.0,
        };
        auto_time_words(&mut line);
        let mut node = CaptionNode::new(vec![line], test_font_collection());
        node.update(0.5);
        for width in [10.0, 25.0, 400.0] {
            let measured = node.measure(
                Size {
                    width: Some(width),
                    height: None,
                },
                Size {
                    width: AvailableSpace::Definite(width),
                    height: AvailableSpace::Definite(1920.0),
                },
            );
            assert!(measured.height > 0.0);
            let measured_block_width = node.block_layout.lock().unwrap().as_ref().unwrap().width;
            let expected_width = (width * node.style.layout_width_fraction).max(20.0);
            assert_eq!(measured_block_width, expected_width);
            let mut surface = skia_safe::surfaces::raster_n32_premul((500, 500)).unwrap();
            node.render(
                surface.canvas(),
                Rect::from_xywh(0.0, 0.0, width, measured.height),
                1.0,
                &mut |_| {},
            )
            .unwrap();
            let guard = node.block_layout.lock().unwrap();
            let rendered = guard.as_ref().unwrap();
            assert_eq!(rendered.width, measured_block_width);
            assert_eq!(rendered.total_height, measured.height);
        }
    }

    #[test]
    fn test_measure_height_matches_block_layout() {
        let fc = test_font_collection();
        let long_text = "The quick brown fox jumps over the lazy dog repeatedly";
        let mut line = CaptionLine {
            measured: false,
            text: long_text.into(),
            words: vec![],
            start_sec: 0.0,
            end_sec: 5.0,
        };
        auto_time_words(&mut line);
        let node = CaptionNode::new(vec![line], fc);
        let canvas_width = 400.0f32;
        let canvas_h = 600.0f32;
        let layout_style = node.style.resolved();
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

        let layout = node.build_block_layout(block_width);
        // Insets position the box; they are not part of its content height.
        let expected_h = layout.total_height;
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

    // ── Measured word times ───────────────────────────────────────

    fn tw(text: &str, start: f64, end: f64) -> TimedWord {
        TimedWord { text: text.to_string(), start, end }
    }

    /// Eight words with uneven spacing, as speech recognition reports them.
    fn spoken() -> Vec<TimedWord> {
        vec![
            tw("Root", 0.32, 0.55),
            tw("beer", 0.56, 0.88),
            tw("is", 1.20, 1.31),
            tw("the", 1.33, 1.41),
            tw("best", 1.44, 1.97),
            tw("drink", 2.40, 2.71),
            tw("ever", 2.75, 3.02),
            tw("made", 3.10, 3.60),
        ]
    }

    fn starts(line: &CaptionLine) -> Vec<f64> {
        line.words.iter().map(|w| w.start_sec).collect()
    }

    #[test]
    fn measured_word_times_survive_phrase_break() {
        let mut lines = cues_from_words(&spoken(), &CueRules::default());
        assert_eq!(lines.len(), 1);
        assert!(lines[0].measured);
        let before = starts(&lines[0]);
        let style = CaptionStyle { max_words_per_line: 3, ..CaptionStyle::default() };
        refresh_caption_lines(&mut lines, &style);
        assert_eq!(starts(&lines[0]), before);
        let dl: Vec<usize> = lines[0].words.iter().map(|w| w.display_line_idx).collect();
        assert_eq!(dl, [0, 0, 0, 1, 1, 1, 2, 2]);
        // The node constructor runs the same refresh.
        let node = CaptionNode::new(lines.clone(), test_font_collection());
        assert_eq!(starts(&node.lines[0]), before);
    }

    #[test]
    fn estimated_cues_still_split_each_display_line_evenly() {
        let mut lines = parse_srt("1\n00:00:00,000 --> 00:00:02,000\none two three four five six\n");
        assert!(!lines[0].measured);
        refresh_caption_lines(&mut lines, &CaptionStyle { max_words_per_line: 3, ..CaptionStyle::default() });
        let first_of_line_1 = lines[0].words.iter().find(|w| w.display_line_idx == 1).unwrap();
        assert!(first_of_line_1.start_sec > 0.0);
    }

    #[test]
    fn cues_from_words_follow_the_asr_cue_rules() {
        let mut words = spoken();
        words[4].text = "best.".to_string();
        words.insert(0, tw("  ", 0.0, 0.1));
        let lines = cues_from_words(&words, &CueRules::default());
        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["Root beer is the best.", "drink ever made"]);
        assert_eq!(lines[0].end_sec, 2.40, "a short pause holds the cue until the next");
        assert_eq!(lines[1].start_sec, 2.40);
        assert!((lines[1].end_sec - 4.10).abs() < 1e-9, "the last cue holds 0.5 s");
        let ends = cues_from_words(&words, &CueRules { hold: 0.0, ..CueRules::default() });
        assert_eq!((ends[0].end_sec, ends[1].end_sec), (1.97, 3.60));
        let tight = CueRules { max_gap: 0.3, ..CueRules::default() };
        let texts: Vec<String> = cues_from_words(&spoken(), &tight).into_iter().map(|l| l.text).collect();
        assert_eq!(texts, ["Root beer", "is the best", "drink ever made"]);
    }

    #[test]
    fn a_cue_cut_for_length_carries_its_weak_tail() {
        let words: Vec<TimedWord> = "worth six hundred billion to the whole market"
            .split(' ')
            .enumerate()
            .map(|(i, w)| tw(w, i as f64 * 0.3, i as f64 * 0.3 + 0.3))
            .collect();
        let rules = CueRules { max_chars: 30, ..CueRules::default() };
        let lines = cues_from_words(&words, &rules);
        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["worth six hundred billion", "to the whole market"]);
        assert_eq!((lines[0].end_sec, lines[1].start_sec), (1.2, 1.2));
    }

    #[test]
    fn a_word_wider_than_the_column_shrinks_to_one_line() {
        let words = [tw("ON NOVEMBER 8TH,", 0.0, 1.0), tw("his", 1.0, 1.2), tw("own", 1.2, 1.4)];
        let mut node = CaptionNode::new(cues_from_words(&words, &CueRules::default()), test_font_collection());
        node.style.max_words_per_line = 2;
        node.after_style_changed();
        let wide = node.build_block_layout(4000.0);
        let narrow = node.build_block_layout(120.0);
        let (wide, narrow) = (&wide.cues[0].display_lines, &narrow.cues[0].display_lines);
        assert_eq!(narrow[0].tokens.len(), 1, "the long word has a line of its own");
        assert_eq!(narrow[0].line_height, wide[1].line_height, "and that line never wraps");
        let shown = narrow[0].line_width * narrow[0].fit;
        assert!(narrow[0].fit < 1.0 && shown <= 120.0 - 2.0 * node.style.outline() + 0.01, "{shown}");
        assert!(narrow[0].tokens[0].width > 0.9 * narrow[0].line_width, "the word measures whole");
        assert!(wide.iter().all(|l| l.fit == 1.0));
    }

    #[test]
    fn a_cue_cut_for_length_ends_at_its_last_clause() {
        let words: Vec<TimedWord> = "And 11 DAYS later, the man who caused it got his job back."
            .split(' ')
            .enumerate()
            .map(|(i, w)| tw(w, i as f64 * 0.3, i as f64 * 0.3 + 0.3))
            .collect();
        let texts: Vec<String> = cues_from_words(&words, &CueRules::default()).into_iter().map(|l| l.text).collect();
        assert_eq!(texts, ["And 11 DAYS later,", "the man who caused it got his job back."]);
    }

    #[test]
    fn transcript_json_reads_the_asr_object_and_a_bare_array() {
        let asr = "\u{feff}{\"model\":\"bark-3.0\",\"words\":[{\"text\":\"Hi\",\"start\":0.25,\"end\":0.5},{\"text\":\"all.\",\"start\":0.55,\"end\":0.9}]}";
        assert_eq!(parse_transcript_words(asr).unwrap(), [tw("Hi", 0.25, 0.5), tw("all.", 0.55, 0.9)]);
        let array = "[{\"word\":\"Hi\",\"start\":0.25,\"end\":0.5}]";
        assert_eq!(parse_transcript_words(array).unwrap(), [tw("Hi", 0.25, 0.5)]);
        assert!(parse_transcript_words("[{\"text\":\"x\",\"start\":1.0,\"end\":0.5}]").is_err());
        assert!(parse_transcript_words("[{\"text\":\"x\",\"start\":-1.0,\"end\":0.5}]").is_err());
        assert!(parse_transcript_words("{\"cues\":[]}").is_err());
    }

    #[test]
    fn vtt_inline_timestamps_set_each_word_start() {
        let vtt = "WEBVTT\r\n\r\n00:00:00.320 --> 00:00:01.200 align:center line:90%\r\nFish <00:00:00.640>&amp; <00:00:00.880>chips.\r\n\r\nnext-cue\r\n00:02.400 --> 00:02.700\r\n<v Sam>Next</v>\r\n";
        let lines = parse_vtt(vtt);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].measured);
        assert_eq!(lines[0].text, "Fish & chips.");
        let times: Vec<(f64, f64)> = lines[0].words.iter().map(|w| (w.start_sec, w.end_sec)).collect();
        assert_eq!(times, [(0.32, 0.64), (0.64, 0.88), (0.88, 1.2)]);
        assert_eq!(lines[1].text, "Next");
        assert_eq!((lines[1].start_sec, lines[1].end_sec), (2.4, 2.7));
    }

    #[test]
    fn director_asr_vtt_output_round_trips_its_word_starts() {
        // The text `director-engine asr transcribe --format vtt` writes.
        let vtt = "WEBVTT\n\n00:00:00.320 --> 00:00:01.970\nRoot <00:00:00.560>beer <00:00:01.200>is <00:00:01.330>the <00:00:01.440>best\n\n";
        let mut lines = parse_vtt(vtt);
        refresh_caption_lines(&mut lines, &CaptionStyle { max_words_per_line: 2, ..CaptionStyle::default() });
        assert_eq!(starts(&lines[0]), [0.32, 0.56, 1.2, 1.33, 1.44]);
    }

    #[test]
    fn srt_with_crlf_bom_and_no_index_reads() {
        let srt = "\u{feff}00:00:01,000 --> 00:00:02,000\r\nHello there\r\n\r\n2\r\n00:00:03,000 --> 00:00:04,500\r\nSecond line\r\nwraps\r\n";
        let lines = parse_srt(srt);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "Hello there");
        assert_eq!(lines[1].text, "Second line\nwraps");
        assert_eq!((lines[1].start_sec, lines[1].end_sec), (3.0, 4.5));
        assert!(lines.iter().all(|l| !l.measured));
    }

    #[test]
    fn measured_lines_reveal_by_their_first_word() {
        let mut lines = cues_from_words(&spoken(), &CueRules::default());
        let style = CaptionStyle { max_words_per_line: 3, ..CaptionStyle::default() };
        refresh_caption_lines(&mut lines, &style);
        let cue = &lines[0];
        assert_eq!(line_reveal_factor(cue, 0, 0.32, &style), 1.0);
        assert_eq!(line_reveal_factor(cue, 0, 0.30, &style), 0.0);
        // Line 1 starts with "the" at 1.33 and line 2 with "ever" at 2.75.
        assert_eq!(line_reveal_factor(cue, 1, 1.33, &style), 1.0);
        assert!(line_reveal_factor(cue, 1, 1.30, &style) < 1.0);
        assert_eq!(line_reveal_factor(cue, 1, 1.0, &style), 0.0);
        assert_eq!(line_reveal_factor(cue, 2, 2.75, &style), 1.0);
    }

    #[test]
    fn every_highlight_lands_within_one_frame_of_its_word() {
        let mut lines = cues_from_words(&spoken(), &CueRules { max_chars: 14, ..CueRules::default() });
        assert!(lines.len() > 1);
        refresh_caption_lines(&mut lines, &CaptionStyle { max_words_per_line: 2, ..CaptionStyle::default() });
        let fps = 30.0;
        let frame = 1.0 / fps;
        for (ci, line) in lines.iter().enumerate() {
            for (wi, word) in line.words.iter().enumerate() {
                let first = (0..200)
                    .map(|k| k as f64 / fps)
                    .find(|&t| active_word_at(&lines, t) == Some((ci, wi)))
                    .unwrap_or_else(|| panic!("{:?} is never highlighted", word.text));
                assert!(
                    first >= word.start_sec - 1e-9 && first - word.start_sec < frame,
                    "{:?} starts at {} and lights at {first}",
                    word.text,
                    word.start_sec
                );
            }
        }
    }

    #[test]
    fn pages_follow_the_display_lines_and_max_lines() {
        let lines = cues_from_words(&spoken(), &CueRules::default());
        let mut node = CaptionNode::new(lines, test_font_collection());
        node.style.max_words_per_line = 3;
        node.style.line_break = LineBreak::Words;
        node.style.max_lines = 1;
        node.after_style_changed();
        let pages = node.pages();
        let texts: Vec<&str> = pages.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(texts, ["Root beer is", "the best drink", "ever made"]);
        assert_eq!((pages[0].start, pages[0].end), (0.32, 1.33));
        assert!((pages[2].end - 4.10).abs() < 1e-9, "the last page holds with its cue");
        node.style.max_lines = 2;
        node.after_style_changed();
        let texts: Vec<String> = node.pages().into_iter().map(|p| p.text).collect();
        assert_eq!(texts, ["Root beer is the best drink", "ever made"]);
    }

    #[test]
    fn highlight_color_has_no_lead_by_default() {
        let lines = cues_from_words(&spoken(), &CueRules::default());
        let node = CaptionNode::new(lines, test_font_collection());
        let word = &node.lines[0].words[1];
        assert_eq!(node.compute_word_color(word, word.start_sec - 0.05, false), node.style.color);
    }
}
