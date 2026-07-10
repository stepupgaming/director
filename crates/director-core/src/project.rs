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
//! * **Text**: minimal reconstruction (transform, opacity, `font_size`,
//!   `color`, `font_family`, `content` from a `_content` prop key) plus
//!   layout. Rich per-span styling degrades to a single default span.
//! * **Other kinds** (Image, Video, Lottie, Effect, Composition, Caption,
//!   Vector, Spectrum, WGPU): kind tag + transform + opacity + layout
//!   preserved. On reload the node is reconstructed as a `BoxNode`
//!   placeholder with its Taffy layout applied. The original element type is
//!   lossy, but the graph, geometry, and timeline stay loadable and renderable.
//! * **Animation**: transform channels (scale/rotation/skew/translate) and
//!   Box props (opacity, bg_color, border_*, shadow_*, blur, backdrop_blur)
//!   serialize their **keyframes**, so motion survives the round-trip and
//!   reproduces at any time after `update(t)`. Text nodes preserve opacity
//!   keyframes (reconstructed as Box placeholders; see below).
//! * **Tracks & legacy timeline**: fully round-tripped (clips, fades,
//!   speed, source in/out, z_index, audio_tracks).

use crate::animation::{Animated, EasingType};
use crate::director::{Director, TimelineItem};
use crate::node::text::TextNode;
use crate::node::BoxNode;
use crate::scripting::utils::{apply_layout_style_json, style_to_json};
use crate::timeline::{Track, TrackClip};
use crate::types::{Color, NodeId};
use crate::video_wrapper::RenderMode;
use crate::{AssetLoader, DefaultAssetLoader};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use skia_safe::BlendMode;
use std::sync::Arc;

/// Current project schema version. Bump on any breaking change to the
/// serialized shape; `load_project_json` rejects unknown (newer) versions.
pub const PROJECT_VERSION: u32 = 1;

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
            easing: *e,
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
            easing: *e,
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
    a.add_segment(kfs[0].value, kfs[0].value, 0.0, kfs[0].easing);
    let mut prev = kfs[0].time;
    for k in &kfs[1..] {
        let dur = (k.time - prev).max(0.0);
        a.add_keyframe(k.value, dur, k.easing);
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
    a.add_segment(c0, c0, 0.0, kfs[0].easing);
    let mut prev = kfs[0].time;
    for k in &kfs[1..] {
        let dur = (k.time - prev).max(0.0);
        a.add_keyframe(
            Color::new(k.value[0], k.value[1], k.value[2], k.value[3]),
            dur,
            k.easing,
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
    if let Some(preset) = &b.safe_area_preset {
        p.insert("safe_area".into(), JsonValue::from(preset.clone()));
    }
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
    if let Some(v) = props
        .get("safe_area")
        .or_else(|| props.get("safe_area_preset"))
        .and_then(|v| v.as_str())
    {
        b.safe_area_preset = Some(v.to_string());
    }
}

// ---- Text props ----

fn text_span_to_json(s: &crate::element::TextSpan) -> JsonValue {
    let mut m = JsonMap::new();
    m.insert("text".into(), JsonValue::from(s.text.clone()));
    if let Some(c) = s.color {
        m.insert("color".into(), JsonValue::from(crate::project::color_to_hex_pub(c)));
    }
    if let Some(f) = s.font_size {
        m.insert("font_size".into(), JsonValue::from(f));
    }
    if let Some(w) = s.font_weight {
        m.insert("font_weight".into(), JsonValue::from(w as i64));
    }
    if let Some(ff) = &s.font_family {
        m.insert("font_family".into(), JsonValue::from(ff.clone()));
    }
    if let Some(fs) = &s.font_style {
        m.insert("font_style".into(), JsonValue::from(fs.clone()));
    }
    if let Some(bc) = s.background_color {
        m.insert("background_color".into(), JsonValue::from(crate::project::color_to_hex_pub(bc)));
    }
    if let Some(bp) = s.background_padding {
        m.insert("background_padding".into(), JsonValue::from(bp));
    }
    if let Some(sw) = s.stroke_width {
        m.insert("stroke_width".into(), JsonValue::from(sw));
    }
    if let Some(sc) = s.stroke_color {
        m.insert("stroke_color".into(), JsonValue::from(crate::project::color_to_hex_pub(sc)));
    }
    if let Some(ls) = s.letter_spacing {
        m.insert("letter_spacing".into(), JsonValue::from(ls));
    }
    if let Some(lh) = s.line_height {
        m.insert("line_height".into(), JsonValue::from(lh));
    }
    JsonValue::Object(m)
}

fn text_span_from_json(v: &JsonValue) -> Option<crate::element::TextSpan> {
    let obj = v.as_object()?;
    let text = obj.get("text").and_then(|v| v.as_str())?.to_string();
    let parse_color = |v: &JsonValue| -> Option<crate::types::Color> {
        v.as_str().and_then(crate::project::parse_hex_pub)
    };
    Some(crate::element::TextSpan {
        text,
        color: obj.get("color").and_then(parse_color),
        font_size: obj.get("font_size").and_then(|v| v.as_f64()).map(|v| v as f32),
        font_weight: obj.get("font_weight").and_then(|v| v.as_i64()).map(|v| v as u16),
        font_family: obj.get("font_family").and_then(|v| v.as_str()).map(|s| s.to_string()),
        font_style: obj.get("font_style").and_then(|v| v.as_str()).map(|s| s.to_string()),
        background_color: obj.get("background_color").and_then(parse_color),
        background_padding: obj
            .get("background_padding")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32),
        stroke_width: obj.get("stroke_width").and_then(|v| v.as_f64()).map(|v| v as f32),
        stroke_color: obj.get("stroke_color").and_then(parse_color),
        fill_gradient: None,
        letter_spacing: obj
            .get("letter_spacing")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32),
        line_height: obj.get("line_height").and_then(|v| v.as_f64()).map(|v| v as f32),
    })
}

fn text_props_to_json(t: &TextNode) -> JsonMap<String, JsonValue> {
    let mut p = JsonMap::new();
    let spans: Vec<JsonValue> = t.spans.iter().map(text_span_to_json).collect();
    p.insert("spans".into(), JsonValue::Array(spans));
    p.insert(
        "default_font_size".into(),
        JsonValue::from(t.default_font_size.current_value),
    );
    p.insert(
        "default_color".into(),
        JsonValue::from(crate::project::color_to_hex_pub(t.default_color.current_value)),
    );
    p.insert("min_size".into(), JsonValue::from(t.min_size));
    p.insert("max_size".into(), JsonValue::from(t.max_size));
    let fit = match t.fit_mode {
        crate::element::TextFit::None => "none",
        crate::element::TextFit::Shrink => "shrink",
    };
    p.insert("fit".into(), JsonValue::from(fit));
    let align = match t.text_align {
        skia_safe::textlayout::TextAlign::Left => "left",
        skia_safe::textlayout::TextAlign::Center => "center",
        skia_safe::textlayout::TextAlign::Right => "right",
        skia_safe::textlayout::TextAlign::Justify => "justify",
        _ => "left",
    };
    p.insert("text_align".into(), JsonValue::from(align));
    if let Some(sh) = &t.shadow {
        p.insert(
            "shadow_enabled".into(),
            JsonValue::from(1.0),
        );
        p.insert(
            "shadow_color".into(),
            JsonValue::from(crate::project::color_to_hex_pub(sh.color)),
        );
        p.insert("shadow_blur".into(), JsonValue::from(sh.blur));
        p.insert(
            "shadow_offset_x".into(),
            JsonValue::from(sh.offset.0),
        );
        p.insert(
            "shadow_offset_y".into(),
            JsonValue::from(sh.offset.1),
        );
    }
    p
}

pub(crate) fn color_to_hex_pub(c: crate::types::Color) -> String {
    color_to_hex(c)
}

pub(crate) fn parse_hex_pub(s: &str) -> Option<crate::types::Color> {
    parse_hex(s)
}

fn apply_text_props(
    node: &mut TextNode,
    props: &JsonMap<String, JsonValue>,
) {
    // Rebuild the span list (the only piece TextNode::new needs to take).
    let mut spans: Vec<crate::element::TextSpan> = Vec::new();
    if let Some(arr) = props.get("spans").and_then(|v| v.as_array()) {
        for s in arr {
            if let Some(span) = text_span_from_json(s) {
                spans.push(span);
            }
        }
    }
    node.spans = spans.clone();

    if let Some(v) = props.get("default_font_size").and_then(|v| v.as_f64()) {
        node.default_font_size = Animated::new(v as f32);
    }
    if let Some(s) = props.get("default_color").and_then(|v| v.as_str()) {
        if let Some(c) = parse_hex(s) {
            node.default_color = Animated::new(c);
        } else {
            node.default_color = Animated::new(crate::types::Color::WHITE);
        }
    } else {
        // Bootstrap a default so `node.init_paragraph` has something to draw.
        node.default_color = Animated::new(crate::types::Color::WHITE);
    }
    node.default_font_size.current_value = node.default_font_size.current_value.max(1.0);

    if let Some(v) = props.get("min_size").and_then(|v| v.as_f64()) {
        node.min_size = v as f32;
    }
    if let Some(v) = props.get("max_size").and_then(|v| v.as_f64()) {
        node.max_size = v as f32;
    }
    if let Some(s) = props.get("fit").and_then(|v| v.as_str()) {
        node.fit_mode = match s {
            "shrink" => crate::element::TextFit::Shrink,
            _ => crate::element::TextFit::None,
        };
    }
    if let Some(s) = props.get("text_align").and_then(|v| v.as_str()) {
        node.text_align = match s {
            "center" => skia_safe::textlayout::TextAlign::Center,
            "right" => skia_safe::textlayout::TextAlign::Right,
            "justify" => skia_safe::textlayout::TextAlign::Justify,
            _ => skia_safe::textlayout::TextAlign::Left,
        };
    }
    if props.contains_key("shadow_color") {
        let color = props
            .get("shadow_color")
            .and_then(|v| v.as_str())
            .and_then(parse_hex)
            .unwrap_or(crate::types::Color::BLACK);
        let blur = props
            .get("shadow_blur")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32;
        let offset_x = props
            .get("shadow_offset_x")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32;
        let offset_y = props
            .get("shadow_offset_y")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32;
        node.shadow = Some(crate::element::TextShadow {
            color,
            blur,
            offset: (offset_x, offset_y),
        });
    }
    // Need to recompute the Skia paragraph after span/style mutation.
    node.init_paragraph();
}

// ---- Image props ----

fn image_props_to_json(n: &crate::node::image_node::ImageNode) -> JsonMap<String, JsonValue> {
    let mut p = JsonMap::new();
    if let Some(path) = &n.source_path {
        p.insert("source".into(), JsonValue::from(path.clone()));
    }
    let fit = match n.object_fit {
        crate::types::ObjectFit::Fill => "fill",
        crate::types::ObjectFit::Contain => "contain",
        crate::types::ObjectFit::Cover => "cover",
    };
    p.insert("object_fit".into(), JsonValue::from(fit));
    p.insert(
        "border_radius".into(),
        JsonValue::from(n.border_radius.current_value),
    );
    p
}

fn apply_image_props(
    n: &mut crate::node::image_node::ImageNode,
    props: &JsonMap<String, JsonValue>,
    loader: &dyn AssetLoader,
) {
    if let Some(s) = props.get("source").and_then(|v| v.as_str()) {
        n.source_path = Some(s.to_string());
        if let Ok(bytes) = loader.load_bytes(s) {
            n.image = skia_safe::Image::from_encoded(skia_safe::Data::new_copy(&bytes));
        }
    }
    if let Some(s) = props.get("object_fit").and_then(|v| v.as_str()) {
        n.object_fit = match s {
            "contain" => crate::types::ObjectFit::Contain,
            "fill" => crate::types::ObjectFit::Fill,
            _ => crate::types::ObjectFit::Cover,
        };
    }
    if let Some(v) = props.get("border_radius").and_then(|v| v.as_f64()) {
        n.border_radius = Animated::new(v as f32);
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
    1.0
}

// ---- Save (Director -> Project) ----

pub fn director_to_project(d: &Director) -> Project {
    let mut nodes = Vec::new();
    let total = d.scene.nodes.len();
    for i in 0..total {
        if let Some(sn) = d.scene.get_node(i as NodeId) {
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
                    .map(text_props_to_json)
                    .unwrap_or_else(|| {
                        let mut p = JsonMap::new();
                        p.insert("_content".into(), JsonValue::from(""));
                        p.insert("font_size".into(), JsonValue::from(24.0));
                        p
                    }),
                "Image" => sn
                    .element
                    .as_any()
                    .downcast_ref::<crate::node::image_node::ImageNode>()
                    .map(image_props_to_json)
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
                id: i as NodeId,
                parent: sn.parent,
                kind,
                transform,
                opacity,
                z_index: sn.z_index,
                blend_mode: blend_to_str(sn.blend_mode),
                props,
                animations,
                layout,
            });
        }
    }
    let tracks = d
        .tracks
        .iter()
        .map(|t| TrackData {
            opacity: t.opacity,
            clips: t.clips.iter().map(track_clip_to_data).collect(),
        })
        .collect();
    let legacy_timeline = d
        .timeline
        .iter()
        .map(|it| TimelineItemData {
            scene_root: it.scene_root,
            start_time: it.start_time,
            duration: it.duration,
            z_index: it.z_index,
            name: it.name.clone(),
            audio_tracks: it.audio_tracks.clone(),
        })
        .collect();
    Project {
        version: PROJECT_VERSION,
        width: d.width,
        height: d.height,
        fps: d.fps,
        nodes,
        tracks,
        legacy_timeline,
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

    // First pass: create nodes (Box / Text / Image / placeholder Box for others).
    for n in &project.nodes {
        let new_id = match n.kind.as_str() {
            "Box" => {
                let mut b = BoxNode::new();
                apply_box_props(&mut b, &n.props);
                apply_layout_style_json(&mut b.style, &n.layout);
                director.scene.add_node(Box::new(b))
            }
            "Text" => {
                // Restore as a real TextNode. A TextNode with zero spans
                // would otherwise render nothing, so seed an empty span if
                // `apply_text_props` could not recover any from the JSON.
                let fc = director.assets.font_collection.clone();
                let mut t = TextNode::new(vec![crate::element::TextSpan {
                    text: String::new(),
                    ..Default::default()
                }], fc);
                apply_text_props(&mut t, &n.props);
                apply_layout_style_json(&mut t.style, &n.layout);
                director.scene.add_node(Box::new(t))
            }
            "Image" => {
                // Reconstruct as a real ImageNode, then re-load bytes via the
                // Director's AssetLoader if a source path was round-tripped.
                let mut i = crate::node::image_node::ImageNode::new(Vec::new());
                apply_image_props(&mut i, &n.props, director.assets.loader.as_ref());
                apply_layout_style_json(&mut i.style, &n.layout);
                director.scene.add_node(Box::new(i))
            }
            _ => {
                let mut b = BoxNode::new();
                b.opacity.current_value = n.opacity;
                apply_layout_style_json(&mut b.style, &n.layout);
                tracing::warn!(
                    "load_project: kind '{}' is not fully round-tripped; \
                     reconstructing as a BoxNode placeholder (id was {})",
                    n.kind,
                    n.id
                );
                director.scene.add_node(Box::new(b))
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
                }
                // TextNode: rebuild opacity from the `opacity` animation channel.
                if let Some(t) = sn.element.as_any_mut().downcast_mut::<TextNode>() {
                    if let Some(a) = n
                        .animations
                        .get("opacity")
                        .and_then(rebuild_animated_f32)
                    {
                        t.opacity = a;
                    }
                }
                // ImageNode: same treatment.
                if let Some(img) =
                    sn.element
                        .as_any_mut()
                        .downcast_mut::<crate::node::image_node::ImageNode>()
                {
                    if let Some(a) = n
                        .animations
                        .get("opacity")
                        .and_then(rebuild_animated_f32)
                    {
                        img.opacity = a;
                    }
                    if let Some(a) = n
                        .animations
                        .get("border_radius")
                        .and_then(rebuild_animated_f32)
                    {
                        img.border_radius = a;
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
                            fade_in: c.fade_in,
                            fade_out: c.fade_out,
                            audio_tracks: c.audio_tracks,
                        })
                    })
                    .collect(),
            }
        })
        .collect();

    director.timeline = project
        .legacy_timeline
        .into_iter()
        .filter_map(|it| {
            id_map.get(&it.scene_root).map(|&new_root| TimelineItem {
                scene_root: new_root,
                start_time: it.start_time,
                duration: it.duration,
                z_index: it.z_index,
                name: it.name,
                audio_tracks: it.audio_tracks,
            })
        })
        .collect();

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
