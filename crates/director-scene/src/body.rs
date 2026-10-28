//! Kind-specific node fields.

use crate::caption::{ActiveSpeakerCameraBody, CaptionBody, SmartCaptionBody};
use crate::channel::{Channel, Rgba};
use crate::effect::Effect;
use crate::html::{HtmlBody, HtmlCaptureBody, HtmlVideoBody, SvgSmilBody};
use crate::{AssetId, SceneDocument};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Fields that depend on the node kind. The tag matches `NodeRecord::kind`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeBody {
    Box(BoxBody),
    Text(TextBody),
    Image(MediaBody),
    Video(MediaBody),
    Shape(ShapeBody),
    Vector(VectorBody),
    Lottie(LottieBody),
    Composition(CompositionBody),
    Effect(EffectBody),
    Caption(CaptionBody),
    SmartCaption(SmartCaptionBody),
    ActiveSpeakerCamera(ActiveSpeakerCameraBody),
    SpectrumVisualizer(SpectrumBody),
    Html(Box<HtmlBody>),
    HtmlVideo(HtmlVideoBody),
    HtmlCapture(HtmlCaptureBody),
    SvgSmil(SvgSmilBody),
}

impl NodeBody {
    /// The `NodeRecord::kind` tag this body serializes under.
    pub fn kind(&self) -> &'static str {
        match self {
            NodeBody::Box(_) => "box",
            NodeBody::Text(_) => "text",
            NodeBody::Image(_) => "image",
            NodeBody::Video(_) => "video",
            NodeBody::Shape(_) => "shape",
            NodeBody::Vector(_) => "vector",
            NodeBody::Lottie(_) => "lottie",
            NodeBody::Composition(_) => "composition",
            NodeBody::Effect(_) => "effect",
            NodeBody::Caption(_) => "caption",
            NodeBody::SmartCaption(_) => "smart_caption",
            NodeBody::ActiveSpeakerCamera(_) => "active_speaker_camera",
            NodeBody::SpectrumVisualizer(_) => "spectrum_visualizer",
            NodeBody::Html(_) => "html",
            NodeBody::HtmlVideo(_) => "html_video",
            NodeBody::HtmlCapture(_) => "html_capture",
            NodeBody::SvgSmil(_) => "svg_smil",
        }
    }
}

/// A painted rectangle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BoxBody {
    pub background: Option<Channel<Rgba>>,
    pub background_gradient: Option<Gradient>,
    /// Layer blur sigma.
    pub blur: Channel,
    pub shadow_color: Option<Channel<Rgba>>,
    pub shadow_blur: Channel,
    pub shadow_offset_x: Channel,
    pub shadow_offset_y: Channel,
    pub border_radius: Channel,
    pub border_width: Channel,
    pub border_color: Option<Channel<Rgba>>,
    /// `visible` or `hidden`.
    pub overflow: String,
    /// Blur of the content behind the box.
    pub backdrop_blur: Channel,
    /// Shadows painted after the main one.
    pub extra_shadows: Vec<BoxShadow>,
    /// Clip polygon in fractions of the box size.
    pub clip_path: Option<Vec<[f32; 2]>>,
}

/// One stacked box shadow.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BoxShadow {
    pub color: Channel<Rgba>,
    pub blur: Channel,
    pub offset_x: Channel,
    pub offset_y: Channel,
}

/// A color gradient in fractions of the painted box.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Gradient {
    pub kind: GradientKind,
    pub colors: Vec<Rgba>,
    /// Stop positions in `0.0..=1.0`. Absent spaces the colors evenly.
    pub positions: Option<Vec<f32>>,
    /// Linear start point.
    pub start: [f32; 2],
    /// Linear end point.
    pub end: [f32; 2],
    /// Radial and conic center.
    pub center: [f32; 2],
    /// Radial radius as a fraction of the shorter side.
    pub radius: f32,
    /// Conic start angle in degrees.
    pub angle: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GradientKind {
    Linear,
    Radial,
    Conic,
}

/// A paragraph of styled spans.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextBody {
    pub spans: Vec<TextSpan>,
    /// Size for spans without their own.
    pub font_size: Channel,
    /// Color for spans without their own.
    pub color: Channel<Rgba>,
    pub fit: TextFit,
    /// Smallest size the fit may choose.
    pub min_size: f32,
    /// Largest size the fit may choose.
    pub max_size: f32,
    /// Shadows in CSS order. The first paints on top.
    pub shadows: Vec<TextShadow>,
    pub align: TextAlign,
    pub direction: TextDirection,
    /// Paint the span stroke over the fill.
    pub stroke_over_fill: bool,
    /// Per-glyph animators.
    pub animators: Vec<TextAnimator>,
    /// Number ticker. The first span shows `counter_value` formatted this way.
    pub counter: Option<Counter>,
    pub counter_value: Channel,
}

/// One run of text with its own style.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextSpan {
    pub text: String,
    pub color: Option<Rgba>,
    pub font_family: Option<String>,
    pub font_weight: Option<u16>,
    /// `normal` or `italic`.
    pub font_style: Option<String>,
    pub font_size: Option<f32>,
    pub background_color: Option<Rgba>,
    pub background_padding: Option<f32>,
    pub stroke_width: Option<f32>,
    pub stroke_color: Option<Rgba>,
    pub fill_gradient: Option<Gradient>,
    /// Extra advance between letters in pixels.
    pub letter_spacing: Option<f32>,
    /// Extra advance after each space in pixels.
    pub word_spacing: Option<f32>,
    /// Line height as a multiple of the font size.
    pub line_height: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextFit {
    None,
    Shrink,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    Left,
    Right,
    Center,
    Justify,
    Start,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextDirection {
    Auto,
    Ltr,
    Rtl,
}

/// A text drop shadow.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextShadow {
    pub color: Rgba,
    pub blur: f32,
    pub offset: [f32; 2],
}

/// Animates one property across a range of glyphs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextAnimator {
    /// First glyph, inclusive.
    pub start: usize,
    /// Last glyph, exclusive.
    pub end: usize,
    pub property: GlyphProperty,
    pub animation: Channel,
    /// Delay between glyph starts in seconds.
    pub stagger: f32,
    /// Duration of one glyph's animation in seconds.
    pub duration: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GlyphProperty {
    Opacity,
    OffsetX,
    OffsetY,
    Scale,
    Rotation,
}

/// Number ticker formatting.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Counter {
    pub decimals: usize,
    pub prefix: String,
    pub suffix: String,
    pub thousands_separator: bool,
}

/// An image or a video frame placed in the node box.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MediaBody {
    pub asset: AssetId,
    pub fit: ObjectFit,
    pub position: ObjectPosition,
    pub border_radius: Channel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObjectFit {
    Cover,
    Contain,
    Fill,
    None,
    ScaleDown,
}

/// `object-position` as a fraction of the leftover space plus a pixel offset.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectPosition {
    pub x_percent: f32,
    pub x_offset: f32,
    pub y_percent: f32,
    pub y_offset: f32,
}

/// A filled and stroked geometric shape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ShapeBody {
    pub geometry: ShapeGeometry,
    pub fill: Option<Channel<Rgba>>,
    pub fill_gradient: Option<Gradient>,
    pub stroke: Option<Channel<Rgba>>,
    pub stroke_width: Channel,
}

/// Shape geometry in node pixels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShapeGeometry {
    Circle { cx: f32, cy: f32, r: f32 },
    Ellipse { cx: f32, cy: f32, rx: f32, ry: f32 },
    Path { commands: Vec<PathCommand> },
}

/// One path segment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PathCommand {
    MoveTo { x: f32, y: f32 },
    LineTo { x: f32, y: f32 },
    QuadTo { cx: f32, cy: f32, x: f32, y: f32 },
    CubicTo { c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32 },
    ArcTo { cx: f32, cy: f32, rx: f32, ry: f32, start_angle: f32, sweep_angle: f32 },
    Close,
}

/// An SVG drawing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VectorBody {
    /// SVG text written from the parsed tree.
    pub asset: AssetId,
}

/// A Lottie animation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LottieBody {
    /// The parsed Lottie JSON.
    pub animation: serde_json::Value,
    /// Frame channel in Lottie frames.
    pub frame: Channel,
    pub speed: f32,
    pub loop_animation: bool,
    /// Images that replace the animation's image assets, by Lottie asset id.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub images: std::collections::BTreeMap<String, AssetId>,
}

/// A nested composition with its own canvas, timeline and nodes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CompositionBody {
    /// Parent time minus this offset is the composition time.
    pub start_offset: f64,
    pub document: std::boxed::Box<SceneDocument>,
}

/// A stack of effects applied to the node's children.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EffectBody {
    pub effects: Vec<Effect>,
}

/// Audio spectrum bars.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpectrumBody {
    /// Audio track slot.
    pub track: usize,
    pub bar_count: usize,
    pub bar_color: Rgba,
    pub bar_gap: f32,
    /// `0.0` responds at once, `0.9` is very smooth.
    pub smoothing: f32,
    /// `bottom`, `top` or `center`.
    pub position: String,
    /// Fractions of the node height.
    pub min_height: f32,
    pub max_height: f32,
    pub border_radius: f32,
}
