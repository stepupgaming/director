//! Scene document for Director.
//!
//! This crate is data only. It does not link Skia, Taffy, or FFmpeg.
//! `director-core` exports a live `Director` into [`SceneDocument`]. A later
//! renderer reads the document back.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Document version this crate reads and writes.
pub const VERSION: u32 = 1;

/// Failure while reading a scene document.
#[derive(Debug, thiserror::Error)]
pub enum SceneError {
    /// The bytes are not a scene document.
    #[error("scene json: {0}")]
    Json(#[from] serde_json::Error),
    /// `version` is newer than [`VERSION`].
    #[error("unsupported scene version {found}; this crate reads version 1")]
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
    /// Occupied scene-graph nodes, in arena order.
    pub nodes: Vec<NodeRecord>,
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
    /// Skia blend-mode debug name, such as `SrcOver`.
    pub blend_mode: String,
    /// The subtree may be painted once and reused.
    pub is_static: bool,
    /// Affine channels and the pivot.
    pub transform: TransformChannels,
    /// Opacity channel. `1.0` is opaque.
    pub opacity: Channel,
    /// Kind-specific fields. Unknown kinds use [`NodeBody::Unspecified`].
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

/// One animated scalar.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Channel {
    /// Samples in time order. A still value is one keyframe at `t = 0`.
    pub keyframes: Vec<Keyframe>,
}

/// A sample on a [`Channel`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Keyframe {
    /// Time in seconds from the start of the channel.
    pub time: f64,
    /// Value at `time`.
    pub value: f32,
    /// Easing into this sample. `linear` for the first sample.
    pub easing: String,
}

/// sRGB color with straight alpha, each component in `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rgba {
    /// Red.
    pub r: f32,
    /// Green.
    pub g: f32,
    /// Blue.
    pub b: f32,
    /// Alpha. `0.0` is clear.
    pub a: f32,
}

/// Fields that depend on the node kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeBody {
    /// A colored rectangle. Width and height are set only for length sizes.
    Box {
        /// Current background. Absent when the box has no fill.
        background: Option<Rgba>,
        /// Explicit width in pixels.
        width: Option<f32>,
        /// Explicit height in pixels.
        height: Option<f32>,
    },
    /// A text run. Channels for size and color are the current values.
    Text {
        /// Concatenated span text.
        content: String,
        /// Font size in pixels.
        font_size: f32,
        /// Fill color.
        color: Rgba,
    },
    /// The exporter does not yet copy this kind's fields.
    Unspecified,
}

impl SceneDocument {
    /// Serialize with stable key order from the struct field order.
    pub fn to_json(&self) -> Result<String, SceneError> {
        Ok(serde_json::to_string(self)?)
    }

    /// Parse a document and reject a version this crate does not read.
    pub fn from_json(text: &str) -> Result<Self, SceneError> {
        let document: Self = serde_json::from_str(text)?;
        if document.version != VERSION {
            return Err(SceneError::Version {
                found: document.version,
            });
        }
        Ok(document)
    }

    /// JSON Schema for agents and editors.
    pub fn json_schema() -> schemars::schema::RootSchema {
        schemars::schema_for!(SceneDocument)
    }
}

impl Channel {
    /// A channel that holds `value` at time 0 with linear easing.
    pub fn still(value: f32) -> Self {
        Self {
            keyframes: vec![Keyframe {
                time: 0.0,
                value,
                easing: "linear".to_string(),
            }],
        }
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
            nodes: vec![NodeRecord {
                id: NodeRef {
                    index: 0,
                    generation: 1,
                },
                parent: None,
                children: vec![],
                kind: "box".to_string(),
                z_index: 0,
                blend_mode: "SrcOver".to_string(),
                is_static: true,
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
                            easing: "linear".to_string(),
                        },
                        Keyframe {
                            time: 1.0,
                            value: 1.0,
                            easing: "ease_out".to_string(),
                        },
                    ],
                },
                body: NodeBody::Box {
                    background: Some(Rgba {
                        r: 1.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    }),
                    width: Some(32.0),
                    height: Some(16.0),
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
    fn a_newer_version_is_rejected() {
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
}
