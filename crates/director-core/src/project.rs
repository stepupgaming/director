//! Project (de)serialization (Critical blocker #2 foundation).
//!
//! Versioned, agent-friendly JSON save/load of a `Director` so agents can
//! `render -> save -> inspect -> edit -> reload -> render` without
//! re-running the original script.
//!
//! ## Round-trip scope (this slice)
//!
//! * Director settings: `width`, `height`, `fps` (f64; audit #12).
//! * Scene graph topology: every node's `id`, `parent`, `kind`, and
//!   `children` are preserved. On reload, node ids are remapped and
//!   the `scene_root` in tracks/timeline is updated.
//! * Per-node: `transform` (current values), `opacity`, `z_index`,
//!   `blend_mode`, a `props` map, and the Taffy layout `Style`
//!   (`position`, `size`, `inset`, `flex_*`, `align_items`,
//!   `justify_content`, `padding`, `margin`, `gap`, `aspect_ratio`,
//!   `display`) — see `style_to_json` / `apply_layout_style_json`.
//! * **Box**: full node-field round-trip (opacity, bg_color, border_radius,
//!   border_width, border_color, shadow_*, blur, overflow) plus layout.
//! * **Text**: full reconstruction — content spans (text + per-span color,
//!   font family/weight/style, size, background, stroke, letter spacing, line
//!   height), default size/color, `fit`/`min_size`/`max_size`, `align`,
//!   `text_direction`, and text shadow — rebuilt via `text_node_to_json` and
//!   `build_text_node_from_props`, plus transform/opacity/layout.
//! * **Required kinds** (see [`NodeKindTier`]): Box, Text, Image, Video,
//!   Caption, SmartCaption, Effect, Composition, Lottie, Spectrum — full
//!   rebuild or **hard error** (never silent Box placeholder).
//! * **Optional kinds** (Vector, Shape, ActiveSpeakerCamera, Wgpu, Unknown):
//!   may degrade with a warning; load continues.
//! * **Animation**: transform channels and Box/Text opacity keyframes survive
//!   round-trip and reproduce after `update(t)`.
//! * **Tracks & legacy timeline**: tracks are canonical; legacy_timeline is
//!   migrated on load (plan 01).
use crate::animation::{Animated, EasingType};
use crate::director::Director;
use crate::entity::{EntityId, EntityTrack};
use crate::node::text::TextNode;
use crate::node::BoxNode;
use crate::node::caption::CaptionNode;
use crate::node::effect::{EffectNode, EffectType, ShaderUniform};
use crate::node::smart_caption::{SmartCaptionConfig, SmartCaptionNode};
use crate::node::image_node::ImageNode;
use crate::node::video_node::VideoNode;
use crate::node::lottie::LottieNode;
use crate::node::spectrum_visualizer::SpectrumVisualizerNode;
use crate::node::composition::CompositionNode;
use crate::scripting::utils::{apply_layout_style_json, json_map_to_rhai, style_to_json, text_node_to_json};
use crate::timeline::{Track, TrackClip};
use crate::types::{Color, NodeId};
use crate::video_wrapper::RenderMode;
use crate::{AssetLoader, DefaultAssetLoader};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use skia_safe::BlendMode;
use std::collections::HashMap;
use std::sync::Arc;
use taffy::style::Style;

/// Current project schema version. Bump on any breaking change to the
/// serialized shape; `load_project_json` rejects unknown (newer) versions.
/// Bumped to 2 when `NodeId` became generational `{ index, generation }` in JSON.
pub const PROJECT_VERSION: u32 = 2;

/// Round-trip tier for a project JSON `kind` string (Plan 05).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKindTier {
    /// Must rebuild exactly; failure is a hard load error.
    Required,
    /// Best-effort; may warn and continue (placeholder only if rebuild fails).
    Optional,
    /// Not a known engine kind.
    Unknown,
}

/// Classify a serialized node kind for the kind-policy matrix.
pub fn node_kind_tier(kind: &str) -> NodeKindTier {
    match kind {
        "Box" | "Text" | "Image" | "Video" | "Caption" | "SmartCaption" | "Effect"
        | "Composition" | "Lottie" | "Spectrum" => NodeKindTier::Required,
        "Vector" | "Shape" | "ActiveSpeakerCamera" | "Wgpu" => NodeKindTier::Optional,
        _ => NodeKindTier::Unknown,
    }
}

/// Required kind names for tests and docs.
pub const REQUIRED_NODE_KINDS: &[&str] = &[
    "Box",
    "Text",
    "Image",
    "Video",
    "Caption",
    "SmartCaption",
    "Effect",
    "Composition",
    "Lottie",
    "Spectrum",
];

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Project {
    pub version: u32,
    pub width: i32,
    pub height: i32,
    /// `f64` so 23.976 / 29.97 NTSC-accurate frame rates are representable.
    pub fps: f64,
    pub nodes: Vec<NodeData>,
    pub tracks: Vec<TrackData>,
    #[serde(default)]
    pub legacy_timeline: Vec<TimelineItemData>,
    /// Semantic entity tracks (faces, speakers, objects, caption phrases).
    #[serde(default)]
    pub entities: Vec<EntityTrack>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrackData {
    #[serde(default = "default_one")]
    pub opacity: f32,
    pub clips: Vec<ClipData>,
}

fn default_one() -> f32 {
    1.0
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClipData {
    pub scene_root: NodeId,
    #[serde(default)]
    pub name: Option<String>,
    pub start_time: f64,
    pub duration: f64,
    #[serde(default)]
    pub z_index: i32,
    #[serde(default)]
    pub source_start: f64,
    #[serde(default)]
    pub source_end: f64,
    #[serde(default = "default_one_f")]
    pub speed: f64,
    /// When set, pin source local time for the whole clip (plan 01 freeze).
    #[serde(default)]
    pub freeze_time: Option<f64>,
    #[serde(default)]
    pub fade_in: f64,
    #[serde(default)]
    pub fade_out: f64,
    #[serde(default)]
    pub audio_tracks: Vec<usize>,
}

fn default_one_f() -> f64 {
    1.0
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TimelineItemData {
    pub scene_root: NodeId,
    pub start_time: f64,
    pub duration: f64,
    #[serde(default)]
    pub z_index: i32,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub audio_tracks: Vec<usize>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NodeData {
    pub id: NodeId,
    #[serde(default)]
    pub parent: Option<NodeId>,
    pub kind: String,
    pub transform: TransformData,
    pub opacity: f32,
    #[serde(default)]
    pub z_index: i32,
    #[serde(default = "default_blend")]
    pub blend_mode: String,
    /// Kind-specific props + minimal layout subset (width/height/left/top
    /// as `f32` px) as accepted by the Rhai builder.
    #[serde(default)]
    pub props: JsonMap<String, JsonValue>,
    /// Keyframed animated channels (transform + Box/Text props) so motion
    /// survives the round-trip. Keys: `scale_x`, `translate_x`, `opacity`,
    /// `bg_color`, `border_radius`, ... Each value is a JSON array of
    /// `{ time, value, easing }` (f32) or `{ time, value:[r,g,b,a], easing }`
    /// (color). Absent keys mean the channel is static (use `transform`/
    /// `props` current values).
    #[serde(default)]
    pub animations: JsonMap<String, JsonValue>,
    /// Taffy layout `Style` so geometry/positioning survives the round-trip.
    /// Serialized by `style_to_json` and restored by `apply_layout_style_json`
    /// (which routes through the canonical Rhai layout parser).
    #[serde(default)]
    pub layout: JsonMap<String, JsonValue>,
    /// Semantic entity UUIDs referenced by this node.
    #[serde(default)]
    pub entity_refs: Vec<EntityId>,
}

fn default_blend() -> String {
    "srcover".to_string()
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct TransformData {
    #[serde(default = "default_one")]
    pub scale_x: f32,
    #[serde(default = "default_one")]
    pub scale_y: f32,
    #[serde(default)]
    pub rotation: f32,
    #[serde(default)]
    pub skew_x: f32,
    #[serde(default)]
    pub skew_y: f32,
    #[serde(default)]
    pub translate_x: f32,
    #[serde(default)]
    pub translate_y: f32,
    #[serde(default = "default_half")]
    pub pivot_x: f32,
    #[serde(default = "default_half")]
    pub pivot_y: f32,
}

fn default_half() -> f32 {
    0.5
}

// ---- Color / blend helpers ----

fn color_to_hex(c: Color) -> String {
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        (c.r * 255.0) as u8,
        (c.g * 255.0) as u8,
        (c.b * 255.0) as u8,
        (c.a * 255.0) as u8
    )
}

fn parse_hex(s: &str) -> Option<Color> {
    let s = s.trim().strip_prefix('#').unwrap_or(s);
    if s.len() != 6 && s.len() != 8 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()? as f32 / 255.0;
    let g = u8::from_str_radix(&s[2..4], 16).ok()? as f32 / 255.0;
    let b = u8::from_str_radix(&s[4..6], 16).ok()? as f32 / 255.0;
    let a = if s.len() == 8 {
        u8::from_str_radix(&s[6..8], 16).ok()? as f32 / 255.0
    } else {
        1.0
    };
    Some(Color::new(r, g, b, a))
}

fn blend_to_str(b: BlendMode) -> String {
    format!("{:?}", b).to_lowercase()
}

fn str_to_blend(s: &str) -> BlendMode {
    match s {
        "multiply" => BlendMode::Multiply,
        "screen" => BlendMode::Screen,
        "overlay" => BlendMode::Overlay,
        "darken" => BlendMode::Darken,
        "lighten" => BlendMode::Lighten,
        "colordodge" => BlendMode::ColorDodge,
        "colorburn" => BlendMode::ColorBurn,
        "hardlight" => BlendMode::HardLight,
        "softlight" => BlendMode::SoftLight,
        "difference" => BlendMode::Difference,
        "exclusion" => BlendMode::Exclusion,
        "hue" => BlendMode::Hue,
        "saturation" => BlendMode::Saturation,
        "color" => BlendMode::Color,
        "luminosity" => BlendMode::Luminosity,
        _ => BlendMode::SrcOver,
    }
}

// ---- Animated keyframe (de)serialization ----
//
// `Animated<T>` is not `Serialize` itself, but its `raw_keyframes`
// (`Vec<(T, f64, EasingType)>`) fully describe a channel. We serialize the
// keyframes so motion (not just the current value) round-trips.

#[derive(Serialize, Deserialize, Clone)]
struct KfF32 {
    time: f64,
    value: f32,
    easing: EasingType,
}

#[derive(Serialize, Deserialize, Clone)]
struct KfColor {
    time: f64,
    value: [f32; 4],
    easing: EasingType,
}

fn serialize_animated_f32(a: &Animated<f32>) -> Option<JsonValue> {
    if a.raw_keyframes.len() <= 1 {
        return None; // static; current_value is preserved separately
    }
    let kfs: Vec<KfF32> = a
        .raw_keyframes
        .iter()
        .map(|(v, t, e)| KfF32 {
            time: *t,
            value: *v,
            easing: e.clone(),
        })
        .collect();
    serde_json::to_value(kfs).ok()
}

fn serialize_animated_color(a: &Animated<Color>) -> Option<JsonValue> {
    if a.raw_keyframes.len() <= 1 {
        return None;
    }
    let kfs: Vec<KfColor> = a
        .raw_keyframes
        .iter()
        .map(|(v, t, e)| KfColor {
            time: *t,
            value: [v.r, v.g, v.b, v.a],
            easing: e.clone(),
        })
        .collect();
    serde_json::to_value(kfs).ok()
}

fn rebuild_animated_f32(v: &JsonValue) -> Option<Animated<f32>> {
    let kfs: Vec<KfF32> = serde_json::from_value(v.clone()).ok()?;
    if kfs.is_empty() {
        return None;
    }
    let mut a = Animated::new(kfs[0].value);
    // The keyframe crate derives each segment's easing from the keyframe at its
    // START, so the first keyframe's easing must be `kfs[0].easing` (matching
    // how `animate`/`add_segment` store it). `Animated::new` hardcodes Linear,
    // so re-apply it via `add_segment`, which sets the initial keyframe easing.
    a.add_segment(kfs[0].value, kfs[0].value, 0.0, kfs[0].easing.clone());
    let mut prev = kfs[0].time;
    for k in &kfs[1..] {
        let dur = (k.time - prev).max(0.0);
        a.add_keyframe(k.value, dur, k.easing.clone());
        prev = k.time;
    }
    Some(a)
}

fn rebuild_animated_color(v: &JsonValue) -> Option<Animated<Color>> {
    let kfs: Vec<KfColor> = serde_json::from_value(v.clone()).ok()?;
    if kfs.is_empty() {
        return None;
    }
    let c0 = Color::new(
        kfs[0].value[0],
        kfs[0].value[1],
        kfs[0].value[2],
        kfs[0].value[3],
    );
    let mut a = Animated::new(c0);
    a.add_segment(c0, c0, 0.0, kfs[0].easing.clone());
    let mut prev = kfs[0].time;
    for k in &kfs[1..] {
        let dur = (k.time - prev).max(0.0);
        a.add_keyframe(
            Color::new(k.value[0], k.value[1], k.value[2], k.value[3]),
            dur,
            k.easing.clone(),
        );
        prev = k.time;
    }
    Some(a)
}

/// Rebuild an `Animated<f32>` from saved keyframes, or fall back to a static
/// `current_value` when no keyframes were stored.
fn restore_animated_f32(field: &mut Animated<f32>, anim: Option<&JsonValue>, fallback: f32) {
    match anim.and_then(rebuild_animated_f32) {
        Some(a) => *field = a,
        None => field.current_value = fallback,
    }
}

/// Rebuild an optional `Animated<Color>` from saved keyframes, or fall back to
/// a static `current_value` (or `None`) when no keyframes were stored.
fn restore_animated_color(
    field: &mut Option<Animated<Color>>,
    anim: Option<&JsonValue>,
    fallback: Option<Color>,
) {
    match anim.and_then(rebuild_animated_color) {
        Some(a) => *field = Some(a),
        None => *field = fallback.map(Animated::new),
    }
}

/// Serialize every animated Box property that carries keyframes.
fn box_anims_to_json(b: &BoxNode) -> JsonMap<String, JsonValue> {
    let mut m = JsonMap::new();
    if let Some(v) = serialize_animated_f32(&b.opacity) {
        m.insert("opacity".into(), v);
    }
    if let Some(c) = &b.bg_color {
        if let Some(v) = serialize_animated_color(c) {
            m.insert("bg_color".into(), v);
        }
    }
    if let Some(v) = serialize_animated_f32(&b.border_radius) {
        m.insert("border_radius".into(), v);
    }
    if let Some(v) = serialize_animated_f32(&b.border_width) {
        m.insert("border_width".into(), v);
    }
    if let Some(c) = &b.border_color {
        if let Some(v) = serialize_animated_color(c) {
            m.insert("border_color".into(), v);
        }
    }
    if let Some(c) = &b.shadow_color {
        if let Some(v) = serialize_animated_color(c) {
            m.insert("shadow_color".into(), v);
        }
    }
    if let Some(v) = serialize_animated_f32(&b.shadow_blur) {
        m.insert("shadow_blur".into(), v);
    }
    if let Some(v) = serialize_animated_f32(&b.shadow_offset_x) {
        m.insert("shadow_offset_x".into(), v);
    }
    if let Some(v) = serialize_animated_f32(&b.shadow_offset_y) {
        m.insert("shadow_offset_y".into(), v);
    }
    if let Some(v) = serialize_animated_f32(&b.blur) {
        m.insert("blur".into(), v);
    }
    if let Some(v) = serialize_animated_f32(&b.backdrop_blur) {
        m.insert("backdrop_blur".into(), v);
    }
    m
}

/// Rebuild every animated Box property from saved keyframes, falling back to
/// the static value already applied by `apply_box_props`.
fn restore_box_props_anims(b: &mut BoxNode, anims: &JsonMap<String, JsonValue>) {
    let fb_op = b.opacity.current_value;
    restore_animated_f32(&mut b.opacity, anims.get("opacity"), fb_op);
    let fb_bg = b.bg_color.as_ref().map(|a| a.current_value);
    restore_animated_color(&mut b.bg_color, anims.get("bg_color"), fb_bg);
    let fb_br = b.border_radius.current_value;
    restore_animated_f32(&mut b.border_radius, anims.get("border_radius"), fb_br);
    let fb_bw = b.border_width.current_value;
    restore_animated_f32(&mut b.border_width, anims.get("border_width"), fb_bw);
    let fb_bc = b.border_color.as_ref().map(|a| a.current_value);
    restore_animated_color(&mut b.border_color, anims.get("border_color"), fb_bc);
    let fb_sc = b.shadow_color.as_ref().map(|a| a.current_value);
    restore_animated_color(&mut b.shadow_color, anims.get("shadow_color"), fb_sc);
    let fb_sbl = b.shadow_blur.current_value;
    restore_animated_f32(&mut b.shadow_blur, anims.get("shadow_blur"), fb_sbl);
    let fb_sx = b.shadow_offset_x.current_value;
    restore_animated_f32(&mut b.shadow_offset_x, anims.get("shadow_offset_x"), fb_sx);
    let fb_sy = b.shadow_offset_y.current_value;
    restore_animated_f32(&mut b.shadow_offset_y, anims.get("shadow_offset_y"), fb_sy);
    let fb_bl = b.blur.current_value;
    restore_animated_f32(&mut b.blur, anims.get("blur"), fb_bl);
    let fb_bbl = b.backdrop_blur.current_value;
    restore_animated_f32(&mut b.backdrop_blur, anims.get("backdrop_blur"), fb_bbl);
}

// ---- Image props ----

fn image_props_to_json(node: &ImageNode) -> JsonMap<String, JsonValue> {
    let mut p = JsonMap::new();
    if let Some(path) = &node.source_path {
        p.insert("source_path".into(), JsonValue::from(path.clone()));
    }
    p.insert(
        "object_fit".into(),
        serde_json::to_value(&node.object_fit).unwrap_or_default(),
    );
    p.insert(
        "border_radius".into(),
        JsonValue::from(node.border_radius.current_value),
    );
    p
}

fn rebuild_image_node_from_props(
    props: &JsonMap<String, JsonValue>,
    assets: &crate::systems::assets::AssetManager,
) -> ImageNode {
    let source_path = props
        .get("source_path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let image = source_path
        .as_ref()
        .and_then(|path| assets.load_image(path))
        .or_else(|| {
            // Fallback: try loading from a `path` prop for broader compatibility.
            props
                .get("path")
                .and_then(|v| v.as_str())
                .and_then(|path| assets.load_image(path))
        });

    let mut node = ImageNode::from_image_with_path(image, source_path.unwrap_or_default());

    if let Some(fit) = props.get("object_fit").and_then(|v| serde_json::from_value(v.clone()).ok()) {
        node.object_fit = fit;
    }
    if let Some(v) = props.get("border_radius").and_then(|v| v.as_f64()) {
        node.border_radius = Animated::new(v as f32);
    }
    node
}

// ---- Video props ----

fn video_props_to_json(node: &VideoNode) -> JsonMap<String, JsonValue> {
    let mut p = JsonMap::new();
    if let Some(path) = &node.source_path {
        p.insert("source_path".into(), JsonValue::from(path.clone()));
    }
    p.insert(
        "path".into(),
        JsonValue::from(node.path.to_string_lossy().to_string()),
    );
    p.insert(
        "object_fit".into(),
        serde_json::to_value(&node.object_fit).unwrap_or_default(),
    );
    p.insert(
        "border_radius".into(),
        JsonValue::from(node.border_radius.current_value),
    );
    p.insert(
        "render_mode".into(),
        JsonValue::from(match node.render_mode {
            crate::video_wrapper::RenderMode::Preview => "preview",
            crate::video_wrapper::RenderMode::Export => "export",
        }),
    );
    p
}

fn rebuild_video_node_from_props(
    props: &JsonMap<String, JsonValue>,
    render_mode: RenderMode,
) -> VideoNode {
    let source_path = props
        .get("source_path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let path = props
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| std::path::PathBuf::from(s))
        .unwrap_or_else(|| {
            source_path
                .clone()
                .map(std::path::PathBuf::from)
                .unwrap_or_default()
        });

    // Preserve the original path even if the file is not currently available;
    // VideoLoader will fail gracefully and the node can still round-trip.
    let source = crate::node::video_node::VideoSource::Path(path.clone());

    let render_mode = props
        .get("render_mode")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "export" => crate::video_wrapper::RenderMode::Export,
            _ => render_mode,
        })
        .unwrap_or(render_mode);

    let mut node = VideoNode::new(source, render_mode).with_source_path(
        source_path.unwrap_or_else(|| path.to_string_lossy().to_string()),
    );

    if let Some(fit) = props.get("object_fit").and_then(|v| serde_json::from_value(v.clone()).ok()) {
        node.object_fit = fit;
    }
    if let Some(v) = props.get("border_radius").and_then(|v| v.as_f64()) {
        node.border_radius = Animated::new(v as f32);
    }
    node
}

// ---- Lottie props ----

fn lottie_props_to_json(node: &LottieNode) -> JsonMap<String, JsonValue> {
    let mut p = JsonMap::new();
    p.insert("speed".into(), JsonValue::from(node.speed));
    p.insert("loop_anim".into(), JsonValue::from(node.loop_anim));
    p.insert("frame".into(), JsonValue::from(node.frame.current_value));
    p.insert("opacity".into(), JsonValue::from(node.opacity.current_value));
    if let Ok(model_json) = serde_json::to_value(node.model()) {
        p.insert("model".into(), model_json);
    }
    p
}

fn rebuild_lottie_node_from_props(
    props: &JsonMap<String, JsonValue>,
    assets: &crate::systems::assets::AssetManager,
) -> Option<LottieNode> {
    let model: Option<lottie_data::model::LottieJson> = props
        .get("model")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let data = if let Some(model) = model {
        serde_json::to_vec(&model).ok()?
    } else {
        // Fallback to loading from a persisted source path.
        let path = props
            .get("source_path")
            .or_else(|| props.get("path"))
            .and_then(|v| v.as_str())?;
        assets.load_blob(path).ok().map(|arc| (*arc).clone())?
    };

    let mut node = LottieNode::new(&data, HashMap::new(), assets).ok()?;

    if let Some(speed) = props.get("speed").and_then(|v| v.as_f64()) {
        node.speed = speed as f32;
    }
    if let Some(loop_anim) = props.get("loop_anim").and_then(|v| v.as_bool()) {
        node.loop_anim = loop_anim;
    }
    if let Some(frame) = props.get("frame").and_then(|v| v.as_f64()) {
        node.frame.current_value = frame as f32;
    }
    if let Some(opacity) = props.get("opacity").and_then(|v| v.as_f64()) {
        node.opacity.current_value = opacity as f32;
    }

    Some(node)
}

// ---- Spectrum props ----

fn spectrum_props_to_json(node: &SpectrumVisualizerNode) -> JsonMap<String, JsonValue> {
    let mut p = JsonMap::new();
    p.insert("track_id".into(), JsonValue::from(node.track_id));
    p.insert("bar_count".into(), JsonValue::from(node.bar_count));
    p.insert(
        "bar_color".into(),
        JsonValue::from(color_to_hex(node.bar_color)),
    );
    p.insert("bar_gap".into(), JsonValue::from(node.bar_gap));
    p.insert("smoothing".into(), JsonValue::from(node.smoothing));
    p.insert("position".into(), JsonValue::from(node.position.clone()));
    p.insert("min_height".into(), JsonValue::from(node.min_height));
    p.insert("max_height".into(), JsonValue::from(node.max_height));
    p.insert(
        "border_radius".into(),
        JsonValue::from(node.border_radius),
    );
    p
}

fn rebuild_spectrum_node_from_props(props: &JsonMap<String, JsonValue>) -> SpectrumVisualizerNode {
    let track_id = props
        .get("track_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(0);
    let bar_count = props
        .get("bar_count")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(16);
    let bar_color = props
        .get("bar_color")
        .and_then(|v| v.as_str())
        .and_then(parse_hex)
        .unwrap_or(crate::types::Color::new(0.0, 1.0, 0.0, 1.0));
    let bar_gap = props
        .get("bar_gap")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .unwrap_or(2.0);
    let smoothing = props
        .get("smoothing")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .unwrap_or(0.8);
    let position = props
        .get("position")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "bottom".to_string());
    let min_height = props
        .get("min_height")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .unwrap_or(0.05);
    let max_height = props
        .get("max_height")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .unwrap_or(0.9);
    let border_radius = props
        .get("border_radius")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .unwrap_or(0.0);

    SpectrumVisualizerNode::new(
        track_id,
        bar_count,
        bar_color,
        bar_gap,
        smoothing,
        position,
        min_height,
        max_height,
        border_radius,
    )
}

// ---- Composition props ----

fn composition_props_to_json(node: &CompositionNode) -> JsonMap<String, JsonValue> {
    let mut p = JsonMap::new();
    p.insert("start_offset".into(), JsonValue::from(node.start_offset));
    if let Ok(inner) = node.internal_director.lock() {
        let inner_project = director_to_project(&inner);
        if let Ok(json) = serde_json::to_value(inner_project) {
            p.insert("internal_project".into(), json);
        }
    }
    p
}

fn rebuild_composition_node_from_props(
    props: &JsonMap<String, JsonValue>,
) -> Option<CompositionNode> {
    let start_offset = props
        .get("start_offset")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let inner_project: Option<Project> = props
        .get("internal_project")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let inner_director = inner_project.and_then(|p| director_from_project(p).ok())?;
    let mut node = CompositionNode::new(inner_director);
    node.start_offset = start_offset;
    Some(node)
}

// ---- Effect props ----

fn effect_props_to_json(node: &EffectNode) -> JsonMap<String, JsonValue> {
    let mut p = JsonMap::new();
    let effects_json: Vec<JsonValue> = node
        .effects
        .iter()
        .map(effect_type_to_json)
        .collect();
    p.insert("effects".into(), JsonValue::Array(effects_json));
    p
}

fn effect_type_to_json(effect: &EffectType) -> JsonValue {
    use serde_json::json;
    match effect {
        EffectType::Blur(sigma) => json!({ "type": "Blur", "sigma": sigma.current_value }),
        EffectType::ColorMatrix(matrix) => json!({ "type": "ColorMatrix", "matrix": matrix }),
        EffectType::RuntimeShader { sksl, uniforms } => {
            let uniforms_json: JsonMap<String, JsonValue> = uniforms
                .iter()
                .map(|(k, v)| {
                    let val = match v {
                        ShaderUniform::Float(anim) => json!({ "kind": "float", "value": anim.current_value }),
                        ShaderUniform::Vec(anim) => json!({ "kind": "vec", "value": anim.current_value.0 }),
                    };
                    (k.clone(), val)
                })
                .collect();
            json!({ "type": "RuntimeShader", "sksl": sksl, "uniforms": uniforms_json })
        }
        EffectType::DropShadow { blur, offset_x, offset_y, color } => json!({
            "type": "DropShadow",
            "blur": blur.current_value,
            "offset_x": offset_x.current_value,
            "offset_y": offset_y.current_value,
            "color": color.current_value,
        }),
        EffectType::DirectionalBlur { strength, angle, samples } => json!({
            "type": "DirectionalBlur",
            "strength": strength.current_value,
            "angle": angle.current_value,
            "samples": *samples,
        }),
        EffectType::FilmGrain { intensity, size } => json!({
            "type": "FilmGrain",
            "intensity": intensity.current_value,
            "size": size.current_value,
        }),
        EffectType::Vignette { intensity, radius, feather } => json!({
            "type": "Vignette",
            "intensity": intensity.current_value,
            "radius": radius.current_value,
            "feather": feather.current_value,
        }),
        EffectType::ChromaticAberration(v) => json!({ "type": "ChromaticAberration", "value": v.current_value }),
        EffectType::Pixelate(v) => json!({ "type": "Pixelate", "value": v.current_value }),
        EffectType::Sharpen(v) => json!({ "type": "Sharpen", "value": v.current_value }),
        EffectType::Glow { intensity, threshold } => json!({
            "type": "Glow",
            "intensity": intensity.current_value,
            "threshold": threshold.current_value,
        }),
        EffectType::Warp { amplitude, frequency, direction, speed } => json!({
            "type": "Warp",
            "amplitude": amplitude.current_value,
            "frequency": frequency.current_value,
            "direction": direction.current_value,
            "speed": speed.current_value,
        }),
        EffectType::Swirl { radius, angle } => json!({
            "type": "Swirl",
            "radius": radius.current_value,
            "angle": angle.current_value,
        }),
        EffectType::Ripple { amplitude, frequency, speed } => json!({
            "type": "Ripple",
            "amplitude": amplitude.current_value,
            "frequency": frequency.current_value,
            "speed": speed.current_value,
        }),
        EffectType::Glass { scale, intensity } => json!({
            "type": "Glass",
            "scale": scale.current_value,
            "intensity": intensity.current_value,
        }),
        EffectType::Shake { intensity, speed } => json!({
            "type": "Shake",
            "intensity": intensity.current_value,
            "speed": speed.current_value,
        }),
        EffectType::Duotone { shadow, highlight } => json!({
            "type": "Duotone",
            "shadow": shadow,
            "highlight": highlight,
        }),
        EffectType::Posterize(v) => json!({ "type": "Posterize", "value": v.current_value }),
        EffectType::Threshold(v) => json!({ "type": "Threshold", "value": v.current_value }),
        EffectType::Scanlines { intensity, count } => json!({
            "type": "Scanlines",
            "intensity": intensity.current_value,
            "count": count.current_value,
        }),
        EffectType::Glitch { intensity, speed } => json!({
            "type": "Glitch",
            "intensity": intensity.current_value,
            "speed": speed.current_value,
        }),
        EffectType::EdgeDetect(v) => json!({ "type": "EdgeDetect", "value": v.current_value }),
        EffectType::ZoomBlur(v) => json!({ "type": "ZoomBlur", "value": v.current_value }),
        EffectType::Kaleidoscope(v) => json!({ "type": "Kaleidoscope", "value": v.current_value }),
        EffectType::Thermal => json!({ "type": "Thermal" }),
        EffectType::Liquid { frequency, amplitude } => json!({
            "type": "Liquid",
            "frequency": frequency.current_value,
            "amplitude": amplitude.current_value,
        }),
        EffectType::Acid { color, speed } => json!({
            "type": "Acid",
            "color": color,
            "speed": speed.current_value,
        }),
        EffectType::Halftone(v) => json!({ "type": "Halftone", "value": v.current_value }),
        EffectType::Ascii { scale, color } => json!({
            "type": "Ascii",
            "scale": scale.current_value,
            "color": color,
        }),
        EffectType::Sketch(v) => json!({ "type": "Sketch", "value": v.current_value }),
        EffectType::Mirror => json!({ "type": "Mirror" }),
        EffectType::TriTone { shadow, mid, highlight, midpoint } => json!({
            "type": "TriTone",
            "shadow": shadow,
            "mid": mid,
            "highlight": highlight,
            "midpoint": midpoint.current_value,
        }),
        EffectType::Vhs { distortion, scanline } => json!({
            "type": "Vhs",
            "distortion": distortion.current_value,
            "scanline": scanline.current_value,
        }),
        EffectType::NeonEdge { glow_color, threshold } => json!({
            "type": "NeonEdge",
            "glow_color": glow_color,
            "threshold": threshold.current_value,
        }),
        EffectType::Fisheye(v) => json!({ "type": "Fisheye", "value": v.current_value }),
        EffectType::Bulge { strength, radius } => json!({
            "type": "Bulge",
            "strength": strength.current_value,
            "radius": radius.current_value,
        }),
        EffectType::Barrel(v) => json!({ "type": "Barrel", "value": v.current_value }),
        EffectType::LensFlare { intensity, color } => json!({
            "type": "LensFlare",
            "intensity": intensity.current_value,
            "color": color,
        }),
        EffectType::Noir(v) => json!({ "type": "Noir", "value": v.current_value }),
        EffectType::Cyberpunk(v) => json!({ "type": "Cyberpunk", "value": v.current_value }),
        EffectType::Vintage(v) => json!({ "type": "Vintage", "value": v.current_value }),
        EffectType::Gameboy => json!({ "type": "Gameboy" }),
        EffectType::BleachBypass(v) => json!({ "type": "BleachBypass", "value": v.current_value }),
        EffectType::TealOrange(v) => json!({ "type": "TealOrange", "value": v.current_value }),
        EffectType::Plasma { scale, speed } => json!({
            "type": "Plasma",
            "scale": scale.current_value,
            "speed": speed.current_value,
        }),
        EffectType::Hologram(v) => json!({ "type": "Hologram", "value": v.current_value }),
        EffectType::Emboss { angle, strength } => json!({
            "type": "Emboss",
            "angle": angle.current_value,
            "strength": strength.current_value,
        }),
        EffectType::Toon { levels, edge } => json!({
            "type": "Toon",
            "levels": levels.current_value,
            "edge": edge.current_value,
        }),
        EffectType::OilPaint { block_size, levels } => json!({
            "type": "OilPaint",
            "block_size": block_size.current_value,
            "levels": levels.current_value,
        }),
        EffectType::LightLeak { intensity, color } => json!({
            "type": "LightLeak",
            "intensity": intensity.current_value,
            "color": color,
        }),
        EffectType::Aurora { intensity, speed } => json!({
            "type": "Aurora",
            "intensity": intensity.current_value,
            "speed": speed.current_value,
        }),
        EffectType::SunRays { intensity, angle, speed } => json!({
            "type": "SunRays",
            "intensity": intensity.current_value,
            "angle": angle.current_value,
            "speed": speed.current_value,
        }),
        EffectType::SkinSmooth { intensity, radius } => json!({
            "type": "SkinSmooth",
            "intensity": intensity.current_value,
            "radius": radius.current_value,
        }),
        EffectType::BeautyGlow { intensity, threshold } => json!({
            "type": "BeautyGlow",
            "intensity": intensity.current_value,
            "threshold": threshold.current_value,
        }),
        EffectType::SoftFocus { amount } => json!({
            "type": "SoftFocus",
            "amount": amount.current_value,
        }),
        EffectType::Vibrance { amount } => json!({
            "type": "Vibrance",
            "amount": amount.current_value,
        }),
        EffectType::Fog { intensity, color } => json!({
            "type": "Fog",
            "intensity": intensity.current_value,
            "color": color,
        }),
        EffectType::Mist { intensity, speed } => json!({
            "type": "Mist",
            "intensity": intensity.current_value,
            "speed": speed.current_value,
        }),
        EffectType::Rain { intensity, speed } => json!({
            "type": "Rain",
            "intensity": intensity.current_value,
            "speed": speed.current_value,
        }),
        EffectType::Snow { intensity, speed, size } => json!({
            "type": "Snow",
            "intensity": intensity.current_value,
            "speed": speed.current_value,
            "size": size.current_value,
        }),
        EffectType::Dust { intensity, speed } => json!({
            "type": "Dust",
            "intensity": intensity.current_value,
            "speed": speed.current_value,
        }),
        EffectType::BokehDots { intensity, size, color } => json!({
            "type": "BokehDots",
            "intensity": intensity.current_value,
            "size": size.current_value,
            "color": color,
        }),
        EffectType::FilmLook { amount, grain } => json!({
            "type": "FilmLook",
            "amount": amount.current_value,
            "grain": grain.current_value,
        }),
        EffectType::OrangeTeal { amount } => json!({
            "type": "OrangeTeal",
            "amount": amount.current_value,
        }),
        EffectType::CrossProcess { amount } => json!({
            "type": "CrossProcess",
            "amount": amount.current_value,
        }),
        EffectType::Polaroid { amount } => json!({
            "type": "Polaroid",
            "amount": amount.current_value,
        }),
        EffectType::FadedFilm { amount } => json!({
            "type": "FadedFilm",
            "amount": amount.current_value,
        }),
        EffectType::Midnight { amount } => json!({
            "type": "Midnight",
            "amount": amount.current_value,
        }),
        EffectType::Watercolor { intensity, edge } => json!({
            "type": "Watercolor",
            "intensity": intensity.current_value,
            "edge": edge.current_value,
        }),
        EffectType::Crosshatch { intensity, density } => json!({
            "type": "Crosshatch",
            "intensity": intensity.current_value,
            "density": density.current_value,
        }),
        EffectType::Pointillism { dot_size, intensity } => json!({
            "type": "Pointillism",
            "dot_size": dot_size.current_value,
            "intensity": intensity.current_value,
        }),
        EffectType::Mosaic { tile_size } => json!({
            "type": "Mosaic",
            "tile_size": tile_size.current_value,
        }),
        EffectType::Ink { threshold, intensity } => json!({
            "type": "Ink",
            "threshold": threshold.current_value,
            "intensity": intensity.current_value,
        }),
        EffectType::PencilColor { intensity } => json!({
            "type": "PencilColor",
            "intensity": intensity.current_value,
        }),
    }
}

fn rebuild_effect_node_from_props(
    props: &JsonMap<String, JsonValue>,
    assets: &crate::systems::assets::AssetManager,
) -> Option<EffectNode> {
    use serde_json::json;

    let effects_arr = props.get("effects")?.as_array()?;
    let mut effects = Vec::new();

    for eff in effects_arr {
        let ty = eff.get("type")?.as_str()?;
        let effect = match ty {
            "Blur" => {
                let v = eff.get("sigma")?.as_f64()? as f32;
                EffectType::Blur(Animated::new(v))
            }
            "ColorMatrix" => {
                let matrix: Vec<f32> = serde_json::from_value(eff.get("matrix")?.clone()).ok()?;
                EffectType::ColorMatrix(matrix)
            }
            "RuntimeShader" => {
                let sksl = eff.get("sksl")?.as_str()?.to_string();
                let mut uniforms = HashMap::new();
                if let Some(map) = eff.get("uniforms").and_then(|v| v.as_object()) {
                    for (k, v) in map {
                        let kind = v.get("kind")?.as_str()?;
                        match kind {
                            "float" => {
                                let value = v.get("value")?.as_f64()? as f32;
                                uniforms.insert(k.clone(), ShaderUniform::Float(Animated::new(value)));
                            }
                            "vec" => {
                                let value: Vec<f32> = serde_json::from_value(v.get("value")?.clone()).ok()?;
                                uniforms.insert(k.clone(), ShaderUniform::Vec(Animated::new(crate::animation::TweenableVector(value))));
                            }
                            _ => {}
                        }
                    }
                }
                EffectType::RuntimeShader { sksl, uniforms }
            }
            "DropShadow" => EffectType::DropShadow {
                blur: Animated::new(eff.get("blur")?.as_f64()? as f32),
                offset_x: Animated::new(eff.get("offset_x")?.as_f64()? as f32),
                offset_y: Animated::new(eff.get("offset_y")?.as_f64()? as f32),
                color: Animated::new(serde_json::from_value(eff.get("color")?.clone()).ok()?),
            },
            "DirectionalBlur" => EffectType::DirectionalBlur {
                strength: Animated::new(eff.get("strength")?.as_f64()? as f32),
                angle: Animated::new(eff.get("angle")?.as_f64()? as f32),
                samples: eff.get("samples")?.as_u64()? as u32,
            },
            "FilmGrain" => EffectType::FilmGrain {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
                size: Animated::new(eff.get("size")?.as_f64()? as f32),
            },
            "Vignette" => EffectType::Vignette {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
                radius: Animated::new(eff.get("radius")?.as_f64()? as f32),
                feather: Animated::new(eff.get("feather")?.as_f64()? as f32),
            },
            "ChromaticAberration" => EffectType::ChromaticAberration(Animated::new(eff.get("value")?.as_f64()? as f32)),
            "Pixelate" => EffectType::Pixelate(Animated::new(eff.get("value")?.as_f64()? as f32)),
            "Sharpen" => EffectType::Sharpen(Animated::new(eff.get("value")?.as_f64()? as f32)),
            "Glow" => EffectType::Glow {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
                threshold: Animated::new(eff.get("threshold")?.as_f64()? as f32),
            },
            "Warp" => EffectType::Warp {
                amplitude: Animated::new(eff.get("amplitude")?.as_f64()? as f32),
                frequency: Animated::new(eff.get("frequency")?.as_f64()? as f32),
                direction: Animated::new(eff.get("direction")?.as_f64()? as f32),
                speed: Animated::new(eff.get("speed")?.as_f64()? as f32),
            },
            "Swirl" => EffectType::Swirl {
                radius: Animated::new(eff.get("radius")?.as_f64()? as f32),
                angle: Animated::new(eff.get("angle")?.as_f64()? as f32),
            },
            "Ripple" => EffectType::Ripple {
                amplitude: Animated::new(eff.get("amplitude")?.as_f64()? as f32),
                frequency: Animated::new(eff.get("frequency")?.as_f64()? as f32),
                speed: Animated::new(eff.get("speed")?.as_f64()? as f32),
            },
            "Glass" => EffectType::Glass {
                scale: Animated::new(eff.get("scale")?.as_f64()? as f32),
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
            },
            "Shake" => EffectType::Shake {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
                speed: Animated::new(eff.get("speed")?.as_f64()? as f32),
            },
            "Duotone" => EffectType::Duotone {
                shadow: serde_json::from_value(eff.get("shadow")?.clone()).ok()?,
                highlight: serde_json::from_value(eff.get("highlight")?.clone()).ok()?,
            },
            "Posterize" => EffectType::Posterize(Animated::new(eff.get("value")?.as_f64()? as f32)),
            "Threshold" => EffectType::Threshold(Animated::new(eff.get("value")?.as_f64()? as f32)),
            "Scanlines" => EffectType::Scanlines {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
                count: Animated::new(eff.get("count")?.as_f64()? as f32),
            },
            "Glitch" => EffectType::Glitch {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
                speed: Animated::new(eff.get("speed")?.as_f64()? as f32),
            },
            "EdgeDetect" => EffectType::EdgeDetect(Animated::new(eff.get("value")?.as_f64()? as f32)),
            "ZoomBlur" => EffectType::ZoomBlur(Animated::new(eff.get("value")?.as_f64()? as f32)),
            "Kaleidoscope" => EffectType::Kaleidoscope(Animated::new(eff.get("value")?.as_f64()? as f32)),
            "Thermal" => EffectType::Thermal,
            "Liquid" => EffectType::Liquid {
                frequency: Animated::new(eff.get("frequency")?.as_f64()? as f32),
                amplitude: Animated::new(eff.get("amplitude")?.as_f64()? as f32),
            },
            "Acid" => EffectType::Acid {
                color: serde_json::from_value(eff.get("color")?.clone()).ok()?,
                speed: Animated::new(eff.get("speed")?.as_f64()? as f32),
            },
            "Halftone" => EffectType::Halftone(Animated::new(eff.get("value")?.as_f64()? as f32)),
            "Ascii" => EffectType::Ascii {
                scale: Animated::new(eff.get("scale")?.as_f64()? as f32),
                color: serde_json::from_value(eff.get("color")?.clone()).ok()?,
            },
            "Sketch" => EffectType::Sketch(Animated::new(eff.get("value")?.as_f64()? as f32)),
            "Mirror" => EffectType::Mirror,
            "TriTone" => EffectType::TriTone {
                shadow: serde_json::from_value(eff.get("shadow")?.clone()).ok()?,
                mid: serde_json::from_value(eff.get("mid")?.clone()).ok()?,
                highlight: serde_json::from_value(eff.get("highlight")?.clone()).ok()?,
                midpoint: Animated::new(eff.get("midpoint")?.as_f64()? as f32),
            },
            "Vhs" => EffectType::Vhs {
                distortion: Animated::new(eff.get("distortion")?.as_f64()? as f32),
                scanline: Animated::new(eff.get("scanline")?.as_f64()? as f32),
            },
            "NeonEdge" => EffectType::NeonEdge {
                glow_color: serde_json::from_value(eff.get("glow_color")?.clone()).ok()?,
                threshold: Animated::new(eff.get("threshold")?.as_f64()? as f32),
            },
            "Fisheye" => EffectType::Fisheye(Animated::new(eff.get("value")?.as_f64()? as f32)),
            "Bulge" => EffectType::Bulge {
                strength: Animated::new(eff.get("strength")?.as_f64()? as f32),
                radius: Animated::new(eff.get("radius")?.as_f64()? as f32),
            },
            "Barrel" => EffectType::Barrel(Animated::new(eff.get("value")?.as_f64()? as f32)),
            "LensFlare" => EffectType::LensFlare {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
                color: serde_json::from_value(eff.get("color")?.clone()).ok()?,
            },
            "Noir" => EffectType::Noir(Animated::new(eff.get("value")?.as_f64()? as f32)),
            "Cyberpunk" => EffectType::Cyberpunk(Animated::new(eff.get("value")?.as_f64()? as f32)),
            "Vintage" => EffectType::Vintage(Animated::new(eff.get("value")?.as_f64()? as f32)),
            "Gameboy" => EffectType::Gameboy,
            "BleachBypass" => EffectType::BleachBypass(Animated::new(eff.get("value")?.as_f64()? as f32)),
            "TealOrange" => EffectType::TealOrange(Animated::new(eff.get("value")?.as_f64()? as f32)),
            "Plasma" => EffectType::Plasma {
                scale: Animated::new(eff.get("scale")?.as_f64()? as f32),
                speed: Animated::new(eff.get("speed")?.as_f64()? as f32),
            },
            "Hologram" => EffectType::Hologram(Animated::new(eff.get("value")?.as_f64()? as f32)),
            "Emboss" => EffectType::Emboss {
                angle: Animated::new(eff.get("angle")?.as_f64()? as f32),
                strength: Animated::new(eff.get("strength")?.as_f64()? as f32),
            },
            "Toon" => EffectType::Toon {
                levels: Animated::new(eff.get("levels")?.as_f64()? as f32),
                edge: Animated::new(eff.get("edge")?.as_f64()? as f32),
            },
            "OilPaint" => EffectType::OilPaint {
                block_size: Animated::new(eff.get("block_size")?.as_f64()? as f32),
                levels: Animated::new(eff.get("levels")?.as_f64()? as f32),
            },
            "LightLeak" => EffectType::LightLeak {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
                color: serde_json::from_value(eff.get("color")?.clone()).ok()?,
            },
            "Aurora" => EffectType::Aurora {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
                speed: Animated::new(eff.get("speed")?.as_f64()? as f32),
            },
            "SunRays" => EffectType::SunRays {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
                angle: Animated::new(eff.get("angle")?.as_f64()? as f32),
                speed: Animated::new(eff.get("speed")?.as_f64()? as f32),
            },
            "SkinSmooth" => EffectType::SkinSmooth {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
                radius: Animated::new(eff.get("radius")?.as_f64()? as f32),
            },
            "BeautyGlow" => EffectType::BeautyGlow {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
                threshold: Animated::new(eff.get("threshold")?.as_f64()? as f32),
            },
            "SoftFocus" => EffectType::SoftFocus {
                amount: Animated::new(eff.get("amount")?.as_f64()? as f32),
            },
            "Vibrance" => EffectType::Vibrance {
                amount: Animated::new(eff.get("amount")?.as_f64()? as f32),
            },
            "Fog" => EffectType::Fog {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
                color: serde_json::from_value(eff.get("color")?.clone()).ok()?,
            },
            "Mist" => EffectType::Mist {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
                speed: Animated::new(eff.get("speed")?.as_f64()? as f32),
            },
            "Rain" => EffectType::Rain {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
                speed: Animated::new(eff.get("speed")?.as_f64()? as f32),
            },
            "Snow" => EffectType::Snow {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
                speed: Animated::new(eff.get("speed")?.as_f64()? as f32),
                size: Animated::new(eff.get("size")?.as_f64()? as f32),
            },
            "Dust" => EffectType::Dust {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
                speed: Animated::new(eff.get("speed")?.as_f64()? as f32),
            },
            "BokehDots" => EffectType::BokehDots {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
                size: Animated::new(eff.get("size")?.as_f64()? as f32),
                color: serde_json::from_value(eff.get("color")?.clone()).ok()?,
            },
            "FilmLook" => EffectType::FilmLook {
                amount: Animated::new(eff.get("amount")?.as_f64()? as f32),
                grain: Animated::new(eff.get("grain")?.as_f64()? as f32),
            },
            "OrangeTeal" => EffectType::OrangeTeal {
                amount: Animated::new(eff.get("amount")?.as_f64()? as f32),
            },
            "CrossProcess" => EffectType::CrossProcess {
                amount: Animated::new(eff.get("amount")?.as_f64()? as f32),
            },
            "Polaroid" => EffectType::Polaroid {
                amount: Animated::new(eff.get("amount")?.as_f64()? as f32),
            },
            "FadedFilm" => EffectType::FadedFilm {
                amount: Animated::new(eff.get("amount")?.as_f64()? as f32),
            },
            "Midnight" => EffectType::Midnight {
                amount: Animated::new(eff.get("amount")?.as_f64()? as f32),
            },
            "Watercolor" => EffectType::Watercolor {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
                edge: Animated::new(eff.get("edge")?.as_f64()? as f32),
            },
            "Crosshatch" => EffectType::Crosshatch {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
                density: Animated::new(eff.get("density")?.as_f64()? as f32),
            },
            "Pointillism" => EffectType::Pointillism {
                dot_size: Animated::new(eff.get("dot_size")?.as_f64()? as f32),
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
            },
            "Mosaic" => EffectType::Mosaic {
                tile_size: Animated::new(eff.get("tile_size")?.as_f64()? as f32),
            },
            "Ink" => EffectType::Ink {
                threshold: Animated::new(eff.get("threshold")?.as_f64()? as f32),
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
            },
            "PencilColor" => EffectType::PencilColor {
                intensity: Animated::new(eff.get("intensity")?.as_f64()? as f32),
            },
            _ => continue,
        };
        effects.push(effect);
    }

    Some(EffectNode {
        effects,
        style: Style::default(),
        shader_cache: assets.shader_cache.clone(),
        current_time: 0.0,
    })
}

// ---- SmartCaption props ----

fn caption_props_to_json(node: &CaptionNode) -> JsonMap<String, JsonValue> {
    let mut p = JsonMap::new();
    p.insert(
        "lines".into(),
        serde_json::to_value(&node.lines).unwrap_or_default(),
    );
    p.insert(
        "style".into(),
        serde_json::to_value(&node.style).unwrap_or_default(),
    );
    p.insert("opacity".into(), JsonValue::from(node.opacity.current_value));
    p
}

fn smart_caption_props_to_json(node: &SmartCaptionNode) -> JsonMap<String, JsonValue> {
    let mut p = JsonMap::new();
    p.insert("lines".into(), serde_json::to_value(&node.inner.lines).unwrap_or_default());
    p.insert("style".into(), serde_json::to_value(&node.inner.style).unwrap_or_default());
    p.insert("trajectory".into(), serde_json::to_value(&node.trajectory).unwrap_or_default());
    p.insert("canvas_w".into(), JsonValue::from(node.canvas_w));
    p.insert("canvas_h".into(), JsonValue::from(node.canvas_h));
    p.insert("continuous".into(), JsonValue::from(node.is_continuous()));
    if let Some(manifest) = &node.manifest {
        p.insert("manifest".into(), serde_json::to_value(manifest).unwrap_or_default());
    }
    if let Some(config) = &node.config {
        p.insert("config".into(), serde_json::to_value(config).unwrap_or_default());
    }
    p
}

fn rebuild_caption_node_from_props(
    props: &JsonMap<String, JsonValue>,
    font_collection: std::sync::Arc<std::sync::Mutex<skia_safe::textlayout::FontCollection>>,
) -> CaptionNode {
    let lines: Vec<crate::node::caption::CaptionLine> = props
        .get("lines")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let style: Option<crate::node::caption::CaptionStyle> = props
        .get("style")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let mut node = CaptionNode::new(lines, font_collection);
    if let Some(style) = style {
        node.style = style;
        node.after_style_changed();
    }
    if let Some(opacity) = props.get("opacity").and_then(|v| v.as_f64()) {
        node.opacity.current_value = opacity as f32;
    }
    node
}

// ---- Box props ----

fn box_props_to_json(b: &BoxNode) -> JsonMap<String, JsonValue> {
    let mut p = JsonMap::new();
    p.insert("opacity".into(), JsonValue::from(b.opacity.current_value));
    if let Some(bg) = &b.bg_color {
        p.insert("bg_color".into(), JsonValue::from(color_to_hex(bg.current_value)));
    }
    p.insert("border_radius".into(), JsonValue::from(b.border_radius.current_value));
    p.insert("border_width".into(), JsonValue::from(b.border_width.current_value));
    if let Some(bc) = &b.border_color {
        p.insert("border_color".into(), JsonValue::from(color_to_hex(bc.current_value)));
    }
    if let Some(sc) = &b.shadow_color {
        p.insert("shadow_color".into(), JsonValue::from(color_to_hex(sc.current_value)));
    }
    p.insert("shadow_blur".into(), JsonValue::from(b.shadow_blur.current_value));
    p.insert("shadow_x".into(), JsonValue::from(b.shadow_offset_x.current_value));
    p.insert("shadow_y".into(), JsonValue::from(b.shadow_offset_y.current_value));
    p.insert("blur".into(), JsonValue::from(b.blur.current_value));
    p.insert(
        "backdrop_blur".into(),
        JsonValue::from(b.backdrop_blur.current_value),
    );
    p.insert("overflow".into(), JsonValue::from(b.overflow.clone()));
    p
}

fn apply_box_props(b: &mut BoxNode, props: &JsonMap<String, JsonValue>) {
    if let Some(v) = props.get("opacity").and_then(|v| v.as_f64()) {
        b.opacity.current_value = v as f32;
    }
    if let Some(v) = props.get("bg_color").and_then(|v| v.as_str()) {
        if let Some(c) = parse_hex(v) {
            b.bg_color = Some(Animated::new(c));
        }
    }
    if let Some(v) = props.get("border_radius").and_then(|v| v.as_f64()) {
        b.border_radius.current_value = v as f32;
    }
    if let Some(v) = props.get("border_width").and_then(|v| v.as_f64()) {
        b.border_width.current_value = v as f32;
    }
    if let Some(v) = props.get("border_color").and_then(|v| v.as_str()) {
        if let Some(c) = parse_hex(v) {
            b.border_color = Some(Animated::new(c));
        }
    }
    if let Some(v) = props.get("shadow_color").and_then(|v| v.as_str()) {
        if let Some(c) = parse_hex(v) {
            b.shadow_color = Some(Animated::new(c));
        }
    }
    if let Some(v) = props.get("shadow_blur").and_then(|v| v.as_f64()) {
        b.shadow_blur.current_value = v as f32;
    }
    if let Some(v) = props.get("shadow_x").and_then(|v| v.as_f64()) {
        b.shadow_offset_x.current_value = v as f32;
    }
    if let Some(v) = props.get("shadow_y").and_then(|v| v.as_f64()) {
        b.shadow_offset_y.current_value = v as f32;
    }
    if let Some(v) = props.get("blur").and_then(|v| v.as_f64()) {
        b.blur.current_value = v as f32;
    }
    if let Some(v) = props.get("backdrop_blur").and_then(|v| v.as_f64()) {
        b.backdrop_blur.current_value = v as f32;
    }
    if let Some(v) = props.get("overflow").and_then(|v| v.as_str()) {
        b.overflow = v.to_string();
    }
}

// ---- Node kind detection (for save) ----

fn node_kind_name(sn: &crate::scene::SceneNode) -> String {
    use crate::node::*;
    let a = sn.element.as_any();
    if a.is::<BoxNode>() {
        return "Box".into();
    }
    if a.is::<TextNode>() {
        return "Text".into();
    }
    if a.is::<crate::node::image_node::ImageNode>() {
        return "Image".into();
    }
    if a.is::<crate::node::video_node::VideoNode>() {
        return "Video".into();
    }
    if a.is::<crate::node::lottie::LottieNode>() {
        return "Lottie".into();
    }
    if a.is::<crate::node::vector::VectorNode>() {
        return "Vector".into();
    }
    if a.is::<crate::node::effect::EffectNode>() {
        return "Effect".into();
    }
    if a.is::<crate::node::caption::CaptionNode>() {
        return "Caption".into();
    }
    if a.is::<crate::node::smart_caption::SmartCaptionNode>() {
        return "SmartCaption".into();
    }
    if a.is::<crate::node::composition::CompositionNode>() {
        return "Composition".into();
    }
    if a.is::<crate::node::spectrum_visualizer::SpectrumVisualizerNode>() {
        return "Spectrum".into();
    }
    "Unknown".into()
}

fn node_opacity(sn: &crate::scene::SceneNode) -> f32 {
    if let Some(b) = sn.element.as_any().downcast_ref::<BoxNode>() {
        return b.opacity.current_value;
    }
    if let Some(t) = sn.element.as_any().downcast_ref::<TextNode>() {
        return t.opacity.current_value;
    }
    if let Some(img) = sn.element.as_any().downcast_ref::<ImageNode>() {
        return img.opacity.current_value;
    }
    if let Some(vid) = sn.element.as_any().downcast_ref::<VideoNode>() {
        return vid.opacity.current_value;
    }
    if let Some(cap) = sn.element.as_any().downcast_ref::<CaptionNode>() {
        return cap.opacity.current_value;
    }
    if let Some(smart) = sn.element.as_any().downcast_ref::<crate::node::smart_caption::SmartCaptionNode>() {
        return smart.inner.opacity.current_value;
    }
    if let Some(lottie) = sn.element.as_any().downcast_ref::<LottieNode>() {
        return lottie.opacity.current_value;
    }
    if let Some(spectrum) = sn.element.as_any().downcast_ref::<SpectrumVisualizerNode>() {
        return spectrum.opacity.current_value;
    }
    if let Some(eff) = sn.element.as_any().downcast_ref::<EffectNode>() {
        return eff.effects.first().map(|_| 1.0).unwrap_or(1.0);
    }
    1.0
}

// ---- Save (Director -> Project) ----

pub fn director_to_project(d: &Director) -> Project {
    let mut nodes = Vec::new();
    for (id, sn) in d.scene.iter_alive() {
            let kind = node_kind_name(sn);
            let opacity = node_opacity(sn);
            let transform = TransformData {
                scale_x: sn.transform.scale_x.current_value,
                scale_y: sn.transform.scale_y.current_value,
                rotation: sn.transform.rotation.current_value,
                skew_x: sn.transform.skew_x.current_value,
                skew_y: sn.transform.skew_y.current_value,
                translate_x: sn.transform.translate_x.current_value,
                translate_y: sn.transform.translate_y.current_value,
                pivot_x: sn.transform.pivot_x,
                pivot_y: sn.transform.pivot_y,
            };
            let props = match kind.as_str() {
                "Box" => sn
                    .element
                    .as_any()
                    .downcast_ref::<BoxNode>()
                    .map(box_props_to_json)
                    .unwrap_or_default(),
                "Text" => sn
                    .element
                    .as_any()
                    .downcast_ref::<TextNode>()
                    .map(text_node_to_json)
                    .unwrap_or_default(),
                "Caption" => sn
                    .element
                    .as_any()
                    .downcast_ref::<CaptionNode>()
                    .map(caption_props_to_json)
                    .unwrap_or_default(),
                "SmartCaption" => sn
                    .element
                    .as_any()
                    .downcast_ref::<SmartCaptionNode>()
                    .map(smart_caption_props_to_json)
                    .unwrap_or_default(),
                "Image" => sn
                    .element
                    .as_any()
                    .downcast_ref::<ImageNode>()
                    .map(image_props_to_json)
                    .unwrap_or_default(),
                "Video" => sn
                    .element
                    .as_any()
                    .downcast_ref::<VideoNode>()
                    .map(video_props_to_json)
                    .unwrap_or_default(),
                "Lottie" => sn
                    .element
                    .as_any()
                    .downcast_ref::<LottieNode>()
                    .map(lottie_props_to_json)
                    .unwrap_or_default(),
                "Effect" => sn
                    .element
                    .as_any()
                    .downcast_ref::<EffectNode>()
                    .map(effect_props_to_json)
                    .unwrap_or_default(),
                "Composition" => sn
                    .element
                    .as_any()
                    .downcast_ref::<CompositionNode>()
                    .map(composition_props_to_json)
                    .unwrap_or_default(),
                "Spectrum" => sn
                    .element
                    .as_any()
                    .downcast_ref::<SpectrumVisualizerNode>()
                    .map(spectrum_props_to_json)
                    .unwrap_or_default(),
                _ => {
                    let mut p = JsonMap::new();
                    p.insert(
                        "_note".into(),
                        JsonValue::from(format!("{}_not_round_tripped", kind)),
                    );
                    p
                }
            };

            // Layout (Taffy Style) so geometry/positioning survives the round-trip.
            let layout = style_to_json(&sn.element.layout_style());

            // Animated channels (keyframes) so motion survives the round-trip.
            let mut animations = JsonMap::new();
            let t_channels: [(&str, &Animated<f32>); 7] = [
                ("scale_x", &sn.transform.scale_x),
                ("scale_y", &sn.transform.scale_y),
                ("rotation", &sn.transform.rotation),
                ("skew_x", &sn.transform.skew_x),
                ("skew_y", &sn.transform.skew_y),
                ("translate_x", &sn.transform.translate_x),
                ("translate_y", &sn.transform.translate_y),
            ];
            for (k, a) in t_channels {
                if let Some(v) = serialize_animated_f32(a) {
                    animations.insert(k.to_string(), v);
                }
            }
            match kind.as_str() {
                "Box" => {
                    if let Some(b) = sn.element.as_any().downcast_ref::<BoxNode>() {
                        for (k, v) in box_anims_to_json(b) {
                            animations.insert(k, v);
                        }
                    }
                }
                "Text" => {
                    if let Some(t) = sn.element.as_any().downcast_ref::<TextNode>() {
                        if let Some(v) = serialize_animated_f32(&t.opacity) {
                            animations.insert("opacity".into(), v);
                        }
                    }
                }
                _ => {}
            }
            nodes.push(NodeData {
                id,
                parent: sn.parent,
                kind,
                transform,
                opacity,
                z_index: sn.z_index,
                blend_mode: blend_to_str(sn.blend_mode),
                props,
                animations,
                layout,
                entity_refs: sn.entity_refs.clone(),
            });
    }
    let tracks = d
        .tracks
        .iter()
        .map(|t| TrackData {
            opacity: t.opacity,
            clips: t.clips.iter().map(track_clip_to_data).collect(),
        })
        .collect();
    // Tracks are canonical; legacy_timeline is only read on load for migration.
    Project {
        version: PROJECT_VERSION,
        width: d.width,
        height: d.height,
        fps: d.fps,
        nodes,
        tracks,
        legacy_timeline: Vec::new(),
        entities: d.entities.iter().cloned().collect(),
    }
}

fn track_clip_to_data(c: &TrackClip) -> ClipData {
    ClipData {
        scene_root: c.scene_root,
        name: c.name.clone(),
        start_time: c.start_time,
        duration: c.duration,
        z_index: c.z_index,
        source_start: c.source_start,
        source_end: c.source_end,
        speed: c.speed,
        freeze_time: c.freeze_time,
        fade_in: c.fade_in,
        fade_out: c.fade_out,
        audio_tracks: c.audio_tracks.clone(),
    }
}

// ---- Load (Project -> Director) ----

pub fn director_from_project(project: Project) -> anyhow::Result<Director> {
    let loader: Arc<dyn AssetLoader> = Arc::new(DefaultAssetLoader);
    let mut director = Director::new(
        project.width,
        project.height,
        project.fps,
        loader,
        RenderMode::Preview,
        None,
    );

    let mut id_map: std::collections::HashMap<NodeId, NodeId> = Default::default();

    // First pass: create nodes. Required kinds hard-error; optional may placeholder.
    for n in &project.nodes {
        let new_id = match n.kind.as_str() {
            "Box" => {
                let mut b = BoxNode::new();
                apply_box_props(&mut b, &n.props);
                apply_layout_style_json(&mut b.style, &n.layout);
                director.scene.add_node(Box::new(b))
            }
            "Text" => {
                let mut map = json_map_to_rhai(&n.props);
                for (k, v) in json_map_to_rhai(&n.layout) {
                    map.insert(k, v);
                }
                let mut tnode = crate::scripting::api::nodes::build_text_node_from_props(
                    director.assets.font_collection.clone(),
                    &map,
                );
                tnode.opacity.current_value = n.opacity;
                director.scene.add_node(Box::new(tnode))
            }
            "Caption" => {
                let mut node = rebuild_caption_node_from_props(
                    &n.props,
                    director.assets.font_collection.clone(),
                );
                node.opacity.current_value = n.opacity;
                apply_layout_style_json(&mut node.style_cache, &n.layout);
                director.scene.add_node(Box::new(node))
            }
            "SmartCaption" => {
                let inner = rebuild_caption_node_from_props(
                    &n.props,
                    director.assets.font_collection.clone(),
                );
                let trajectory: crate::semantic::CaptionTrajectory = n
                    .props
                    .get("trajectory")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let canvas_w = n
                    .props
                    .get("canvas_w")
                    .and_then(|v| v.as_f64())
                    .map(|v| v as f32)
                    .unwrap_or(1080.0);
                let canvas_h = n
                    .props
                    .get("canvas_h")
                    .and_then(|v| v.as_f64())
                    .map(|v| v as f32)
                    .unwrap_or(1920.0);
                let continuous = n
                    .props
                    .get("continuous")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let manifest: Option<crate::semantic::SemanticManifest> = n
                    .props
                    .get("manifest")
                    .and_then(|v| serde_json::from_value(v.clone()).ok());
                let config: Option<SmartCaptionConfig> = n
                    .props
                    .get("config")
                    .and_then(|v| serde_json::from_value(v.clone()).ok());
                let mut node = SmartCaptionNode::from_parts(
                    inner,
                    trajectory,
                    canvas_w,
                    canvas_h,
                    continuous,
                    manifest,
                    config,
                );
                node.inner.opacity.current_value = n.opacity;
                apply_layout_style_json(&mut node.inner.style_cache, &n.layout);
                director.scene.add_node(Box::new(node))
            }
            "Image" => {
                let mut node = rebuild_image_node_from_props(&n.props, &director.assets);
                node.opacity.current_value = n.opacity;
                apply_layout_style_json(&mut node.style, &n.layout);
                director.scene.add_node(Box::new(node))
            }
            "Video" => {
                let mut node = rebuild_video_node_from_props(&n.props, director.render_mode);
                node.opacity.current_value = n.opacity;
                apply_layout_style_json(&mut node.style, &n.layout);
                director.scene.add_node(Box::new(node))
            }
            "Lottie" => {
                let mut node = rebuild_lottie_node_from_props(&n.props, &director.assets)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "load_project: required kind 'Lottie' failed to rebuild (id was {})",
                            n.id
                        )
                    })?;
                node.opacity.current_value = n.opacity;
                apply_layout_style_json(&mut node.style, &n.layout);
                director.scene.add_node(Box::new(node))
            }
            "Effect" => {
                let mut node = rebuild_effect_node_from_props(&n.props, &director.assets)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "load_project: required kind 'Effect' failed to rebuild (id was {})",
                            n.id
                        )
                    })?;
                apply_layout_style_json(&mut node.style, &n.layout);
                director.scene.add_node(Box::new(node))
            }
            "Composition" => {
                let mut node = rebuild_composition_node_from_props(&n.props).ok_or_else(|| {
                    anyhow::anyhow!(
                        "load_project: required kind 'Composition' failed to rebuild (id was {})",
                        n.id
                    )
                })?;
                apply_layout_style_json(&mut node.style, &n.layout);
                director.scene.add_node(Box::new(node))
            }
            "Spectrum" => {
                let mut node = rebuild_spectrum_node_from_props(&n.props);
                node.opacity.current_value = n.opacity;
                apply_layout_style_json(&mut node.style, &n.layout);
                director.scene.add_node(Box::new(node))
            }
            other => {
                match node_kind_tier(other) {
                    NodeKindTier::Required => {
                        anyhow::bail!(
                            "load_project: required kind '{other}' has no loader (id was {})",
                            n.id
                        );
                    }
                    NodeKindTier::Optional | NodeKindTier::Unknown => {
                        let mut b = BoxNode::new();
                        b.opacity.current_value = n.opacity;
                        apply_layout_style_json(&mut b.style, &n.layout);
                        tracing::warn!(
                            "load_project: kind '{other}' is optional/unknown; \
                             reconstructing as a BoxNode placeholder (id was {})",
                            n.id
                        );
                        director.scene.add_node(Box::new(b))
                    }
                }
            }
        };
        id_map.insert(n.id, new_id);
    }

    // Second pass: restore parent/child links.
    for n in &project.nodes {
        if let Some(parent_old) = n.parent {
            if let (Some(&child_new), Some(&parent_new)) =
                (id_map.get(&n.id), id_map.get(&parent_old))
            {
                let _ = director.scene.try_add_child(parent_new, child_new);
            }
        }
    }

    // Third pass: restore transform / z_index / blend_mode (+ keyframes) on each node.
    for n in &project.nodes {
        if let Some(&new_id) = id_map.get(&n.id) {
            if let Some(sn) = director.scene.get_node_mut(new_id) {
                // Transform channels: rebuild from keyframes when present.
                restore_animated_f32(
                    &mut sn.transform.scale_x,
                    n.animations.get("scale_x"),
                    n.transform.scale_x,
                );
                restore_animated_f32(
                    &mut sn.transform.scale_y,
                    n.animations.get("scale_y"),
                    n.transform.scale_y,
                );
                restore_animated_f32(
                    &mut sn.transform.rotation,
                    n.animations.get("rotation"),
                    n.transform.rotation,
                );
                restore_animated_f32(
                    &mut sn.transform.skew_x,
                    n.animations.get("skew_x"),
                    n.transform.skew_x,
                );
                restore_animated_f32(
                    &mut sn.transform.skew_y,
                    n.animations.get("skew_y"),
                    n.transform.skew_y,
                );
                restore_animated_f32(
                    &mut sn.transform.translate_x,
                    n.animations.get("translate_x"),
                    n.transform.translate_x,
                );
                restore_animated_f32(
                    &mut sn.transform.translate_y,
                    n.animations.get("translate_y"),
                    n.transform.translate_y,
                );
                sn.transform.pivot_x = n.transform.pivot_x;
                sn.transform.pivot_y = n.transform.pivot_y;
                sn.z_index = n.z_index;
                sn.blend_mode = str_to_blend(&n.blend_mode);
                sn.entity_refs = n.entity_refs.clone();

                // Property animations. Real Box nodes restore all Box props;
                // other kinds (reconstructed as Box placeholders) at least
                // keep their opacity animation.
                if let Some(b) = sn.element.as_any_mut().downcast_mut::<BoxNode>() {
                    if n.kind == "Box" {
                        restore_box_props_anims(b, &n.animations);
                    } else if let Some(a) =
                        n.animations.get("opacity").and_then(rebuild_animated_f32)
                    {
                        b.opacity = a;
                    }
                } else if let Some(tn) = sn.element.as_any_mut().downcast_mut::<TextNode>() {
                    if let Some(a) = n.animations.get("opacity").and_then(rebuild_animated_f32) {
                        tn.opacity = a;
                    }
                }
            }
        }
    }

    // Restore tracks.
    director.tracks = project
        .tracks
        .into_iter()
        .map(|t| {
            // Track struct fields — provide required `name` and `enabled` defaults.
            Track {
                name: String::new(),
                enabled: true,
                opacity: t.opacity,
                clips: t
                    .clips
                    .into_iter()
                    .filter_map(|c| {
                        id_map.get(&c.scene_root).map(|&new_root| TrackClip {
                            scene_root: new_root,
                            name: c.name,
                            start_time: c.start_time,
                            duration: c.duration,
                            z_index: c.z_index,
                            source_start: c.source_start,
                            source_end: c.source_end,
                            speed: c.speed,
                            freeze_time: c.freeze_time,
                            fade_in: c.fade_in,
                            fade_out: c.fade_out,
                            audio_tracks: c.audio_tracks,
                        })
                    })
                    .collect(),
            }
        })
        .collect();

    // Migrate pre-plan-01 projects that only stored legacy_timeline.
    let tracks_empty = director.tracks.iter().all(|t| t.clips.is_empty());
    if tracks_empty && !project.legacy_timeline.is_empty() {
        director.ensure_default_track();
        let idx = director.default_track_idx;
        for it in project.legacy_timeline {
            if let Some(&new_root) = id_map.get(&it.scene_root) {
                let mut clip = TrackClip::new(new_root, it.start_time, it.duration);
                clip.name = it.name;
                clip.z_index = it.z_index;
                clip.audio_tracks = it.audio_tracks;
                director.tracks[idx].clips.push(clip);
            }
        }
    }

    // Restore semantic entity tracks (UUIDs are stable, no remapping needed).
    for track in project.entities {
        director.entities.add_track(track);
    }

    Ok(director)
}

// ---- Public save/load entry points on Director ----

impl Director {
    /// Serialize this director to a versioned project JSON file.
    pub fn save_project_json(&self, path: impl AsRef<std::path::Path>) -> anyhow::Result<()> {
        let project = director_to_project(self);
        let json = serde_json::to_string_pretty(&project)?;
        if let Some(parent) = path.as_ref().parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load a project JSON file into a new Director. Node ids are remapped;
    /// see module docs for the round-trip scope and lossy kinds.
    pub fn load_project_json(path: impl AsRef<std::path::Path>) -> anyhow::Result<Director> {
        let bytes = std::fs::read(path)?;
        let project: Project = serde_json::from_slice(&bytes)?;
        if project.version > PROJECT_VERSION {
            anyhow::bail!(
                "Project version {} is newer than supported version {}. \
                 Update the engine or regenerate the project.",
                project.version,
                PROJECT_VERSION
            );
        }
        director_from_project(project)
    }
}
