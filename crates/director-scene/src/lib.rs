//! Scene document for Director.
//!
//! This crate is data only. It does not link Skia, Taffy, or FFmpeg.
//! `director-core` exports a live `Director` into [`SceneDocument`]. A later
//! renderer reads the document back.

pub mod audio;
pub mod body;
pub mod caption;
pub mod channel;
pub mod effect;
pub mod entity;
pub mod html;
pub mod layout;
pub mod timeline;

pub use audio::{AudioEffect, AudioMix, AudioParamAnimation, AudioTrack};
pub use body::*;
pub use caption::{ActiveSpeakerCameraBody, CaptionBody, SmartCaptionBody};
pub use channel::{Channel, Easing, Keyframe, Rgba};
pub use effect::{Effect, EffectParam, Uniform};
pub use entity::{BoundingBox, Entity, EntityId, EntityKind, EntitySample};
pub use html::{HtmlBody, HtmlCaptureBody, HtmlVideoBody, SvgSmilBody};
pub use layout::LayoutStyle;
pub use timeline::{Clip, Timeline, Track, Transition};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Document version this crate reads and writes.
pub const VERSION: u32 = 2;

/// Failure while reading a scene document.
#[derive(Debug, thiserror::Error)]
pub enum SceneError {
    /// The bytes are not a scene document.
    #[error("scene json: {0}")]
    Json(#[from] serde_json::Error),
    /// `version` differs from [`VERSION`].
    #[error("unsupported scene version {found}; this crate reads version {VERSION}")]
    Version { found: u32 },
}

/// A whole composition, independent of the renderer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SceneDocument {
    /// [`VERSION`] at the time of writing.
    pub version: u32,
    /// Canvas width in pixels.
    pub width: i32,
    /// Canvas height in pixels.
    pub height: i32,
    /// Frames per second. Must be finite and positive to render.
    pub fps: f64,
    /// Horizontal device scale applied at render time.
    pub output_scale_x: f32,
    /// Vertical device scale applied at render time.
    pub output_scale_y: f32,
    /// Leave uncovered pixels clear.
    pub transparent_background: bool,
    pub motion_blur: MotionBlur,
    /// Occupied scene-graph nodes, in arena order.
    pub nodes: Vec<NodeRecord>,
    pub timeline: Timeline,
    pub audio: AudioMix,
    pub entities: Vec<Entity>,
    /// Media the nodes and audio tracks refer to, by id.
    pub assets: Vec<AssetRecord>,
}

/// Temporal supersampling.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MotionBlur {
    /// Samples blended per frame. `1` disables motion blur.
    pub samples_per_frame: u32,
    /// Shutter angle in degrees. `360` opens for the whole frame.
    pub shutter_angle: f32,
}

/// Asset table key, such as `image-0`.
pub type AssetId = String;

/// One piece of media.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AssetRecord {
    pub id: AssetId,
    /// MIME type, such as `image/png` or `audio/x-f32le`.
    pub media_type: String,
    pub source: AssetSource,
}

/// Where an asset's bytes live.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssetSource {
    /// A file the renderer opens.
    Path { path: String },
    /// The bytes in standard base64.
    Embedded { base64: String },
}

/// Stable identity. Matches `director-core`'s generational `NodeId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct NodeRef {
    /// Arena slot.
    pub index: u32,
    /// Slot generation. A recycled slot has a new generation.
    pub generation: u32,
}

/// One node plus the channels that move it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NodeRecord {
    /// Identity of this node.
    pub id: NodeRef,
    /// Parent, when this node is not a root.
    pub parent: Option<NodeRef>,
    /// Children in paint order before z-index sorting.
    pub children: Vec<NodeRef>,
    /// `Element::kind` tag: `box`, `text`, `image`, and so on.
    pub kind: String,
    /// Local stacking order. Higher paints later.
    pub z_index: i32,
    /// Blend mode in snake case, such as `src_over` or `multiply`.
    pub blend_mode: String,
    /// The subtree may be painted once and reused.
    pub is_static: bool,
    /// Taffy style for this node.
    pub layout: LayoutStyle,
    /// Affine channels and the pivot.
    pub transform: TransformChannels,
    /// Opacity channel. `1.0` is opaque.
    pub opacity: Channel,
    /// Node whose alpha masks this node.
    pub mask: Option<NodeRef>,
    /// CSS filter functions for the subtree.
    pub filter: Filter,
    pub path_animation: Option<PathAnimation>,
    pub path_morph: Option<PathMorph>,
    pub audio_bindings: Vec<AudioBinding>,
    /// Entities this node follows or avoids.
    pub entity_refs: Vec<EntityId>,
    /// Kind-specific fields.
    pub body: NodeBody,
}

/// Keyframed `f32` channels for the local transform.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TransformChannels {
    /// Horizontal translation in pixels.
    pub translate_x: Channel,
    /// Vertical translation in pixels.
    pub translate_y: Channel,
    /// Horizontal scale. `1.0` is unscaled.
    pub scale_x: Channel,
    /// Vertical scale. `1.0` is unscaled.
    pub scale_y: Channel,
    /// Rotation in degrees.
    pub rotation: Channel,
    /// Horizontal skew.
    pub skew_x: Channel,
    /// Vertical skew.
    pub skew_y: Channel,
    /// Pivot as a fraction of the node width.
    pub pivot_x: f32,
    /// Pivot as a fraction of the node height.
    pub pivot_y: f32,
}

impl Default for TransformChannels {
    /// The identity transform pivoting on the node center.
    fn default() -> Self {
        Self {
            translate_x: Channel::still(0.0),
            translate_y: Channel::still(0.0),
            scale_x: Channel::still(1.0),
            scale_y: Channel::still(1.0),
            rotation: Channel::still(0.0),
            skew_x: Channel::still(0.0),
            skew_y: Channel::still(0.0),
            pivot_x: 0.5,
            pivot_y: 0.5,
        }
    }
}

/// CSS filter values. Defaults leave the subtree unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Filter {
    pub blur: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    /// Degrees.
    pub hue_rotate: f32,
    pub invert: f32,
    pub sepia: f32,
    pub opacity: f32,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            blur: 0.0,
            brightness: 1.0,
            contrast: 1.0,
            saturation: 1.0,
            hue_rotate: 0.0,
            invert: 0.0,
            sepia: 0.0,
            opacity: 1.0,
        }
    }
}

/// Motion along a path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PathAnimation {
    /// SVG path data in node pixels.
    pub path: String,
    /// Distance along the path in `0.0..=1.0`.
    pub progress: Channel,
    /// Rotate the node to the path tangent.
    pub follow_tangent: bool,
    pub speed: f32,
    /// Repeat count. `-1` repeats forever.
    pub loop_count: i32,
    /// Seconds for one pass.
    pub duration: f64,
}

/// Motion along a path that morphs between two shapes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PathMorph {
    /// SVG path data at progress 0.
    pub from_path: String,
    /// SVG path data at progress 1.
    pub to_path: String,
    pub progress: Channel,
    pub follow_tangent: bool,
    pub speed: f32,
    pub loop_count: i32,
    pub duration: f64,
}

/// Maps audio band energy to a node property.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AudioBinding {
    /// Audio track slot.
    pub track: usize,
    /// `bass`, `mids` or `highs`.
    pub band: String,
    /// `scale`, `opacity`, `x`, `y` or `rotation`.
    pub property: String,
    pub min_value: f32,
    pub max_value: f32,
    pub smoothing: f32,
}

impl SceneDocument {
    /// Serialize with stable key order from the struct field order.
    pub fn to_json(&self) -> Result<String, SceneError> {
        Ok(serde_json::to_string(self)?)
    }

    /// Parse a document and reject a version this crate does not read.
    pub fn from_json(text: &str) -> Result<Self, SceneError> {
        let value: serde_json::Value = serde_json::from_str(text)?;
        let found = value.get("version").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        if found != VERSION {
            return Err(SceneError::Version { found });
        }
        Ok(serde_json::from_value(value)?)
    }

    /// JSON Schema for agents and editors.
    pub fn json_schema() -> schemars::schema::RootSchema {
        schemars::schema_for!(SceneDocument)
    }

    /// The asset with this id.
    pub fn asset(&self, id: &str) -> Option<&AssetRecord> {
        self.assets.iter().find(|asset| asset.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SceneDocument {
        SceneDocument {
            version: VERSION,
            width: 64,
            height: 32,
            fps: 30.0,
            output_scale_x: 1.0,
            output_scale_y: 1.0,
            transparent_background: false,
            motion_blur: MotionBlur {
                samples_per_frame: 1,
                shutter_angle: 180.0,
            },
            nodes: vec![NodeRecord {
                id: NodeRef {
                    index: 0,
                    generation: 1,
                },
                parent: None,
                children: vec![],
                kind: "box".to_string(),
                z_index: 0,
                blend_mode: "src_over".to_string(),
                is_static: true,
                layout: LayoutStyle::default(),
                transform: TransformChannels {
                    translate_x: Channel::still(0.0),
                    translate_y: Channel::still(0.0),
                    scale_x: Channel::still(1.0),
                    scale_y: Channel::still(1.0),
                    rotation: Channel::still(0.0),
                    skew_x: Channel::still(0.0),
                    skew_y: Channel::still(0.0),
                    pivot_x: 0.5,
                    pivot_y: 0.5,
                },
                opacity: Channel {
                    keyframes: vec![
                        Keyframe {
                            time: 0.0,
                            value: 0.0,
                            easing: Easing::Linear,
                        },
                        Keyframe {
                            time: 1.0,
                            value: 1.0,
                            easing: Easing::CubicBezier {
                                x1: 0.2,
                                y1: 0.0,
                                x2: 0.0,
                                y2: 1.0,
                            },
                        },
                    ],
                },
                mask: None,
                filter: Filter::default(),
                path_animation: None,
                path_morph: None,
                audio_bindings: vec![],
                entity_refs: vec![],
                body: NodeBody::Box(BoxBody {
                    background: Some(Channel::still(Rgba {
                        r: 1.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    })),
                    background_gradient: None,
                    blur: Channel::still(0.0),
                    shadow_color: None,
                    shadow_blur: Channel::still(0.0),
                    shadow_offset_x: Channel::still(0.0),
                    shadow_offset_y: Channel::still(0.0),
                    border_radius: Channel::still(4.0),
                    border_width: Channel::still(0.0),
                    border_color: None,
                    overflow: "visible".to_string(),
                    backdrop_blur: Channel::still(0.0),
                    extra_shadows: vec![],
                    clip_path: None,
                }),
            }],
            timeline: Timeline::default(),
            audio: AudioMix::default(),
            entities: vec![],
            assets: vec![AssetRecord {
                id: "image-0".to_string(),
                media_type: "image/png".to_string(),
                source: AssetSource::Path {
                    path: "logo.png".to_string(),
                },
            }],
        }
    }

    #[test]
    fn json_round_trip_keeps_channels() {
        let document = sample();
        let parsed = SceneDocument::from_json(&document.to_json().unwrap()).unwrap();
        assert_eq!(parsed, document);
    }

    #[test]
    fn a_different_version_is_rejected() {
        let mut document = sample();
        document.version = VERSION + 1;
        let text = document.to_json().unwrap();
        let error = SceneDocument::from_json(&text).unwrap_err();
        assert!(error.to_string().contains("unsupported scene version"));
    }

    #[test]
    fn schema_names_the_document() {
        let schema = SceneDocument::json_schema();
        let json = serde_json::to_value(schema).unwrap();
        assert!(json["title"].as_str().unwrap().contains("SceneDocument"));
    }

    #[test]
    fn default_layout_writes_no_fields() {
        let json = serde_json::to_string(&LayoutStyle::default()).unwrap();
        assert_eq!(json, "{}");
    }
}
