//! Caption, smart caption and active speaker camera bodies.

use crate::body::{MediaBody, TextAlign, TextDirection};
use crate::channel::Rgba;
use crate::entity::EntityId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Timed caption lines with a style.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CaptionBody {
    /// Cues in source order.
    pub lines: Vec<CaptionLine>,
    pub style: CaptionStyle,
}

/// One timed caption cue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CaptionLine {
    pub text: String,
    pub words: Vec<CaptionWord>,
    pub start_sec: f64,
    pub end_sec: f64,
}

/// One timed word within a cue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CaptionWord {
    pub text: String,
    pub start_sec: f64,
    pub end_sec: f64,
    /// Zero-based display line within the cue.
    pub display_line_idx: usize,
}

/// Where the caption block sits inside its layout rect.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CaptionPosition {
    Bottom,
    Top,
    Center,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    /// Normalized vertical offset of the block center in `0.0..=1.0`.
    Free { y: f32 },
    /// Absolute canvas coordinate of the block center.
    #[serde(rename = "free_xy")]
    FreeXY { x: f32, y: f32 },
}

/// How phrase-broken display lines appear within one cue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PhraseLineMode {
    /// One display line at a time in the same place.
    Replace,
    /// All revealed display lines stack vertically.
    Stack,
}

/// Caption typography, colors, motion and legibility settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CaptionStyle {
    pub font_size: f32,
    /// Color of future words.
    pub color: Rgba,
    /// Color of the active word.
    pub highlight_color: Rgba,
    /// Color of past words.
    pub spoken_color: Rgba,
    pub position: CaptionPosition,
    pub text_align: TextAlign,
    /// Margin used by the left anchors.
    pub left_margin: f32,
    /// Margin used by the right anchors.
    pub right_margin: f32,
    pub text_direction: TextDirection,
    pub font_family: String,
    /// 400 is regular, 700 is bold.
    pub font_weight: u16,
    pub bottom_margin: f32,
    pub top_margin: f32,
    pub line_spacing: f32,
    /// Paragraph width as a fraction of the available width.
    pub layout_width_fraction: f32,
    pub active_opacity: f32,
    pub future_opacity: f32,
    /// Alpha multiplier for the spoken color.
    pub spoken_color_opacity: f32,
    /// Seconds of the future to active color ramp.
    pub color_transition_duration: f64,
    pub active_pop_scale: f32,
    /// Seconds of the active word scale pop.
    pub active_pop_duration: f64,
    pub text_shadow_enabled: bool,
    pub text_shadow_color: Rgba,
    pub text_shadow_offset_x: f32,
    pub text_shadow_offset_y: f32,
    pub text_shadow_blur: f32,
    pub auto_adapt_shadow: bool,
    pub stroke_enabled: bool,
    pub stroke_width: f32,
    pub stroke_color: Rgba,
    pub glow_enabled: bool,
    pub glow_blur: f32,
    /// Reveal start of each display line as a fraction of the cue duration.
    pub line_reveal_offsets: Vec<f32>,
    /// Fade-in seconds of each display line.
    pub line_reveal_durations: Vec<f32>,
    pub pill_enabled: bool,
    pub pill_color: Rgba,
    pub pill_adaptive: bool,
    pub pill_padding_h: f32,
    pub pill_padding_v: f32,
    pub pill_radius: f32,
    pub auto_phrase_break: bool,
    pub max_words_per_line: usize,
    pub phrase_line_mode: PhraseLineMode,
    /// Platform preset such as `tiktok` or `youtube_shorts`.
    pub safe_area_preset: Option<String>,
    pub scale_with_canvas: bool,
    pub reference_canvas_height: f32,
    pub uppercase: bool,
}

/// Captions placed around semantic entities.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SmartCaptionBody {
    /// The wrapped caption. Its opacity is the node opacity.
    pub caption: CaptionBody,
    pub trajectory: CaptionTrajectory,
    /// Canvas width the trajectory was solved for.
    pub canvas_w: f32,
    /// Canvas height the trajectory was solved for.
    pub canvas_h: f32,
    /// The trajectory came from the continuous solver and applies as absolute positions.
    pub continuous: bool,
    pub manifest: Option<SemanticManifest>,
    pub config: Option<SmartCaptionConfig>,
}

/// Sampled caption positions over time, in time order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CaptionTrajectory {
    pub samples: Vec<TrajectorySample>,
}

/// A caption position at one time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TrajectorySample {
    pub time: f64,
    pub x: f32,
    pub y: f32,
    /// The anchor the discrete solver chose. Absent for the continuous solver.
    pub anchor: Option<CaptionAnchor>,
}

/// A candidate caption anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CaptionAnchor {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
    Left,
    Right,
}

/// The entities and solver settings a shot's caption placement uses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SemanticManifest {
    pub entity_refs: Vec<EntityId>,
    pub start_time: f64,
    pub end_time: f64,
    pub solver: SolverConfig,
}

/// Cost weights and grid settings of the caption placement solver.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SolverConfig {
    /// Smallest gap between caption and face in pixels.
    pub clearance: f32,
    /// Smoothing window in samples.
    pub smoothing_window: usize,
    pub motion_penalty: f32,
    pub overlap_penalty: f32,
    pub safe_zone_penalty: f32,
    /// Grid steps per axis for the continuous solver.
    pub grid_steps: usize,
    pub negative_space_weight: f32,
    pub smoothness_weight: f32,
}

/// Canvas, caption size, safe zone and sampling used to solve a trajectory.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SmartCaptionConfig {
    pub canvas_w: f32,
    pub canvas_h: f32,
    /// Expected caption block width in pixels.
    pub caption_w: f32,
    /// Expected caption block height in pixels.
    pub caption_h: f32,
    pub safe_top: f32,
    pub safe_bottom: f32,
    pub safe_left: f32,
    pub safe_right: f32,
    /// Samples per second.
    pub sample_rate: f64,
    pub continuous: bool,
}

/// A video that reframes onto the active speaker.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ActiveSpeakerCameraBody {
    /// The wrapped video. Its opacity is the node opacity.
    pub video: MediaBody,
    pub trajectory: CameraTrajectory,
    pub groups: Vec<ContinuityGroup>,
}

/// Sampled crop centers and heights over time, in time order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CameraTrajectory {
    pub samples: Vec<CameraSample>,
}

/// A crop rectangle at one time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CameraSample {
    pub time: f64,
    /// Crop center x in source pixels.
    pub center_x: f32,
    /// Crop center y in source pixels.
    pub center_y: f32,
    /// Crop height in source pixels.
    pub height: f32,
}

/// A time range that follows one entity with one framing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ContinuityGroup {
    /// Start in seconds, inclusive.
    pub start_time: f64,
    /// End in seconds, exclusive.
    pub end_time: f64,
    pub target_id: EntityId,
    pub framing: Framing,
    /// Face tracks whose union frames the shot when present.
    pub face_ids: Vec<EntityId>,
}

/// How tightly the camera frames the subject.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Framing {
    CloseUp,
    Medium,
    Wide,
    /// Crop height as a multiple of the tracked box height.
    Custom { multiplier: f32 },
}
