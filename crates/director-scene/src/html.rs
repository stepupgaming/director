//! Bodies of the elements `director-html` compiles: styled HTML boxes, HTML
//! videos, sampled browser captures and SMIL-animated SVG.

use crate::body::{BoxBody, MediaBody, NodeBody, ObjectPosition, TextAlign, TextShadow};
use crate::channel::{Channel, Easing, Rgba};
use crate::layout::Length;
use crate::{AssetId, NodeRecord, NodeRef};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One styled HTML box. Its layout is the node record layout before CSS animations apply.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HtmlBody {
    /// Paint before CSS animations apply.
    pub paint: HtmlPaint,
    /// The text or replaced element painted in the content box.
    pub content: Option<Box<HtmlContent>>,
    pub animations: Vec<CssAnimation>,
    /// Visible from this second.
    pub start: f64,
    /// Hidden from this second. Absent means never.
    pub end: Option<f64>,
    /// Media time at `start`.
    pub media_start: f64,
    pub playback_rate: f64,
    pub hidden: bool,
    /// Box and font size the relative values resolved against.
    pub metrics: HtmlMetrics,
    /// A Paper shader painted in place of the background.
    pub paper: Option<PaperShader>,
    /// This box's part in an inline formatting context with atomic inline boxes.
    pub inline: Option<InlineFlow>,
}

/// Atomic inline boxes (`inline-block`) laid out on the lines of a sibling text
/// paragraph. Boxes and the text that places them share one `flow` number.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InlineFlow {
    /// The text holds U+FFFC once per box, in box order, sized by `boxes`.
    Text { flow: u32, boxes: Vec<InlinePlaceholder> },
    /// The box sits on placeholder `slot` of the text.
    Box { flow: u32, slot: u32 },
}

/// One atomic inline box as its text line sees it, margins included, in px.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InlinePlaceholder {
    pub width: f32,
    pub height: f32,
    /// Distance from the top to the baseline.
    pub baseline: f32,
}

/// Content an HTML box paints inside itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HtmlContent {
    pub opacity: Channel,
    pub body: NodeBody,
}

/// Box and font size in px.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HtmlMetrics {
    pub width: f32,
    pub height: f32,
    pub font_size: f32,
}

/// Computed CSS paint of one HTML box. Its opacity is the node opacity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HtmlPaint {
    /// Background color, border width and color, radius, clip and shadow.
    pub box_paint: BoxBody,
    pub visible: bool,
    /// CSS `transform` text.
    pub transform: String,
    pub origin: [Length; 2],
    /// Absolute z of `transform-origin` in px.
    pub origin_z: f32,
    pub color: Rgba,
    pub font_size: f32,
    pub font_family: String,
    pub font_weight: u16,
    pub font_style: String,
    pub letter_spacing: f32,
    #[serde(default)]
    pub word_spacing: f32,
    /// First-line indent. A percent is a fraction of the text box width.
    pub text_indent: Length,
    pub tabular_nums: bool,
    pub line_height: Option<f32>,
    pub align: TextAlign,
    pub preserve_space: bool,
    pub no_wrap: bool,
    pub pre_line: bool,
    pub overflow_wrap: OverflowWrap,
    pub break_word: bool,
    pub break_all: bool,
    pub text_overflow: TextOverflow,
    /// Absent is `none`.
    pub line_clamp: Option<u32>,
    pub text_transform: TextTransform,
    pub rtl: bool,
    pub z_index: i32,
    /// Background layers, topmost first.
    pub layers: Vec<BackgroundLayer>,
    /// Empty lists use the initial value. A shorter list cycles by layer index.
    pub background_position_x: Vec<AxisPosition>,
    pub background_position_y: Vec<AxisPosition>,
    pub background_size: Vec<BackgroundSize>,
    /// Horizontal and vertical repeat per layer.
    pub background_repeat: Vec<[BackgroundRepeat; 2]>,
    pub background_origin: BackgroundOrigin,
    pub background_clip: BackgroundClip,
    pub mask: Option<CssGradient>,
    /// The mask reads luminance. Otherwise it reads alpha.
    pub mask_luminance: bool,
    pub decoration: TextLines,
    pub decoration_style: DecorationStyle,
    pub decoration_thickness: DecorationThickness,
    /// Absent is `currentColor`.
    pub decoration_color: Option<Rgba>,
    pub text_underline_offset: f32,
    pub object_position: ObjectPosition,
    /// Horizontal and vertical radii, top-left first, clockwise.
    pub corners: [[Length; 2]; 4],
    /// Top, right, bottom, left.
    pub border_colors: [Rgba; 4],
    /// Top, right, bottom, left.
    pub border_lines: [BorderLine; 4],
    pub perspective: Option<f32>,
    pub perspective_origin: [Length; 2],
    pub backface_hidden: bool,
    pub preserve_3d: bool,
    /// CSS `filter` functions in source order.
    pub filters: Vec<CssFilter>,
    /// `mix-blend-mode` as a node record blend name.
    pub blend_mode: String,
    pub shadows: Vec<HtmlShadow>,
    /// The first shadow paints on top.
    pub text_shadows: Vec<TextShadow>,
    pub text_stroke_width: f32,
    /// Absent is `currentColor`.
    pub text_stroke_color: Option<Rgba>,
    /// The stroke paints over the fill.
    pub stroke_over_fill: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OverflowWrap {
    Normal,
    BreakWord,
    Anywhere,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextOverflow {
    Clip,
    Ellipsis,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextTransform {
    None,
    Uppercase,
    Lowercase,
    Capitalize,
}

/// One background layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundLayer {
    Gradient(CssGradient),
    Image { asset: AssetId },
}

/// A background position on one axis: `percent` of the free space plus `length` px.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AxisPosition {
    pub percent: f32,
    pub length: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundSize {
    Auto,
    Cover,
    Contain,
    /// Absent axes are `auto`.
    Lengths { width: Option<Length>, height: Option<Length> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundRepeat {
    Repeat,
    NoRepeat,
    Space,
    Round,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundOrigin {
    PaddingBox,
    BorderBox,
    ContentBox,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundClip {
    BorderBox,
    PaddingBox,
    ContentBox,
    Text,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextLines {
    pub underline: bool,
    pub overline: bool,
    pub line_through: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecorationStyle {
    Solid,
    Double,
    Dotted,
    Dashed,
    Wavy,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecorationThickness {
    Auto,
    Px(f32),
    FromFont,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BorderLine {
    Solid,
    Dashed,
    Dotted,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

/// One `box-shadow` entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HtmlShadow {
    pub inset: bool,
    pub color: Rgba,
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub spread: f32,
}

/// One CSS filter function with its amount resolved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CssFilter {
    /// Gaussian sigma in px.
    Blur { radius: f32 },
    Brightness { amount: f32 },
    Contrast { amount: f32 },
    Saturate { amount: f32 },
    /// Clamped to `0.0..=1.0`.
    Grayscale { amount: f32 },
    HueRotate { degrees: f32 },
    DropShadow { x: f32, y: f32, blur: f32, color: Rgba },
    Invert { amount: f32 },
    Sepia { amount: f32 },
    Opacity { amount: f32 },
}

/// A CSS gradient with lengths resolved to px or percent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CssGradient {
    pub shape: GradientShape,
    pub repeating: bool,
    /// One entry per item. Absent is a transition hint.
    pub colors: Vec<Option<Rgba>>,
    /// One entry per item. Absent positions spread evenly.
    pub stops: Vec<Option<GradientStop>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GradientShape {
    Linear { direction: LinearDirection },
    Radial { center: [Length; 2], radius: RadialSize },
    /// `angle` in degrees.
    Conic { center: [Length; 2], angle: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LinearDirection {
    /// Degrees clockwise from up.
    Angle(f32),
    /// `to <corner>`. The angle depends on the painted box.
    Corner { right: bool, bottom: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RadialSize {
    CircleExtent(ShapeExtent),
    EllipseExtent(ShapeExtent),
    /// Radius in px.
    Circle(f32),
    Ellipse { x: Length, y: Length },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShapeExtent {
    ClosestSide,
    FarthestSide,
    ClosestCorner,
    FarthestCorner,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GradientStop {
    Fixed(Length),
    /// A `calc()` mixing px and percent, as CSS text.
    Calc(String),
}

/// One CSS animation from `animation-*` and its `@keyframes`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CssAnimation {
    pub delay: f64,
    pub duration: f64,
    /// Absent is `infinite`.
    pub iteration_count: Option<f64>,
    /// `normal`, `reverse`, `alternate` or `alternate-reverse`.
    pub direction: String,
    /// `none`, `forwards`, `backwards` or `both`.
    pub fill_mode: String,
    pub channels: Vec<CssChannel>,
}

/// The keyframes of one animated property.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CssChannel {
    /// CSS property name.
    pub property: String,
    /// Keyframes in offset order.
    pub keyframes: Vec<CssKeyframe>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CssKeyframe {
    /// Fraction of one iteration.
    pub offset: f32,
    pub value: CssValue,
    /// Timing toward the next keyframe.
    pub timing: CssTiming,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CssValue {
    Scalar(f32),
    Color(Rgba),
    /// CSS `transform` text.
    Transform(String),
    /// Text content for the `text` property. It changes at each keyframe.
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CssTiming {
    Curve(Easing),
    /// `steps(count, jump-start)` when `start`, else `jump-end`.
    Steps { count: u32, start: bool },
    /// A GSAP 3 ease by name, such as `power3.out` or `back.out(1.7)`.
    Gsap(String),
}

/// A Paper shader and its inputs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaperShader {
    pub sksl: String,
    pub uniforms: BTreeMap<String, Vec<f32>>,
    pub images: BTreeMap<String, PaperImage>,
}

/// A shader image as raw RGBA8 bytes in the asset table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaperImage {
    pub width: i32,
    pub height: i32,
    pub asset: AssetId,
    /// Row 0 is the visual top.
    pub flip_y: bool,
}

/// An HTML `<video>`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HtmlVideoBody {
    pub video: MediaBody,
    /// Playback wraps at `duration`.
    pub looped: bool,
    /// Media duration in seconds.
    pub duration: f64,
}

/// A browser capture sampled into scene graphs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HtmlCaptureBody {
    /// Samples per second.
    pub sample_rate: f64,
    /// Distinct sampled graphs.
    pub graphs: Vec<CaptureGraph>,
    /// The `graphs` index of each sample, in time order.
    pub frames: Vec<usize>,
}

/// One sampled scene graph.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CaptureGraph {
    pub nodes: Vec<NodeRecord>,
    pub root: NodeRef,
    /// Layout rectangle `[x, y, width, height]` of each node, in `nodes` order.
    pub rects: Vec<[f32; 4]>,
}

/// An inline SVG with SMIL animations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SvgSmilBody {
    /// SVG markup without its animation elements. Targets carry `director-smil-id`.
    pub markup: AssetId,
    pub animations: Vec<SmilAnimation>,
}

/// One SMIL animation element.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SmilAnimation {
    /// The `director-smil-id` of the target.
    pub target: u32,
    pub begin: f64,
    pub dur: f64,
    pub end: Option<f64>,
    pub repeat: SmilRepeat,
    pub repeat_dur: Option<f64>,
    /// `fill="freeze"`.
    pub freeze: bool,
    pub effect: SmilEffect,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SmilRepeat {
    Once,
    Count(f64),
    Forever,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SmilCalcMode {
    Linear,
    Discrete,
    Paced,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SmilEffect {
    /// `<animate>` or `<set>`.
    Attribute {
        name: String,
        mode: SmilCalcMode,
        values: Vec<SmilValue>,
        key_times: Option<Vec<f64>>,
        /// `<set>` holds its value.
        hold: bool,
    },
    /// `<animateTransform>`.
    Transform {
        /// `translate`, `scale`, `rotate`, `skewX` or `skewY`.
        kind: String,
        mode: SmilCalcMode,
        frames: Vec<Vec<f64>>,
        key_times: Option<Vec<f64>>,
    },
    /// `<animateMotion>` along sampled points.
    Motion {
        points: Vec<[f32; 2]>,
        /// Fixed rotation in degrees.
        rotate: f32,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SmilValue {
    Number { value: f64, unit: String },
    /// Components in `0.0..=1.0`.
    Color(Rgba),
    Raw(String),
}
